use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem;
use crate::alloc::Alloc;
use crate::vm::{Value, Closure, Object};

/// Custom Hash implementation for Value
impl Hash for Value
{
    fn hash<H: Hasher>(&self, state: &mut H)
    {
        // First hash the discriminant to differentiate between variants
        mem::discriminant(self).hash(state);

        // Hash the raw pointer address for each variant
        use Value::*;
        match self {
            String(ptr) => {
                // TODO: we could do an actual string hash here
                // to deduplicate identical strings, but we also
                // need proper string value eq
                let addr = *ptr as usize;
                addr.hash(state);
            },

            Closure(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            Object(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            _ => panic!("hash on non-heap value")
        }
    }
}

fn deepcopy(src_val: Value, dst_alloc: &mut Alloc) -> Value
{
    if !src_val.is_heap() {
        return src_val;
    }

    // Mapping from old to new addresses
    let dst_map: HashMap<Value, Value> = HashMap::new();

    // Stack of values to visit
    let mut stack: Vec<Value> = Vec::new();

    stack.push(src_val);

    // We need to keep a mapping from source to dst...
    // We'll use that to translate src_val

    while stack.len() > 0 {





    }



    // TODO: iterate over dst_map to translate the pointers




    *dst_map.get(&src_val).unwrap()
}

#[cfg(test)]
mod tests
{
    use super::*;


    #[test]
    fn copy_atoms()
    {




    }



}
