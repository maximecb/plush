use crate::vm::{Value, Actor};
use crate::alloc::Alloc;
use crate::host::HostFn;

#[derive(Clone, Default)]
pub struct Array
{
    pub elems: Vec<Value>,
}

impl Array
{
    pub fn with_capacity(cap: u32) -> Self
    {
        Self {
            elems: Vec::with_capacity(cap as usize)
        }
    }

    pub fn push(&mut self, val: Value)
    {
        self.elems.push(val);
    }

    pub fn pop(&mut self) -> Value
    {
        self.elems.pop().unwrap()
    }

    pub fn get(&self, idx: usize) -> Value
    {
        self.elems[idx]
    }

    pub fn set(&mut self, idx: usize, val: Value)
    {
        self.elems[idx] = val;
    }
}

pub fn array_with_size(actor: &mut Actor, _self: Value, num_elems: Value, fill_val: Value) -> Value
{
    let num_elems = num_elems.unwrap_usize();
    let mut elems = Vec::with_capacity(num_elems);
    elems.resize(num_elems, fill_val);
    let arr = Array { elems };
    Value::Array(actor.alloc.alloc(arr))
}

pub fn array_push(actor: &mut Actor, mut array: Value, val: Value)
{
    array.unwrap_arr().push(val);
}

pub fn array_pop(actor: &mut Actor, mut array: Value) -> Value
{
    array.unwrap_arr().pop()
}
