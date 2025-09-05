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

/// Copy image data from a source image into a destination image
/// while performing A-over-B alpha compositing
/// Pixels use the BGRA byte order (0xAA_RR_GG_BB on a little-endian machine)
fn blit_bgra32(
    dst: &mut [u32],
    dst_width: usize,
    dst_height: usize,
    src: &[u32],
    src_width: usize,
    src_height: usize,
    dst_x: i32,
    dst_y: i32,
)
{
    for sy in 0..src_height as i32 {
        let dy = dst_y + sy;
        if dy < 0 || dy >= dst_height as i32 {
            continue;
        }

        for sx in 0..src_width as i32 {
            let dx = dst_x + sx;
            if dx < 0 || dx >= dst_width as i32 {
                continue;
            }

            let src_idx = (sy as usize * src_width + sx as usize) as usize;
            let dst_idx = (dy as usize * dst_width + dx as usize) as usize;

            // Extract source pixel components
            let src_pixel = src[src_idx];
            let src_b = src_pixel & 0xFF;
            let src_g = (src_pixel >> 8) & 0xFF;
            let src_r = (src_pixel >> 16) & 0xFF;
            let src_a = (src_pixel >> 24) & 0xFF;

            // Extract destination pixel components
            let dst_pixel = dst[dst_idx];
            let dst_b = dst_pixel & 0xFF;
            let dst_g = (dst_pixel >> 8) & 0xFF;
            let dst_r = (dst_pixel >> 16) & 0xFF;
            let dst_a = (dst_pixel >> 24) & 0xFF;

            // Perform alpha blending using integer arithmetic
            // out_a = src_a + dst_a * (255 - src_a) / 255
            let one_minus_src_a = 255 - src_a;
            let out_a = src_a + (dst_a * one_minus_src_a + 127) / 255;

            // Avoid division by zero
            if out_a == 0 {
                dst[dst_idx] = 0; // Fully transparent result
                continue;
            }

            // out_color = (src_color * src_a + dst_color * dst_a * (255 - src_a) / 255) / out_a
            let out_r = (src_r * src_a + dst_r * dst_a * one_minus_src_a / 255 + out_a / 2) / out_a;
            let out_g = (src_g * src_a + dst_g * dst_a * one_minus_src_a / 255 + out_a / 2) / out_a;
            let out_b = (src_b * src_a + dst_b * dst_a * one_minus_src_a / 255 + out_a / 2) / out_a;

            // Clamp values to [0, 255]
            let out_r = out_r.min(255) as u32;
            let out_g = out_g.min(255) as u32;
            let out_b = out_b.min(255) as u32;
            let out_a = out_a.min(255) as u32;

            // Pack the result back into a u32
            dst[dst_idx] = (out_r << 16) | (out_g << 8) | out_b | (out_a << 24);
        }
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

pub fn ba_blit_bgra32(
    actor: &mut Actor,
    mut dst: Value,
    dst_width: Value,
    dst_height: Value,
    mut src: Value,
    src_width: Value,
    src_height: Value,
    dst_x: Value,
    dst_y: Value,
)
{
    let dst = dst.unwrap_ba();
    let dst_width = dst_width.unwrap_usize();
    let dst_height = dst_height.unwrap_usize();

    let src = src.unwrap_ba();
    let src_width = src_width.unwrap_usize();
    let src_height = src_height.unwrap_usize();

    let dst_x = dst_x.unwrap_i32();
    let dst_y = dst_y.unwrap_i32();

    blit_bgra32(
        unsafe { dst.get_slice_mut(0, dst_width * dst_height) },
        dst_width,
        dst_height,
        unsafe { src.get_slice(0, src_width * src_height) },
        src_width,
        src_height,
        dst_x,
        dst_y,
    );
}
