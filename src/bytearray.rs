use std::mem::{transmute, size_of};
use crate::vm::Actor;
use crate::value::*;
use crate::alloc::{header_of, set_fixed_len, table_len, Alloc, Tag, FIXED_SIZE, HEADER_SIZE};
use crate::*;
use crate::host::HostResult;

pub struct ByteArray
{
    // Relocated by the collector, which walks the table on its own.
    // The capacity is the length of the table block this points at, so
    // it is read back from that block's header rather than stored here
    pub(crate) bytes: *mut u8,
}

// Nothing but the pointer to the byte table. The length lives in the
// block header, which is what makes this a fixed-layout block.
const _: () = assert!(size_of::<ByteArray>() == FIXED_SIZE);

impl ByteArray
{
    /// Bytes a bytearray of a given size occupies, counting the headers
    /// of both the bytearray and its byte table
    pub fn alloc_size(num_bytes: usize) -> usize
    {
        HEADER_SIZE + size_of::<ByteArray>() +
        HEADER_SIZE + crate::alloc::align_up(num_bytes)
    }

    /// Allocate a zeroed bytearray of a given size.
    ///
    /// The bytearray is allocated before its table, so that the two end
    /// up in the same order the collector puts them in, with the bytes
    /// following the bytearray they belong to. No collection can happen
    /// between the two allocations: callers reserve the space up front.
    pub fn with_size(num_bytes: usize, alloc: &mut Alloc) -> Value
    {
        // alloc_fixed leaves the byte pointer null, so the bytearray is
        // walkable between here and the table allocation below
        let ba = alloc.alloc_fixed(Tag::ByteArray, num_bytes) as *mut ByteArray;
        let bytes: *mut u8 = alloc.alloc_table(num_bytes, Tag::Bytes);
        unsafe { (*ba).bytes = bytes };

        // A new bytearray reads as zeroed. Stale bytes here would be
        // silently wrong data rather than anything that fails.
        #[cfg(feature = "verify_gc")]
        assert!(
            crate::alloc::table_slice(bytes).iter().all(|b| *b == 0),
            "bytearray allocated over memory that was not zeroed"
        );

        Value::bytearray(ba)
    }

    pub fn clone(&self, alloc: &mut Alloc) -> Value
    {
        let len = self.num_bytes();
        let new_ba = Self::with_size(len, alloc);

        unsafe {
            let src_slice: &[u8] = self.get_slice(0, len);
            let dst_slice: &mut [u8] = new_ba.as_ba().get_slice_mut(0, len);
            dst_slice.copy_from_slice(src_slice);
        }

        new_ba
    }

    fn block(&self) -> *mut u8
    {
        self as *const ByteArray as *mut u8
    }

    /// Bytes held, which lives in the block header
    pub fn num_bytes(&self) -> usize
    {
        header_of(self.block()).fixed_len()
    }

    fn set_num_bytes(&mut self, num_bytes: usize)
    {
        set_fixed_len(self.block(), num_bytes);
    }

    /// Bytes the table can hold
    pub fn capacity(&self) -> usize
    {
        table_len(self.bytes)
    }

    pub unsafe fn get_slice<T>(&self, idx: usize, num_elems: usize) -> &'static [T]
    {
        assert!((idx + num_elems) * size_of::<T>() <= self.num_bytes());
        let elem_ptr = transmute::<*const u8 , *const T>(self.bytes as *const u8).add(idx);
        std::slice::from_raw_parts(elem_ptr, num_elems as usize)
    }

    pub unsafe fn get_slice_mut<T>(&mut self, idx: usize, num_elems: usize) -> &'static mut [T]
    {
        assert!((idx + num_elems) * size_of::<T>() <= self.num_bytes());
        let elem_ptr = transmute::<*mut u8 , *mut T>(self.bytes).add(idx);
        std::slice::from_raw_parts_mut(elem_ptr, num_elems as usize)
    }

    /// Load a value at the given byte index
    pub fn load<T>(&mut self, byte_idx: usize) -> T where T: Copy
    {
        assert!(byte_idx + size_of::<T>() <= self.num_bytes());

        unsafe {
            let val_ptr = transmute::<*const u8 , *const T>(self.bytes.add(byte_idx) as *const u8);
            std::ptr::read_unaligned(val_ptr)
        }
    }

