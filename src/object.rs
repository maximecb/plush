use std::mem::size_of;
use crate::ast::ClassId;
use crate::vm::Value;
use crate::alloc::{Alloc, Tag, HEADER_SIZE};

/// The slots are stored inline, right after the object
#[repr(C, align(8))]
pub struct Object
{
    pub class_id: ClassId,
    num_slots: u32,
}

impl Object
{
    /// Bytes an object with a given slot count occupies, header included
    pub fn alloc_size(num_slots: usize) -> usize
    {
        HEADER_SIZE + size_of::<Object>() + num_slots * size_of::<Value>()
    }

    /// Allocate a new object with a given number of slots
    pub fn new(class_id: ClassId, num_slots: usize, alloc: &mut Alloc) -> Value
    {
        let obj = Object { class_id, num_slots: num_slots as u32 };
        let tail_bytes = num_slots * size_of::<Value>();

        Value::Object(alloc.alloc_var(obj, tail_bytes, Tag::Object))
    }

    pub fn num_slots(&self) -> usize
    {
        self.num_slots as usize
    }

    fn slots(&self) -> &[Value]
    {
        unsafe { std::slice::from_raw_parts(
            (self as *const Object).add(1) as *const Value,
            self.num_slots as usize
        )}
    }

    fn slots_mut(&mut self) -> &mut [Value]
    {
        unsafe { std::slice::from_raw_parts_mut(
            (self as *mut Object).add(1) as *mut Value,
            self.num_slots as usize
        )}
    }

    // Get the value associated with a given field
    pub fn get(&self, idx: usize) -> Value
    {
        self.slots()[idx]
    }

    // Set the value of a given field
    pub fn set(&mut self, idx: usize, val: Value)
    {
        self.slots_mut()[idx] = val
    }
}
