use std::{collections::HashMap, hash::{DefaultHasher, Hash, Hasher}, ops::Deref};

use crate::{alloc::{Alloc, Tag, HEADER_SIZE}, str::Str, vm::Value};

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

    fn key_value_mut(&mut self) -> Option<(&mut *const Str, &mut Value)> {
        if self.key.is_null() {
            None
        } else {
            Some((&mut self.key, &mut self.val))
        }
    }

    fn is_occupied(&self) -> bool {
        !self.key.is_null()
    }
}

pub struct Dict {
    // Relocated by the collector, which walks the table on its own
    pub(crate) table: *mut [TableSlot],
    len: usize
}

const THRESHOLD: usize = 75;

impl Dict {
    /// Bytes a dict with a given capacity occupies, counting the headers
    /// of both the dict and its slot table
    pub fn alloc_size(capacity: usize) -> usize {
        HEADER_SIZE + size_of::<Dict>() +
        HEADER_SIZE + std::cmp::max(capacity, 2) * size_of::<TableSlot>()
    }

    fn empty_zeroed_table(capacity: usize, alloc: &mut Alloc) -> *mut [TableSlot] {
        let table = alloc.alloc_table::<TableSlot>(capacity, Tag::SlotTable);

        // Lookups probe until they land on an empty slot, so a table
        // handed back with stale keys in it would loop forever rather
        // than fail. Nothing else notices this, so check it here.
        #[cfg(feature = "verify_gc")]
        assert!(
            unsafe { &*table }.iter().all(|slot| !slot.is_occupied()),
            "dict table allocated over memory that was not zeroed"
        );

        table
    }

    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Self
    {
        let capacity = std::cmp::max(capacity, 2);
        let table = Self::empty_zeroed_table(capacity, alloc);
        Dict { table, len: 0 }
    }

    pub fn clone(&self, alloc: &mut Alloc) -> Self
    {
        let capacity = std::cmp::max(self.capacity(), 1);
        let table = Self::empty_zeroed_table(capacity, alloc);
        let new_dict = Dict { table, len: self.len };
        let table = unsafe { &mut *table };
        let self_table = unsafe { &*self.table };
        table.copy_from_slice(self_table);
        new_dict
    }

    // get slot is the heart of the dict implementation, as it's used for both
    // getting and setting values. it hashes the key and tries to find the slot where the key
    // should go. The hashing algorithm we use is the default one that rust stdlib ships with.
    // We then use linear probing to deal with collisions.
    fn get_slot<'a>(&'a mut self, key: &str) -> &'a mut TableSlot {
        let table = unsafe { &mut *self.table };
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
        let old_table = unsafe { &* self.table };

        let new_table = Self::empty_zeroed_table((old_table.len() + 1) * 2, alloc);

        self.table = new_table;

        for entry in old_table {
            if let Some((key, val)) = entry.key_value() {
                self.set(*key, *val, alloc);
            }
        }
    }

    pub fn capacity(&self) -> usize {
        self.table.len()
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
        let table = unsafe { &*self.table };

        // Note: we must never end up in a situation where there are no
        // free slots after an element is added, because the get_slot method
        // relies on there always being at least one free slot.
        (self.len + 1 == table.len()) ||
        (100 * self.len / table.len() > THRESHOLD)
    }

    // Set the value associated with a given key
    pub fn set(&mut self, field_name: *const Str, new_val: Value, alloc: &mut Alloc) {
        if self.will_allocate_on_set() {
            self.double_size(alloc);
        }

        let key = unsafe { &*field_name }.as_str();
        let slot = self.get_slot(key);
        *slot = TableSlot::new(field_name, new_val);
        self.len += 1;
    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Option<Value> {
        (self.get_slot(field_name).value()).copied()
    }

    pub fn key_values_mut(&self) -> impl Iterator<Item = (&mut *const Str, &mut Value)> {
        let table = unsafe { &mut *self.table };
        table.iter_mut().filter_map(|e| e.key_value_mut())
    }

    pub fn has(&mut self, field_name: &str) -> bool {
        self.get_slot(field_name).is_occupied()
    }
}
