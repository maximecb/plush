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
                // Hash the string
                // This will deduplicate identical strings
                let s: &str = unsafe { (**ptr).as_str() };
                s.hash(state);
            },

            Closure(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            Object(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            Array(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            ByteArray(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            Dict(ptr) => {
                let addr = *ptr as usize;
                addr.hash(state);
            },

            _ => panic!("hash on non-heap value")
        }
    }
}

pub fn deepcopy(
    src_val: Value,
    dst_alloc: &mut Alloc,
    dst_map: &mut HashMap<Value, Value>,
) -> Value
{
    if !src_val.is_heap() {
        return src_val;
    }

    // Stack of values to visit
    let mut stack: Vec<Value> = Vec::new();

    // Queue the source value to be translated
    stack.push(src_val);

    macro_rules! push_val {
        ($val: expr) => {
            if $val.is_heap() {
                stack.push(*$val);
            }
        }
    }

    while stack.len() > 0 {
        let val = stack.pop().unwrap();

        // If this value has already been remapped, skip it
        if dst_map.contains_key(&val) {
            continue;
        }

        // We should only queue heap values for performance
        assert!(val.is_heap());

        let new_val = match val {
            Value::String(p) => {
                dst_alloc.str_val(unsafe { (*p).as_str() })
            }

            Value::Closure(p) => {
                let new_clos = unsafe { (*p).clone() };

                for val in &new_clos.slots {
                    push_val!(val);
                }

                Value::Closure(dst_alloc.alloc(new_clos))
            }

            Value::Dict(p) => {
                let new_obj = unsafe { (*p).clone() };

                for val in new_obj.hash.values() {
                    push_val!(val);
                }

                Value::Dict(dst_alloc.alloc(new_obj))
            }

            Value::Object(p) => {
                let new_obj = unsafe { (*p).clone() };

                for i in 0..new_obj.num_slots() {
                    let val = new_obj.get(i);
                    push_val!(&val);
                }

                Value::Object(dst_alloc.alloc(new_obj))
            }

            Value::Array(p) => {
                let new_arr = unsafe { (*p).clone() };

                for val in &new_arr.elems {
                    push_val!(val);
                }

                Value::Array(dst_alloc.alloc(new_arr))
            }

            Value::ByteArray(p) => {
                let new_arr = unsafe { (*p).clone() };
                Value::ByteArray(dst_alloc.alloc(new_arr))
            }

            _ => panic!("deepcopy unimplemented for type {:?}", val)
        };

        // Insert the new mapping into the translation map
        dst_map.insert(val, new_val);
    }

    *dst_map.get(&src_val).unwrap()
}

/// Remap internal references to copied values
pub fn remap(dst_map: HashMap<Value, Value>)
{
    macro_rules! remap_val {
        ($val: expr) => {
            if ($val.is_heap()) {
                let new_val = dst_map.get($val);
                assert!(new_val.is_some(), "could not remap val: {:?}", $val);
                *$val = *new_val.unwrap();
            }
        }
    }

    // For each already translated heap object
    for (_, val) in dst_map.iter() {
        match val {
            Value::String(_) => {}

            Value::Closure(p) => {
                let clos = unsafe { &mut **p };
                for slot_val in &mut clos.slots {
                    remap_val!(slot_val);
                }
            }

            Value::Dict(p) => {
                let dict = unsafe { &mut **p };
                for val in dict.hash.values_mut() {
                    remap_val!(val);
                }
            }

            Value::Object(p) => {
                let obj = unsafe { &mut **p };
                for i in 0..obj.num_slots() {
                    let mut val = obj.get(i);
                    remap_val!(&mut val);
                    obj.set(i, val);
                }
            }

            Value::Array(p) => {
                let arr = unsafe { &mut **p };
                for val in &mut arr.elems {
                    remap_val!(val);
                }
            }

            Value::ByteArray(_) => {
                // Bytes don't need to be remapped
            }

            _ => panic!()
        }
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn copy_int()
    {
        let mut dst_alloc = Alloc::new();
        let mut dst_map = HashMap::new();
        let v1 = Value::Int64(1337);
        let v2 = deepcopy(v1, &mut dst_alloc, &mut dst_map);
        assert!(v1 == v2);
    }

    #[test]
    fn copy_string()
    {
        let mut src_alloc = Alloc::new();
        let mut dst_alloc = Alloc::new();
        let mut dst_map = HashMap::new();
        let s1 = src_alloc.str_val("foo");
        let s2 = deepcopy(s1, &mut dst_alloc, &mut dst_map);
        assert!(s1 == s2);
    }
}
