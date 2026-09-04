use std::hash::{DefaultHasher, Hash, Hasher};

use crate::{
    alloc::{
        header_of, set_fixed_len, table_len, table_slice, table_slice_mut,
        Alloc, Tag, FIXED_SIZE, HEADER_SIZE,
    },
    str::Str,
    value::Value,
};

#[derive(Clone, Copy)]
pub(crate) struct TableSlot {
    // Scanned by the collector, which walks slot tables on their own
    pub(crate) key: *const Str,
    pub(crate) val: Value
}

impl TableSlot {
    fn new(key: *const Str, val: Value) -> Self {
        Self{ key, val }
    }

    fn key_as_str(&self) -> Option<&str> {
        if self.key.is_null() {
            None
        } else {
            Some(unsafe { &*self.key }.as_str())
        }
    }

    fn value(&self) -> Option<&Value> {
        if self.key.is_null() {
            None
        } else {
            Some(&self.val)
        }
    }

    fn key_value(&self) -> Option<(&*const Str, &Value)> {
        if self.key.is_null() {
            None
        } else {
            Some((&self.key , &self.val))
        }
    }

    fn is_occupied(&self) -> bool {
        !self.key.is_null()
    }
}

pub struct Dict {
    // Relocated by the collector, which walks the table on its own.
    // The capacity is the length of the table block this points at, so
    // it is read back from that block's header rather than stored here
    pub(crate) table: *mut TableSlot,
}

const THRESHOLD: usize = 75;

// Nothing but the pointer to the slot table. The entry count lives in
// the block header, which is what makes this a fixed-layout block.
const _: () = assert!(size_of::<Dict>() == FIXED_SIZE);

impl Dict {
    /// Bytes a dict with a given capacity occupies, counting the headers
    /// of both the dict and its slot table
    pub fn alloc_size(capacity: usize) -> usize {
        HEADER_SIZE + size_of::<Dict>() +
        HEADER_SIZE + std::cmp::max(capacity, 2) * size_of::<TableSlot>()
    }

    fn empty_zeroed_table(capacity: usize, alloc: &mut Alloc) -> *mut TableSlot {
        let table = alloc.alloc_table::<TableSlot>(capacity, Tag::SlotTable);

        // Lookups probe until they land on an empty slot, so a table
        // handed back with stale keys in it would loop forever rather
        // than fail. Nothing else notices this, so check it here.
        #[cfg(feature = "verify_gc")]
        assert!(
            table_slice(table).iter().all(|slot| !slot.is_occupied()),
            "dict table allocated over memory that was not zeroed"
        );

        table
    }

    /// Allocate an empty dict with room for a given number of entries.
    ///
    /// The dict is allocated before its table, so that the two end up in
    /// the same order the collector puts them in, with the slots
    /// following the dict they belong to. No collection can happen
    /// between the two allocations: callers reserve the space up front.
    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Value
    {
        let capacity = std::cmp::max(capacity, 2);
        // alloc_fixed leaves the table pointer null, so the dict is
        // walkable between here and the table allocation below
        let dict = alloc.alloc_fixed(Tag::Dict, 0) as *mut Dict;
        unsafe { (*dict).table = Self::empty_zeroed_table(capacity, alloc) };

        Value::dict(dict)
    }

    fn block(&self) -> *mut u8 {
        self as *const Dict as *mut u8
    }

    /// Number of entries held, which lives in the block header
    pub fn len(&self) -> usize {
        header_of(self.block()).fixed_len()
    }

    fn set_len(&mut self, len: usize) {
        set_fixed_len(self.block(), len);
    }

