use std::mem::size_of;
use crate::ast::FunId;
use crate::vm::Value;
use crate::alloc::{Alloc, Tag, HEADER_SIZE};

/// The slots are stored inline, right after the closure
#[repr(C, align(8))]
pub struct Closure
{
    pub fun_id: FunId,
    num_slots: u32,
}

impl Closure
{
    /// Bytes a closure with a given slot count occupies, header included
    pub fn alloc_size(num_slots: usize) -> usize
    {
        HEADER_SIZE + size_of::<Closure>() + num_slots * size_of::<Value>()
    }

    /// Allocate a new closure with a given number of slots
    pub fn new(fun_id: FunId, num_slots: usize, alloc: &mut Alloc) -> Value
    {
        let clos = Closure { fun_id, num_slots: num_slots as u32 };
        let tail_bytes = num_slots * size_of::<Value>();

        Value::Closure(alloc.alloc_var(clos, tail_bytes, Tag::Closure))
    }

    pub fn num_slots(&self) -> usize
    {
        self.num_slots as usize
    }

    fn slots(&self) -> &[Value]
    {
        unsafe { std::slice::from_raw_parts(
            (self as *const Closure).add(1) as *const Value,
            self.num_slots as usize
        )}
    }

    fn slots_mut(&mut self) -> &mut [Value]
    {
        unsafe { std::slice::from_raw_parts_mut(
            (self as *mut Closure).add(1) as *mut Value,
            self.num_slots as usize
        )}
    }

    // Get the given closure slot value
    pub fn get(&self, idx: usize) -> Value
    {
        self.slots()[idx]
    }

    // Set the value of a given closure slot
    pub fn set(&mut self, idx: usize, val: Value)
    {
        self.slots_mut()[idx] = val
    }
}
