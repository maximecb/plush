use crate::vm::{Value, Actor};
use crate::alloc::Alloc;
use crate::host::HostFn;

#[derive(Clone, Default)]
pub struct ByteArray
{
    pub bytes: Vec<u8>,
}

impl ByteArray
{
}

/// Create a new ByteArray instance
pub fn ba_new(actor: &mut Actor, _self: Value) -> Value
{
    let ba = ByteArray::default();
    let new_arr = actor.alloc.alloc(ba);
    Value::ByteArray(new_arr)
}

/*
// Resize byte array
Insn::ba_resize => {
    let fill_val = pop!().unwrap_u8();
    let new_len = pop!().unwrap_u64();
    let arr = pop!().unwrap_ba();
    ByteArray::resize(arr, new_len, fill_val, &mut self.alloc);
}

// Write u32 value
Insn::ba_write_u32 => {
    let val = pop!().unwrap_u32();
    let idx = pop!().unwrap_u64();
    let arr = pop!().unwrap_ba();
    ByteArray::write_u32(arr, idx, val);
}
*/