    /// Store a value at the given byte index
    pub fn store<T>(&mut self, byte_idx: usize, val: T) where T: Copy
    {
        assert!(byte_idx + size_of::<T>() <= self.num_bytes());

        unsafe {
            let val_ptr = transmute::<*mut u8 , *mut T>(self.bytes.add(byte_idx));
            std::ptr::write_unaligned(val_ptr, val);
        }
    }

    /// Read a value at the given index (aligned read)
    pub fn get<T>(&mut self, idx: usize) -> T where T: Copy
    {
        assert!((idx + 1) * size_of::<T>() <= self.num_bytes());

        unsafe {
            let val_ptr = transmute::<*const u8 , *const T>(self.bytes as *const u8).add(idx);
            std::ptr::read(val_ptr)
        }
    }

    /// Write a value at the given index (aligned write)
    pub fn set<T>(&mut self, idx: usize, val: T) where T: Copy
    {
        assert!((idx + 1) * size_of::<T>() <= self.num_bytes());

        unsafe {
            let val_ptr = transmute::<*mut u8 , *mut T>(self.bytes).add(idx);
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
pub fn ba_with_size(actor: &mut Actor, _self: Value, num_bytes: Value) -> HostResult
{
    let num_bytes = unwrap_usize!(num_bytes);

    actor.gc_check(
        ByteArray::alloc_size(num_bytes),
        &mut []
    );

    Ok(ByteArray::with_size(num_bytes, &mut actor.alloc))
}

pub fn ba_resize(actor: &mut Actor, mut ba: Value, new_size: Value) -> HostResult
{
    let new_size = unwrap_usize!(new_size);

    // Get the current capacity without a mutable borrow
    let capacity = unwrap_ba!(ba).capacity();

    if new_size > capacity {
        actor.gc_check(
            HEADER_SIZE + new_size,
            &mut [&mut ba]
        );
        let ba_mut = ba.as_ba();

        let old_len = ba_mut.num_bytes();
        let new_bytes: *mut u8 = actor.alloc.alloc_table(new_size, Tag::Bytes);
        let copy_len = std::cmp::min(old_len, new_size);

        unsafe {
            std::ptr::copy_nonoverlapping(ba_mut.bytes, new_bytes, copy_len);
        }

        ba_mut.bytes = new_bytes;
        ba_mut.set_num_bytes(new_size);
    }
    else {
        let ba_mut = ba.as_ba();
        ba_mut.set_num_bytes(new_size);
    }

    Ok(Value::NIL)
}

pub fn ba_load_u32(_actor: &mut Actor, ba: Value, byte_idx: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val: u32 = ba.load(byte_idx);
    Ok(Value::from(val))
}

pub fn ba_store_u32(_actor: &mut Actor, ba: Value, byte_idx: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val = unwrap_u32!(val);
    ba.store(byte_idx, val);
    Ok(Value::NIL)
}

pub fn ba_load_u16(_actor: &mut Actor, ba: Value, byte_idx: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val: u16 = ba.load(byte_idx);
    Ok(Value::from(val as u32))
}

pub fn ba_store_u16(_actor: &mut Actor, ba: Value, byte_idx: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val = unwrap_i64!(val);
    ba.store(byte_idx, val as u16);
    Ok(Value::NIL)
}

pub fn ba_load_f32(actor: &mut Actor, ba: Value, byte_idx: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val: f32 = ba.load(byte_idx);
    Ok(actor.float64(val as f64))
}

pub fn ba_store_f32(_actor: &mut Actor, ba: Value, byte_idx: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let byte_idx = unwrap_usize!(byte_idx);
    let val = unwrap_f64!(val);
    ba.store(byte_idx, val as f32);
    Ok(Value::NIL)
}

pub fn ba_get_u32(_actor: &mut Actor, ba: Value, idx: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let idx = unwrap_usize!(idx);
    let val: u32 = ba.get(idx);
    Ok(Value::from(val))
}

pub fn ba_set_u32(_actor: &mut Actor, ba: Value, idx: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let idx = unwrap_usize!(idx);
    let val = unwrap_u32!(val);
    ba.set(idx, val);
    Ok(Value::NIL)
}

pub fn ba_get_f32(actor: &mut Actor, ba: Value, idx: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let idx = unwrap_usize!(idx);
    let val: f32 = ba.get(idx);
    Ok(actor.float64(val as f64))
}

pub fn ba_set_f32(_actor: &mut Actor, ba: Value, idx: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let idx = unwrap_usize!(idx);
    let val = unwrap_f64!(val);
    ba.set(idx, val as f32);
    Ok(Value::NIL)
}

/// How many independent accumulators the dot product kernel keeps.
///
/// One accumulator would make every add wait for the previous one. Splitting
/// the sum into chains that only meet at the end removes that dependency, and
/// shortens the chain each rounding error travels down. Eight measured 11%
/// ahead of four on an 8192-element product, and sixteen fell back behind.
///
/// LLVM compiles this to eight scalar chains, not vector adds: it won't pack
/// a widening f32-to-f64 accumulate on its own. That leaves four FP ops per
/// element against four pipes, and the loop measures at the one element per
/// cycle this predicts. It's the arithmetic that runs out first, not the
/// loads -- the rate holds from a 2 KB working set to a 512 KB one.
const DOT_UNROLL: usize = 8;

// The accumulators are folded together in pairs, which needs a count that
// halves evenly
const _: () = assert!(DOT_UNROLL.is_power_of_two());

/// Sum of the products of `num` pairs of f32 values, taken `stride` elements
/// apart in each operand. Both slices start at the first element to be read
/// and end at the last, so the indices below are in bounds.
///
/// The products are formed in f64. Two 24-bit significands multiply to 48
/// bits, which an f64 holds exactly, so only the sums round, at f64
/// precision. The accuracy is free: the loads have to widen either way.
///
/// `inline(always)` so that the unit-stride caller's literal strides fold
/// the index arithmetic away into contiguous loads.
#[inline(always)]
fn dot_f32_kernel(a: &[f32], a_stride: usize, b: &[f32], b_stride: usize, num: usize) -> f64
{
    let mut acc = [0.0f64; DOT_UNROLL];

    let mut i = 0;
    while i + DOT_UNROLL <= num {
        for k in 0..DOT_UNROLL {
            acc[k] += a[(i + k) * a_stride] as f64 * b[(i + k) * b_stride] as f64;
        }
        i += DOT_UNROLL;
    }

    // Folded in pairs, halving the accumulators each round, which keeps the
    // tree of additions shallow. Both bounds are constants, so it is free
    let mut width = DOT_UNROLL;
    while width > 1 {
        width /= 2;
        for k in 0..width {
            acc[k] += acc[k + width];
        }
    }
    let mut sum = acc[0];

    // The elements left over when the count is not a multiple of the unroll
    while i < num {
        sum += a[i * a_stride] as f64 * b[i * b_stride] as f64;
        i += 1;
    }

    sum
}

/// Dot product of two runs of f32 values, each described by a start index,
/// a stride, and a shared element count. The two runs may live in the same
/// bytearray, and are only read.
pub fn ba_dot_f32(
    actor: &mut Actor,
    a: Value,
    a_idx: Value,
    a_stride: Value,
    b: Value,
    b_idx: Value,
    b_stride: Value,
    num: Value,
) -> HostResult
{
    let a_ba = unwrap_ba!(a);
    let b_ba = unwrap_ba!(b);
    let a_idx = unwrap_usize!(a_idx);
    let a_stride = unwrap_usize!(a_stride);
    let b_idx = unwrap_usize!(b_idx);
    let b_stride = unwrap_usize!(b_stride);
    let num = unwrap_usize!(num);

    if a_stride < 1 || b_stride < 1 {
        error!("expected strides to be at least 1");
    }

    if num == 0 {
        return Ok(actor.float64(0.0));
    }

    // The span an operand covers, in f32 elements. Checked, because a large
    // count times a large stride would otherwise wrap past the bounds test
    fn span(idx: usize, stride: usize, num: usize) -> Option<usize>
    {
        (num - 1).checked_mul(stride)?.checked_add(idx)?.checked_add(1)
    }

    let a_span = match span(a_idx, a_stride, num) {
        Some(v) => v,
        None => error!("dot_f32 index range overflows"),
    };
    let b_span = match span(b_idx, b_stride, num) {
        Some(v) => v,
        None => error!("dot_f32 index range overflows"),
    };

    // Bounds are settled once here, for the whole run
    let a_len = a_ba.num_bytes() / size_of::<f32>();
    let b_len = b_ba.num_bytes() / size_of::<f32>();

    if a_span > a_len || b_span > b_len {
        error!(
            "dot_f32 reads f32 elements up to {} and {}, past the ends at {} and {}",
            a_span - 1, b_span - 1, a_len, b_len
        );
    }

    // Trimmed to exactly the elements that will be read, so that the
    // unit-stride case gets a slice as long as the count and the kernel's
    // bounds checks drop out
    let a_slice = unsafe { a_ba.get_slice::<f32>(a_idx, a_span - a_idx) };
    let b_slice = unsafe { b_ba.get_slice::<f32>(b_idx, b_span - b_idx) };

    // Contiguous operands get their own copy of the kernel, with the strides
    // as constants. This is the case worth being fast.
    let sum = if a_stride == 1 && b_stride == 1 {
        dot_f32_kernel(a_slice, 1, b_slice, 1, num)
    } else {
        dot_f32_kernel(a_slice, a_stride, b_slice, b_stride, num)
    };

    Ok(actor.float64(sum))
}

pub fn ba_num_u32(_actor: &mut Actor, ba: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let len = ba.num_bytes();

    if len % 4 != 0 {
        error!("expected ByteArray size to be divisible by 4");
    }

    Ok(Value::fixnum((len / 4) as i64))
}

pub fn ba_memcpy(_actor: &mut Actor, dst: Value, dst_idx: Value, src: Value, src_idx: Value, num_bytes: Value) -> HostResult
{
    let dst = unwrap_ba!(dst);

    let src = unwrap_ba!(src);

    let src_idx = unwrap_usize!(src_idx);
    let dst_idx = unwrap_usize!(dst_idx);
    let num_bytes = unwrap_usize!(num_bytes);
    dst.memcpy(dst_idx, src, src_idx, num_bytes);
    Ok(Value::NIL)
}

pub fn ba_zero_fill(_actor: &mut Actor, ba: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let slice = unsafe { ba.get_slice_mut(0, ba.num_bytes()) };
    slice.fill(0u8);
    Ok(Value::NIL)
}

pub fn ba_fill_u32(_actor: &mut Actor, ba: Value, idx: Value, num: Value, val: Value) -> HostResult
{
    let ba = unwrap_ba!(ba);
    let idx = unwrap_usize!(idx);
    let num = unwrap_usize!(num);
    let val = unwrap_u32!(val);
    ba.fill(idx, num, val);
    Ok(Value::NIL)
}


#[cfg(test)]
mod tests
{
    use super::*;

    /// Values that are not exactly representable, so that a kernel which
    /// summed them in the wrong precision would show it
    fn sample(n: usize) -> (Vec<f32>, Vec<f32>)
    {
        let mut a = Vec::with_capacity(n);
        let mut b = Vec::with_capacity(n);
        for i in 0..n {
            a.push((i as f32) * 0.1 - 3.7);
            b.push(1.0 / ((i % 13) as f32 + 1.3));
        }
        (a, b)
    }

    /// The portable kernel against a plain running sum, at the same
    /// precision. Only the order of the additions differs, so a length
    /// this short leaves them equal.
    #[test]
    fn kernel_matches_simple_sum()
    {
        for n in [0usize, 1, 3, 7, 8, 9, 16, 31] {
            let (a, b) = sample(n);

            let mut expected = 0.0f64;
            for i in 0..n {
                expected += a[i] as f64 * b[i] as f64;
            }

            let got = dot_f32_kernel(&a, 1, &b, 1, n);
            let err = (got - expected).abs();
            assert!(err <= 1e-12 * expected.abs().max(1.0), "n={}, {} vs {}", n, got, expected);
        }
    }

    /// Reading with a stride finds the same values as a contiguous read of
    /// the elements it lands on
    #[test]
    fn strided_matches_contiguous()
    {
        for n in [1usize, 7, 8, 9, 100, 1013] {
            let (a, b) = sample(n * 3);

            let picked_a: Vec<f32> = (0..n).map(|i| a[i * 3]).collect();
            let picked_b: Vec<f32> = (0..n).map(|i| b[i * 2]).collect();

            let expected = dot_f32_kernel(&picked_a, 1, &picked_b, 1, n);
            let got = dot_f32_kernel(&a, 3, &b, 2, n);

            assert_eq!(got, expected, "n={}", n);
        }
    }

    /// Accumulating in f64 keeps a sum that f32 could not hold. The
    /// products here are exact, and so is their f64 sum, so this asks for
    /// the exact answer.
    #[test]
    fn accumulates_in_f64()
    {
        // 2^24 + 1 is the first integer an f32 cannot represent
        let a = [16777216.0f32, 1.0];
        let b = [1.0f32, 1.0];

        assert_eq!(dot_f32_kernel(&a, 1, &b, 1, 2), 16777217.0);
    }
}
