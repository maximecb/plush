use std::mem::transmute;
use crate::vm::{Value, Actor};
use crate::alloc::Alloc;
use crate::host::HostFn;

#[derive(Clone, Default)]
pub struct ByteArray
{
    bytes: Vec<u8>,
}

impl ByteArray
{
    pub fn get(&self, idx: usize) -> u8
    {
        self.bytes[idx]
    }

    pub fn set(&mut self, idx: usize, val: u8)
    {
        self.bytes[idx] = val;
    }

    pub unsafe fn get_slice<T>(&self, pos: usize, num_elems: usize) -> &'static [T]
    {
        assert!(pos + num_elems * size_of::<T>() <= self.bytes.len());
        let buf_ptr = self.bytes.as_ptr();
        let elem_ptr = transmute::<*const u8 , *mut T>(buf_ptr.add(pos));
        std::slice::from_raw_parts(elem_ptr, num_elems as usize)
    }

    pub unsafe fn get_slice_mut<T>(&mut self, pos: usize, num_elems: usize) -> &'static mut [T]
    {
        assert!(pos + num_elems * size_of::<T>() <= self.bytes.len());
        let buf_ptr = self.bytes.as_mut_ptr();
        let elem_ptr = transmute::<*mut u8 , *mut T>(buf_ptr.add(pos));
        std::slice::from_raw_parts_mut(elem_ptr, num_elems as usize)
    }

    /// Write a value at the given address
    pub fn write<T>(&mut self, pos: usize, val: T) where T: Copy
    {
        assert!(pos + size_of::<T>() <= self.bytes.len());

        unsafe {
            let buf_ptr = self.bytes.as_mut_ptr();
            let val_ptr = transmute::<*mut u8 , *mut T>(buf_ptr.add(pos));
            std::ptr::write_unaligned(val_ptr, val);
        }
    }

    /// Fill an interval with a given value
    pub fn fill<T>(&mut self, pos: usize, num: usize, val: T) where T: Copy + 'static
    {
        unsafe {
            let slice = self.get_slice_mut(pos, num);
            slice.fill(val);
        }
    }

    /// Copy bytes from another bytearray
    pub fn copy_from(&mut self, src: &ByteArray, src_pos: usize, dst_pos: usize, num_bytes: usize)
    {
        // TODO: make sure the slices don't overlap

        let src_slice = unsafe { src.get_slice::<u8>(src_pos, num_bytes) };
        let dst_slice = unsafe {  self.get_slice_mut::<u8>(dst_pos, num_bytes) };
        dst_slice.copy_from_slice(src_slice);
    }
}

/// Create a new ByteArray instance
pub fn ba_new(actor: &mut Actor, _self: Value) -> Value
{
    let ba = ByteArray::default();
    let new_arr = actor.alloc.alloc(ba);
    Value::ByteArray(new_arr)
}

/// Create a new ByteArray instance
pub fn ba_with_size(actor: &mut Actor, _self: Value, num_bytes: Value) -> Value
{
    let num_bytes = num_bytes.unwrap_usize();
    let mut bytes = Vec::with_capacity(num_bytes);
    bytes.resize(num_bytes, 0);
    let ba = ByteArray { bytes };
    Value::ByteArray(actor.alloc.alloc(ba))
}

/*
// Resize byte array
Insn::ba_resize => {
    let fill_val = pop!().unwrap_u8();
    let new_len = pop!().unwrap_u64();
    let arr = pop!().unwrap_ba();
    ByteArray::resize(arr, new_len, fill_val, &mut self.alloc);
}
*/

pub fn ba_write_u32(actor: &mut Actor, mut ba: Value, idx: Value, val: Value)
{
    let ba = ba.unwrap_ba();
    let idx = idx.unwrap_usize();
    let val = val.unwrap_u32();
    ba.write(idx, val);
}

pub fn ba_fill_u32(actor: &mut Actor, mut ba: Value, idx: Value, num: Value, val: Value)
{
    let ba = ba.unwrap_ba();
    let idx = idx.unwrap_usize();
    let num = num.unwrap_usize();
    let val = val.unwrap_u32();
    ba.fill(idx, num, val);
}

pub fn ba_copy_from(actor: &mut Actor, mut dst: Value, src: Value, src_pos: Value, dst_pos: Value, num_bytes: Value)
{
    let dst = dst.unwrap_ba();

    let src = match src {
        Value::ByteArray(p) => unsafe { &*p }
        _ => panic!()
    };

    let src_pos = src_pos.unwrap_usize();
    let dst_pos = dst_pos.unwrap_usize();
    let num_bytes = num_bytes.unwrap_usize();
    dst.copy_from(src, dst_pos, src_pos, num_bytes);
}
