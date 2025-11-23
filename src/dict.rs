use std::{collections::HashMap, hash::{DefaultHasher, Hash, Hasher}, ops::Deref};

use crate::{alloc::Alloc, str::Str, vm::Value};

#[derive(Clone, Copy)]
struct TableSlot(Option<(Str, Value)>);

impl TableSlot {
    fn new(key: Str, val: Value) -> Self {
        Self(Some((key, val)))
    }

    fn key(&self) -> Option<&str> {
        match self.0.as_ref() {
            Some(s) => Some(s.0.as_str()),
            None => None
        }
    }

    fn value(&self) -> Option<&Value> {
        match self.0.as_ref() {
            Some(s) => Some(&s.1),
            None => None
        }
    }

    fn value_mut(&mut self) -> Option<&mut Value> {
        match self.0.as_mut() {
            Some(s) => Some(&mut s.1),
            None => None
        }
    }

    fn key_value(&self) -> Option<(&str, &Value)> {
        match self.0.as_ref() {
            Some(s) => Some((s.0.as_str(), &s.1)),
            None => None
        }
    }

    fn is_occupied(&self) -> bool {
        self.0.is_some()
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
        for elem in unsafe { &mut *table } {
            *elem = TableSlot(None);
        }
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
        let capacity = std::cmp::max(self.len, 1);
        let table = alloc.alloc_table(capacity)?;
        let mut new_arr = Dict { table, len: self.len };
        let table = unsafe { &mut *table };
        let self_table = unsafe { &*self.table };
        table.copy_from_slice(self_table);
        Ok(new_arr)
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
        while let Some(slot_key) = table[pos % len].key() {
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

    // Set the value associated with a given key
    pub fn set(&mut self, field_name: &str, new_val: Value, alloc: &mut Alloc) -> Result<(), ()> {
        let table = unsafe { &*self.table };
        if table.len() == 0 || self.len * 100 / table.len() > THRESHOLD {
            self.double_size(alloc)?;
        }

        let slot = self.get_slot(field_name);
        let key = alloc.raw_str(field_name)?;
        *slot = TableSlot::new(key, new_val);
        self.len += 1;

        Ok(())

    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Value {
        *(self.get_slot(field_name).value().unwrap_or(&Value::Nil))
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let table = unsafe { &*self.table };
        table.iter().filter_map(|e| e.value())
    }

    pub fn values_mut(&self) -> impl Iterator<Item = &mut Value> {
        let table = unsafe { &mut *self.table };
        table.iter_mut().filter_map(|e| e.value_mut())
    }

    pub fn has(&mut self, field_name: &str) -> bool {
        self.get_slot(field_name).is_occupied()
    }
}
