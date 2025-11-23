use std::{collections::HashMap, hash::{DefaultHasher, Hash, Hasher}};

use crate::{alloc::Alloc, str::Str, vm::Value};

#[derive(Clone)]
pub struct Dict {
    table: *mut [Option<(Str, Value)>],
    len: usize
}

const THRESHOLD: usize = 75;

impl Dict {
    fn empty_zeroed_table(capacity: usize, alloc: &mut Alloc) -> Result<*mut [Option<(Str, Value)>], ()> {
        let table = alloc.alloc_table(capacity)?;
        for elem in unsafe { &mut *table } {
            *elem = None;
        }
        Ok(table)
    }

    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Result<Self, ()>
    {
        let capacity = std::cmp::max(capacity, 1);
        let table = Self::empty_zeroed_table(capacity, alloc)?;
        Ok(Dict { table, len: 0 })
    }

    fn get_slot<'a>(&'a mut self, key: &str) -> &'a mut Option<(Str, Value)> {
        let table = unsafe { &mut *self.table };
        let len = table.len();
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        let mut pos = usize::try_from(hash).unwrap_or(usize::MAX);

        loop {
            match &table[pos % len] {
                Some(ref val) => {
                    if val.0.as_str() != key {
                        pos += 1;
                        continue;
                    }
                }
                None => ()
            }

            break;
        }

        &mut table[pos % len]

    }

    fn double_size(&mut self, alloc: &mut Alloc) -> Result<(), ()> {
        let old_table = unsafe { &* self.table };
        let new_table = Self::empty_zeroed_table((old_table.len() + 1) * 2, alloc)?;

        self.table = new_table;

        for entry in old_table {
            if let Some((key, val)) = entry {
                self.set(key.as_str(), *val, alloc).unwrap();
            }
        }

        Ok(())
    }

    // Set the value associated with a given field
    pub fn set(&mut self, field_name: &str, new_val: Value, alloc: &mut Alloc) -> Result<(), ()> {
        let table = unsafe { &*self.table };
        if table.len() == 0 || self.len * 100 / table.len() > THRESHOLD {
            self.double_size(alloc)?;
        }

        let slot = self.get_slot(field_name);
        let key = alloc.raw_str(field_name)?;
        *slot = Some((key, new_val));
        self.len += 1;

        Ok(())

    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Value {
        self.get_slot(field_name).map(|v| v.1).unwrap_or(Value::Nil)
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        let table = unsafe { &*self.table };
        table.iter().filter_map(|e| e.as_ref().map(|e| &e.1))
    }

    pub fn values_mut(&self) -> impl Iterator<Item = &mut Value> {
        let table = unsafe { &mut *self.table };
        table.iter_mut().filter_map(|e| e.as_mut().map(|e| &mut e.1))
    }

    pub fn has(&mut self, field_name: &str) -> bool {
        self.get_slot(field_name).is_some()
    }
}
