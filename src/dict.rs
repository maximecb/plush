use std::collections::HashMap;

use crate::vm::Value;

#[derive(Clone, Default)]
pub struct Dict {
    pub hash: HashMap<String, Value>,
}

impl Dict {
    // Set the value associated with a given field
    pub fn set(&mut self, field_name: &str, new_val: Value) {
        self.hash.insert(field_name.to_string(), new_val);
    }

    // Get the value associated with a given field
    pub fn get(&mut self, field_name: &str) -> Value {
        if let Some(val) = self.hash.get(field_name) {
            *val
        } else {
            panic!("key `{}` not found in dict", field_name);
        }
    }

    // Check if the dictionary has a given key
    pub fn has(&mut self, field_name: &str) -> bool {
        self.hash.contains_key(field_name)
    }
}
