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

    pub fn get(&mut self, idx: usize) -> Value
    {
        self.elems[idx]
    }
}

//fn get_array_field(array: Value)
//{
//}
