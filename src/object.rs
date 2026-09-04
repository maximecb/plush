use std::mem::size_of;
use crate::ast::ClassId;
use crate::value::Value;
use crate::alloc::{header_of, Alloc, Tag, HEADER_SIZE};

/// Nothing is stored here: the slots start at the address of the object
/// itself, and the class and slot count come from the block header
#[repr(C, align(8))]
pub struct Object;

impl Object
{
    /// Bytes an object with a given slot count occupies, header included
    pub fn alloc_size(num_slots: usize) -> usize
    {
        HEADER_SIZE + num_slots * size_of::<Value>()
    }

    /// Allocate a new object with a given number of slots
    pub fn new(class_id: ClassId, num_slots: usize, alloc: &mut Alloc) -> Value
    {
        let p = alloc.alloc_slots(Tag::Object, usize::from(class_id), num_slots);

        // Zeroed memory reads back as the integer zero, so the slots have
        // to be marked uninitialized explicitly
        unsafe { std::slice::from_raw_parts_mut(p, num_slots) }.fill(Value::UNDEF);

        Value::object(p as *mut Object)
    }

    pub fn class_id(&self) -> ClassId
    {
        ClassId::from(header_of(self as *const Object as *const u8).aux24())
    }

    pub fn num_slots(&self) -> usize
    {
        header_of(self as *const Object as *const u8).num_slots()
    }

    fn slots(&self) -> &[Value]
    {
        unsafe { std::slice::from_raw_parts(
            self as *const Object as *const Value,
            self.num_slots()
        )}
    }

    fn slots_mut(&mut self) -> &mut [Value]
    {
        unsafe { std::slice::from_raw_parts_mut(
            self as *mut Object as *mut Value,
            self.num_slots()
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
