use crate::vm::Actor;
use crate::value::*;
use crate::alloc::{
    header_of, set_fixed_len, table_len, table_slice, table_slice_mut,
    Alloc, Tag, FIXED_SIZE, HEADER_SIZE,
};
use crate::*;
use crate::host::HostResult;

pub struct Array
{
    // Relocated by the collector, which walks the table on its own.
    // The capacity is the length of the table block this points at, so
    // it is read back from that block's header rather than stored here
    pub(crate) elems: *mut Value,
}

// Nothing but the pointer to the element table. The length lives in the
// block header, which is what makes this a fixed-layout block.
const _: () = assert!(size_of::<Array>() == FIXED_SIZE);

/// Capacity to grow to when a table of a given capacity is full.
/// Growing doubles, but an empty array has to end up with room for one.
fn grown_capacity(capacity: usize) -> usize
{
    std::cmp::max(capacity * 2, 1)
}

impl Array
{
    /// Bytes an array with a given capacity occupies, counting the
    /// headers of both the array and its element table
    pub fn alloc_size(capacity: usize) -> usize
    {
        HEADER_SIZE + size_of::<Array>() +
        HEADER_SIZE + capacity * size_of::<Value>()
    }

    /// Allocate an empty array with room for a given number of elements.
    ///
    /// The array is allocated before its table, so that the two end up in
    /// the same order the collector puts them in, with the elements
    /// following the array they belong to. No collection can happen
    /// between the two allocations: callers reserve the space up front.
    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Value
    {
        // alloc_fixed leaves the element pointer null, so the array is
        // walkable between here and the table allocation below
        let arr = alloc.alloc_fixed(Tag::Array, 0) as *mut Array;
        unsafe { (*arr).elems = alloc.alloc_table(capacity, Tag::ValueTable) };

        Value::array(arr)
    }

    /// Allocate an array of a given size, with every element filled in
    pub fn with_size(size: usize, fill_val: Value, alloc: &mut Alloc) -> Value
    {
        let arr = Self::with_capacity(size, alloc);
        arr.as_arr().resize(size, fill_val, alloc);
        arr
    }

    /// Capacity to grow to when the element table is full. Growing
    /// doubles, but an empty array has to end up with room for one.
    fn grown_capacity(&self) -> usize
    {
        grown_capacity(self.capacity())
    }

    fn block(&self) -> *mut u8
    {
        self as *const Array as *mut u8
    }

    pub fn len(&self) -> usize
    {
        header_of(self.block()).fixed_len()
    }

    fn set_len(&mut self, len: usize)
    {
        set_fixed_len(self.block(), len);
    }

    pub fn capacity(&self) -> usize
    {
        table_len(self.elems)
    }

    /// The whole element table, spare capacity included
    fn elems(&self) -> &[Value]
    {
        table_slice(self.elems)
    }

    fn elems_mut(&mut self) -> &mut [Value]
    {
        table_slice_mut(self.elems)
    }

    /// Grow the element table to a given capacity, keeping what is in it
    fn grow_to(&mut self, new_capacity: usize, alloc: &mut Alloc)
    {
        let new_elems: *mut Value = alloc.alloc_table(new_capacity, Tag::ValueTable);
        let cur_len = self.len();

        unsafe {
            std::ptr::copy_nonoverlapping(self.elems, new_elems, cur_len);
        }

        self.elems = new_elems;
    }

    /// An index inside the length is inside the table as well, so it can
    /// be read without going to the table header for the capacity. Only
    /// an index in the spare capacity past the length has to check
    /// against it, and that goes out of line so that the common case
    /// stays small enough to inline into the interpreter loop.
    pub fn get(&self, idx: usize) -> Value
    {
        debug_assert!(self.len() <= self.capacity());

        if idx < self.len() {
            return unsafe { *self.elems.add(idx) };
        }

        self.get_past_len(idx)
    }

    #[cold]
    #[inline(never)]
    fn get_past_len(&self, idx: usize) -> Value
    {
        self.elems()[idx]
    }

    pub fn set(&mut self, idx: usize, val: Value)
    {
        debug_assert!(self.len() <= self.capacity());

        if idx < self.len() {
            unsafe { *self.elems.add(idx) = val };
            return;
        }

        self.set_past_len(idx, val);
    }

    #[cold]
    #[inline(never)]
    fn set_past_len(&mut self, idx: usize, val: Value)
    {
        self.elems_mut()[idx] = val;
    }

    pub fn items(&self) -> &[Value] {
        &self.elems()[..self.len()]
    }

    pub fn push(&mut self, val: Value, alloc: &mut Alloc)
    {
        let len = self.len();
        let capacity = self.capacity();
        assert!(len <= capacity);

        // If we are at capacity
        if len == capacity {
            self.grow_to(grown_capacity(capacity), alloc);
        }

        // There is room at the end now, so this needs no bounds check
        unsafe { *self.elems.add(len) = val };
        self.set_len(len + 1);
    }

    pub fn insert(&mut self, idx: usize, val: Value, alloc: &mut Alloc)
    {
        let len = self.len();
        let capacity = self.capacity();

        // If we are at capacity
        if len == capacity {
            self.grow_to(grown_capacity(capacity), alloc);
        }

        let elems = self.elems_mut();
        elems.copy_within(idx..len, idx + 1);
        elems[idx] = val;
        self.set_len(len + 1);
    }

    pub fn remove(&mut self, idx: usize) -> Value
    {
        let len = self.len();

        if idx >= len {
            return Value::NIL;
        }

        let elems = self.elems_mut();
        let removed = elems[idx];
        elems.copy_within(idx + 1..len, idx);

        self.set_len(len - 1);
        self.clear_slot(len - 1);
        removed
    }

