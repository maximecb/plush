use std::{collections::HashMap, hash::{DefaultHasher, Hash, Hasher}, ops::Deref};

use crate::{alloc::Alloc, str::Str, vm::Value};

#[derive(Clone, Copy)]
struct TableSlot {
    key: *const Str,
    val: Value
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

    fn value_mut(&mut self) -> Option<&mut Value> {
        if self.key.is_null() {
            None
        } else {
            Some(&mut self.val)
        }
    }

    fn key_value(&self) -> Option<(&str, &Value)> {
        if self.key.is_null() {
            None
        } else {
            Some((unsafe { &*self.key }.as_str(), &self.val))
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
    table: *mut [TableSlot],
    len: usize
}

const THRESHOLD: usize = 75;

impl Dict {
    fn empty_zeroed_table(capacity: usize, alloc: &mut Alloc) -> Result<*mut [TableSlot], ()> {
        let table = alloc.alloc_table(capacity)?;
        Ok(table)
    }

    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Result<Self, ()>
    {
        let capacity = std::cmp::max(capacity, 1);
        let table = Self::empty_zeroed_table(capacity, alloc)?;
        Ok(Dict { table, len: 0 })
    }

    pub fn clone(&self, alloc: &mut Alloc) -> Result<Self, ()>
    {
        let capacity = std::cmp::max(self.capacity(), 1);
        let table = Self::empty_zeroed_table(capacity, alloc)?;
        let mut new_dict = Dict { table, len: self.len };
        let table = unsafe { &mut *table };
        let self_table = unsafe { &*self.table };
        table.copy_from_slice(self_table);
        Ok(new_dict)
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

        // have to module by len so that it's always inside the table
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
    fn double_size(&mut self, alloc: &mut Alloc) -> Result<(), ()> {
        let old_table = unsafe { &* self.table };
        let new_table = Self::empty_zeroed_table((old_table.len() + 1) * 2, alloc)?;

        self.table = new_table;

        for entry in old_table {
            if let Some((key, val)) = entry.key_value() {
                self.set(key, *val, alloc).unwrap();
            }
        }

        Ok(())
    }

    pub fn capacity(&self) -> usize {
        self.table.len()
    }

    fn will_allocate_on_set(&self) -> bool {
        let table = unsafe { &*self.table };

        table.len() == 0 || self.len * 100 / table.len() > THRESHOLD
    }

    pub const fn size_of_slot() -> usize {
        size_of::<TableSlot>()
    }

    pub fn will_allocate(&self, field_name: &str) -> usize {
        let mut res = 0;
        res += field_name.len();

        if self.will_allocate_on_set() {
            let table = unsafe { &*self.table };

            for elem in table {
                if let Some(key) = elem.key_as_str() {
                    res += key.len();
                    res += size_of::<Str>();
                }
            }

            res += self.capacity() * Dict::size_of_slot() * 2;
        }


        res
    }

    // Set the value associated with a given key
    pub fn set(&mut self, field_name: &str, new_val: Value, alloc: &mut Alloc) -> Result<(), ()> {
        if self.will_allocate_on_set() {
            self.double_size(alloc)?;
        }

        let slot = self.get_slot(field_name);
        let key = alloc.str(field_name)?;
        *slot = TableSlot::new(key, new_val);
        self.len += 1;

        Ok(())

    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Value {
        *(self.get_slot(field_name).value().unwrap_or(&Value::Nil))
    }

    pub fn key_values_mut(&self) -> impl Iterator<Item = (&mut *const Str, &mut Value)> {
        let table = unsafe { &mut *self.table };
        table.iter_mut().filter_map(|e| e.key_value_mut())
    }

    pub fn has(&mut self, field_name: &str) -> bool {
        self.get_slot(field_name).is_occupied()
    }
}
