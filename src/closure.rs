use crate::ast::FunId;
use crate::vm::Value;
use crate::alloc::Alloc;

pub struct Closure
{
    pub fun_id: FunId,
    slots: *mut [Value],
}

impl Closure
{
    /// Allocate a new closure with a given number of slots
    pub fn new(fun_id: FunId, num_slots: usize, alloc: &mut Alloc) -> Result<Value, ()>
    {
        // Allocate the slots for the closure
        let slots = alloc.alloc_table::<Value>(num_slots)?;

        // Create the closure struct
        let obj = Closure { fun_id, slots };

        // Allocate the Object struct itself
        let obj_ptr = alloc.alloc(obj)?;

        Ok(Value::Closure(obj_ptr))
    }

    pub fn num_slots(&self) -> usize
    {
        unsafe { (&*self.slots).len() }
    }

    // Get the given closure slot value
    pub fn get(&self, idx: usize) -> Value
    {
        unsafe { (*self.slots)[idx] }
    }

    // Set the value of a given closure slot
    pub fn set(&mut self, idx: usize, val: Value)
    {
        unsafe { (*self.slots)[idx] = val }
    }
}