    /// Copy the `[start, end)` range of this array into a new array
    pub fn slice(&self, start: usize, end: usize, alloc: &mut Alloc) -> Value
    {
        let new_arr = Self::with_capacity(end - start, alloc);
        let src = &self.items()[start..end];
        new_arr.as_arr().elems_mut()[..src.len()].copy_from_slice(src);
        new_arr.as_arr().set_len(end - start);
        new_arr
    }

    pub fn append(&mut self, other: &Array, alloc: &mut Alloc) {
        let other_elems = other.items();
        let cur_len = self.len();
        let new_len = cur_len + other_elems.len();

        if new_len > self.capacity() {
            self.grow_to(new_len, alloc);
        }

        self.elems_mut()[cur_len..new_len].copy_from_slice(other_elems);
        self.set_len(new_len);
    }

    pub fn resize(&mut self, new_len: usize, fill_val: Value, alloc: &mut Alloc)
    {
        // If the new length doesn't fit in the current table
        if new_len > self.capacity() {
            self.grow_to(new_len, alloc);
        }

        let len = self.len();
        let elems = self.elems_mut();

        if new_len > len {
            elems[len..new_len].fill(fill_val);
        } else {
            // Clear the slots past the new end, which the collector scans
            elems[new_len..len].fill(Value::UNDEF);
        }

        self.set_len(new_len);
    }

    pub fn pop(&mut self) -> Value
    {
        let len = self.len();

        if len == 0 {
            return Value::NIL;
        }

        self.set_len(len - 1);
        let popped = unsafe { *self.elems.add(len - 1) };
        self.clear_slot(len - 1);
        popped
    }

    /// Clear a slot past the end of the array. The collector scans the
    /// whole table, so a stale value left here would be kept alive.
    fn clear_slot(&mut self, idx: usize)
    {
        debug_assert!(idx < self.capacity());
        unsafe { *self.elems.add(idx) = Value::UNDEF };
    }
}

pub fn array_with_size(actor: &mut Actor, _self: Value, num_elems: Value, mut fill_val: Value) -> HostResult
{
    let num_elems = unwrap_usize!(num_elems);

    actor.gc_check(
        Array::alloc_size(num_elems),
        &mut [&mut fill_val]
    );

    Ok(Array::with_size(num_elems, fill_val, &mut actor.alloc))
}

pub fn array_push(actor: &mut Actor, mut array: Value, mut val: Value) -> HostResult
{
    let arr = unwrap_arr!(array);

    if arr.len() == arr.capacity() {
        actor.gc_check(
            HEADER_SIZE + size_of::<Value>() * arr.grown_capacity(),
            &mut [&mut array, &mut val]
        )
    }

    let arr = array.as_arr();
    arr.push(val, &mut actor.alloc);
    Ok(Value::NIL)
}

pub fn array_pop(_actor: &mut Actor, array: Value) -> HostResult
{
    Ok(unwrap_arr!(array).pop())
}

pub fn array_remove(_actor: &mut Actor, array: Value, idx: Value) -> HostResult
{
    let idx = unwrap_usize!(idx);
    Ok(unwrap_arr!(array).remove(idx))
}

pub fn array_insert(actor: &mut Actor, mut array: Value, mut idx: Value, mut val: Value) -> HostResult
{
    let arr = unwrap_arr!(array);

    if arr.len() == arr.capacity() {
        actor.gc_check(
            HEADER_SIZE + size_of::<Value>() * arr.grown_capacity(),
            &mut [&mut array, &mut idx, &mut val]
        )
    }

    let arr = array.as_arr();
    let idx = unwrap_usize!(idx);
    arr.insert(idx, val, &mut actor.alloc);
    Ok(Value::NIL)
}

pub fn array_resize(actor: &mut Actor, mut array: Value, mut new_size: Value, mut fill_val: Value) -> HostResult
{
    let new_len = unwrap_usize!(new_size);
    let capacity = unwrap_arr!(array).capacity();

    if new_len > capacity {
        actor.gc_check(
            HEADER_SIZE + size_of::<Value>() * new_len,
            &mut [&mut array, &mut new_size, &mut fill_val]
        )
    }

    let arr = array.as_arr();
    arr.resize(new_len, fill_val, &mut actor.alloc);
    Ok(Value::NIL)
}

pub fn array_append(actor: &mut Actor, mut self_array: Value, mut other_array: Value) -> HostResult
{
    let a0 = unwrap_arr!(self_array);
    let a1 = unwrap_arr!(other_array);
    let new_len = a0.len() + a1.len();

    if a0.len() + a1.len() > a0.capacity() {
        actor.gc_check(
            HEADER_SIZE + size_of::<Value>() * new_len,
            &mut [&mut self_array, &mut other_array]
        )
    }

    let a0 = self_array.as_arr();
    let a1 = other_array.as_arr();
    a0.append(a1, &mut actor.alloc);
    Ok(Value::NIL)
}

/// Copy the `[start, end)` range of an array into a new array
pub fn array_slice(actor: &mut Actor, mut array: Value, start: Value, end: Value) -> HostResult
{
    let start = unwrap_usize!(start);
    let end = unwrap_usize!(end);
    let len = unwrap_arr!(array).len();

    if start > end || end > len {
        error!("slice range {}..{} is out of bounds for an array of length {}", start, end, len);
    }

    actor.gc_check(
        Array::alloc_size(end - start),
        &mut [&mut array],
    );

    Ok(array.as_arr().slice(start, end, &mut actor.alloc))
}
