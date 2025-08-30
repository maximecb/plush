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
    pub fn new(bytes: &Vec<u8>) -> Self
    {
        Self {
            bytes: bytes.clone()
        }
    }

    pub fn num_bytes(&self) -> usize
    {
        self.bytes.len()
    }

    pub fn get(&self, idx: usize) -> u8
    {
        self.bytes[idx]
    }

    pub fn set(&mut self, idx: usize, val: u8)
    {
        self.bytes[idx] = val;
    }

    pub unsafe fn get_slice<T>(&self, idx: usize, num_elems: usize) -> &'static [T]
    {
        assert!((idx + num_elems) * size_of::<T>() <= self.bytes.len());
        let buf_ptr = self.bytes.as_ptr();
        let elem_ptr = transmute::<*const u8 , *mut T>(buf_ptr).add(idx);
        std::slice::from_raw_parts(elem_ptr, num_elems as usize)
    }

    pub unsafe fn get_slice_mut<T>(&mut self, idx: usize, num_elems: usize) -> &'static mut [T]
    {
        assert!((idx + num_elems) * size_of::<T>() <= self.bytes.len());
        let buf_ptr = self.bytes.as_mut_ptr();
        let elem_ptr = transmute::<*mut u8 , *mut T>(buf_ptr).add(idx);
        std::slice::from_raw_parts_mut(elem_ptr, num_elems as usize)
    }

    /// Read a value at the given index
    pub fn read<T>(&mut self, idx: usize) -> T where T: Copy
    {
        assert!((idx + 1) * size_of::<T>() <= self.bytes.len());

        unsafe {
            let buf_ptr = self.bytes.as_ptr();
            let val_ptr = transmute::<*const u8 , *const T>(buf_ptr).add(idx);
            std::ptr::read(val_ptr)
        }
    }

    /// Write a value at the given index
    pub fn write<T>(&mut self, idx: usize, val: T) where T: Copy
    {
        assert!((idx + 1) * size_of::<T>() <= self.bytes.len());

        unsafe {
            let buf_ptr = self.bytes.as_mut_ptr();
            let val_ptr = transmute::<*mut u8 , *mut T>(buf_ptr).add(idx);
            std::ptr::write(val_ptr, val);
        }
    }

    /// Fill an interval with a given value
    pub fn fill<T>(&mut self, idx: usize, num: usize, val: T) where T: Copy + 'static
    {
        unsafe {
            let slice = self.get_slice_mut(idx, num);
            slice.fill(val);
        }
    }

    /// Copy bytes from another bytearray
    pub fn memcpy(&mut self, dst_idx: usize, src: &ByteArray, src_idx: usize, num_bytes: usize)
    {
        // TODO: make sure the slices don't overlap

        let src_slice = unsafe { src.get_slice::<u8>(src_idx, num_bytes) };
        let dst_slice = unsafe { self.get_slice_mut::<u8>(dst_idx, num_bytes) };
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

pub fn ba_read_u32(actor: &mut Actor, mut ba: Value, idx: Value) -> Value
{
    let ba = ba.unwrap_ba();
    let idx = idx.unwrap_usize();
    let val: u32 = ba.read(idx);
    Value::from(val)
}

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

pub fn ba_memcpy(actor: &mut Actor, mut dst: Value, dst_idx: Value, src: Value, src_idx: Value, num_bytes: Value)
{
    let dst = dst.unwrap_ba();

    let src = match src {
        Value::ByteArray(p) => unsafe { &*p }
        _ => panic!()
    };

    let src_idx = src_idx.unwrap_usize();
    let dst_idx = dst_idx.unwrap_usize();
    let num_bytes = num_bytes.unwrap_usize();
    dst.memcpy(dst_idx, src, src_idx, num_bytes);
}

pub fn ba_zero_fill(actor: &mut Actor, mut ba: Value)
{
    let ba = ba.unwrap_ba();
    ba.bytes.fill(0);
}
