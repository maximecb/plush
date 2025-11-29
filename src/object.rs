use crate::ast::ClassId;
use crate::vm::Value;
use crate::alloc::Alloc;

pub struct Object
{
    pub class_id: ClassId,
    slots: *mut [Value],
}

impl Object
{
    /*
    pub fn new(class_id: ClassId, slots: *mut [Value]) -> Self
    {
        Object {
            class_id,
            slots,
        }
    }
    */

    /// Allocate a new object with a given number of slots
    pub fn new(class_id: ClassId, num_slots: usize, alloc: &mut Alloc) -> Result<Value, ()>
    {
        // Allocate the slots for the object
        let slots = alloc.alloc_table::<Value>(num_slots)?;

        // Create the Object struct
        let obj = Object { class_id, slots };

        // Allocate the Object struct itself
        let obj_ptr = alloc.alloc(obj)?;

        Ok(Value::Object(obj_ptr))
    }

    pub fn num_slots(&self) -> usize
    {
        unsafe { (&*self.slots).len() }
    }

    // Get the value associated with a given field
    pub fn get(&self, idx: usize) -> Value
    {
        unsafe { (*self.slots)[idx] }
    }

    // Set the value of a given field
    pub fn set(&mut self, idx: usize, val: Value)
    {
        unsafe { (*self.slots)[idx] = val }
    }
}