    // get slot is the heart of the dict implementation, as it's used for both
    // getting and setting values. it hashes the key and tries to find the slot where the key
    // should go. The hashing algorithm we use is the default one that rust stdlib ships with.
    // We then use linear probing to deal with collisions.
    fn get_slot<'a>(&'a mut self, key: &str) -> &'a mut TableSlot {
        let table = table_slice_mut(self.table);
        let len = table.len();
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        let mut pos = usize::try_from(hash).unwrap_or(usize::MAX);

        // Have to modulo by len so that it's always inside the table
        while let Some(slot_key) = table[pos % len].key_as_str() {
            // we found an occupied slot for the given key (the key already existed in the dict)
            if slot_key == key {
                break;
            }

            // linear probing on occupied slot
            pos += 1;
        }

        &mut table[pos % len]
    }

    // Double the size of the internal backing table. This allocates a whole new backing table
    // and rehashes all entries into it
    fn double_size(&mut self, alloc: &mut Alloc) {
        let old_table = table_slice(self.table);

        let new_table = Self::empty_zeroed_table((old_table.len() + 1) * 2, alloc);

        self.table = new_table;

        // The entries are counted again as they are put back in
        self.set_len(0);

        for entry in old_table {
            if let Some((key, val)) = entry.key_value() {
                self.set(*key, *val, alloc);
            }
        }
    }

    pub fn capacity(&self) -> usize {
        table_len(self.table)
    }

    pub const fn size_of_slot() -> usize {
        size_of::<TableSlot>()
    }

    /// Bytes the next set may need for a bigger table, zero if it fits
    pub fn will_allocate(&self) -> usize {
        if self.will_allocate_on_set() {
            return HEADER_SIZE + (self.capacity() + 1) * 2 * Dict::size_of_slot();
        }

        0
    }

    fn will_allocate_on_set(&self) -> bool {
        let capacity = self.capacity();
        let len = self.len();

        // Note: we must never end up in a situation where there are no
        // free slots after an element is added, because the get_slot method
        // relies on there always being at least one free slot.
        (len + 1 == capacity) ||
        (100 * len / capacity > THRESHOLD)
    }

    // Set the value associated with a given key
    pub fn set(&mut self, field_name: *const Str, new_val: Value, alloc: &mut Alloc) {
        if self.will_allocate_on_set() {
            self.double_size(alloc);
        }

        let key = unsafe { &*field_name }.as_str();
        let slot = self.get_slot(key);
        let was_occupied = slot.is_occupied();
        *slot = TableSlot::new(field_name, new_val);

        // Overwriting a key that is already there adds no entry
        if !was_occupied {
            let len = self.len();
            self.set_len(len + 1);
        }
    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Option<Value> {
        (self.get_slot(field_name).value()).copied()
    }

    pub fn has(&mut self, field_name: &str) -> bool {
        self.get_slot(field_name).is_occupied()
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    /// Set a key given as a Rust string. No collection can happen here,
    /// so the dict and the keys stay put for the life of the test
    fn set(dict: &mut Value, key: &str, val: i64, alloc: &mut Alloc)
    {
        let key = Str::new(key, alloc);
        dict.as_dict().set(key.as_string() as *const Str, Value::fixnum(val), alloc);
    }

    /// Overwriting a key must not count as a new entry, or the table
    /// keeps growing under a dict that only ever holds one thing
    #[test]
    fn overwrite_keeps_len()
    {
        let mut alloc = Alloc::new();
        let mut dict = Dict::with_capacity(0, &mut alloc);

        // The growth check runs before the key is looked up, so it cannot
        // tell an overwrite from an insert and grows the smallest table
        // once regardless. It has to settle there
        set(&mut dict, "x", 0, &mut alloc);
        set(&mut dict, "x", 1, &mut alloc);
        let capacity = dict.as_dict().capacity();

        for i in 2..100 {
            set(&mut dict, "x", i, &mut alloc);
            assert_eq!(dict.as_dict().len(), 1);
            assert_eq!(dict.as_dict().capacity(), capacity);
        }

        assert_eq!(dict.as_dict().get("x"), Some(Value::fixnum(99)));
    }

    /// The count has to survive the rehash, which puts every entry back
    /// through the same set path
    #[test]
    fn len_survives_growth()
    {
        let mut alloc = Alloc::new();
        let mut dict = Dict::with_capacity(0, &mut alloc);

        for i in 0..200 {
            set(&mut dict, &i.to_string(), i, &mut alloc);
            assert_eq!(dict.as_dict().len(), (i + 1) as usize);
        }

        // Overwrites after a rehash are still overwrites
        for i in 0..200 {
            set(&mut dict, &i.to_string(), i * 2, &mut alloc);
        }
        assert_eq!(dict.as_dict().len(), 200);

        for i in 0..200 {
            let d = dict.as_dict();
            assert_eq!(d.get(&i.to_string()), Some(Value::fixnum(i * 2)));
        }
    }

    /// The table has to keep a free slot for the probe loop to stop on,
    /// however the entries got there
    #[test]
    fn growth_keeps_a_free_slot()
    {
        let mut alloc = Alloc::new();
        let mut dict = Dict::with_capacity(0, &mut alloc);

        for i in 0..200 {
            // Every key is set twice, so the count only moves on the
            // first of each pair
            set(&mut dict, &(i / 2).to_string(), i, &mut alloc);

            let d = dict.as_dict();
            assert_eq!(d.len(), (i / 2 + 1) as usize);
            assert!(d.len() < d.capacity(), "no free slot left in the table");
        }
    }
}
