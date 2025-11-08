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

pub fn array_push(actor: &mut Actor, mut array: Value, val: Value) -> Value
{
    array.unwrap_arr().push(val);
    Value::Nil
}

pub fn array_pop(actor: &mut Actor, mut array: Value) -> Value
{
    array.unwrap_arr().pop()
}

pub fn array_remove(actor: &mut Actor, mut array: Value, idx: Value) -> Value
{
    let idx = idx.unwrap_usize();
    array.unwrap_arr().elems.remove(idx)
}

pub fn array_insert(actor: &mut Actor, mut array: Value, idx: Value, val: Value) -> Value
{
    let idx = idx.unwrap_usize();
    array.unwrap_arr().elems.insert(idx, val);
    Value::Nil
}

pub fn array_append(_actor: &mut Actor, mut self_array: Value, mut other_array: Value) -> Value
{
    let other_elems = other_array.unwrap_arr().elems.clone();
    self_array.unwrap_arr().elems.extend(other_elems);
    Value::Nil
}
