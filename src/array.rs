use crate::vm::Actor;
use crate::value::*;
use crate::alloc::{Alloc, Tag, HEADER_SIZE};
use crate::host::HostFn;
use crate::*;

pub struct Array
{
    // Relocated by the collector, which walks the table on its own
    pub(crate) elems: *mut [Value],
    len: usize,
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

    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Self
    {
        let table = alloc.alloc_table(capacity, Tag::ValueTable);
        Array { elems: table, len: 0 }
    }

    /// Capacity to grow to when the element table is full. Growing
    /// doubles, but an empty array has to end up with room for one.
    fn grown_capacity(&self) -> usize
    {
        std::cmp::max(self.capacity() * 2, 1)
    }

    pub fn len(&self) -> usize
    {
        self.len
    }

    pub fn capacity(&self) -> usize
    {
        self.elems.len()
    }

    pub fn get(&self, idx: usize) -> Value
    {
        unsafe { (*self.elems) [idx] }
    }

    pub fn set(&mut self, idx: usize, val: Value)
    {
        unsafe { (*self.elems) [idx] = val };
    }

    pub fn items(&self) -> &[Value] {
        let elems = unsafe { &*self.elems };
        &elems[..self.len]
    }

    pub fn items_mut(&mut self) -> &mut [Value] {
        let elems = unsafe { &mut *self.elems };
        &mut elems[..self.len]
    }

    pub fn push(&mut self, val: Value, alloc: &mut Alloc)
    {
        assert!(self.len <= self.elems.len());

        // If we are at capacity
        if self.len == self.elems.len() {
            let new_capacity = self.grown_capacity();
            let new_elems = unsafe { &mut *alloc.alloc_table(new_capacity, Tag::ValueTable) };
            new_elems[..self.len].copy_from_slice(self.items());
            self.elems = new_elems;
        }

        unsafe {
            (&mut *self.elems)[self.len] = val;
            self.len += 1;
        }
    }

    pub fn insert(&mut self, idx: usize, val: Value, alloc: &mut Alloc)
    {
        // If we are at capacity
        if self.len == self.elems.len() {
            let new_capacity = self.grown_capacity();
            let new_elems = unsafe { &mut *alloc.alloc_table(new_capacity, Tag::ValueTable) };
            new_elems[..self.len].copy_from_slice(self.items());
            self.elems = new_elems;
        }

        unsafe {
            (&mut *self.elems).copy_within(idx..self.len, idx + 1);
            (&mut *self.elems)[idx] = val;
            self.len += 1;
        }
    }

    pub fn remove(&mut self, idx: usize) -> Value
    {
        if idx >= self.len {
            return Value::NIL;
        }

        let removed = unsafe { (&mut *self.elems)[idx] };
        unsafe {
            (&mut *self.elems).copy_within(idx + 1..self.len, idx);
        }

        self.len -= 1;
        self.clear_slot(self.len);
        removed
    }

    pub fn append(&mut self, other: &Array, alloc: &mut Alloc) {
        let other_elems = other.items();
        let cur_len = self.len();

        if self.len + other_elems.len() > self.elems.len() {
            let new_len = self.len + other_elems.len();
            let new_elems = unsafe { &mut *alloc.alloc_table(new_len, Tag::ValueTable) };

            let elems = self.items();
            new_elems[..cur_len].copy_from_slice(elems);
            new_elems[cur_len..].copy_from_slice(other_elems);
            self.elems = new_elems;
        } else {
            let elems = unsafe { &mut *self.elems };
            elems[cur_len..cur_len + other_elems.len()].copy_from_slice(other_elems);
        }

        self.len += other_elems.len();
    }

    pub fn pop(&mut self) -> Value
    {
        if self.len == 0 {
            return Value::NIL;
        }

        self.len -= 1;
        let popped = unsafe { (*self.elems) [self.len] };
        self.clear_slot(self.len);
        popped
    }

    /// Clear a slot past the end of the array. The collector scans the
    /// whole table, so a stale value left here would be kept alive.
    fn clear_slot(&mut self, idx: usize)
    {
        unsafe { (*self.elems)[idx] = Value::UNDEF };
    }
}

pub fn array_with_size(actor: &mut Actor, _self: Value, num_elems: Value, mut fill_val: Value) -> Result<Value, String>
{
    let num_elems = unwrap_usize!(num_elems);

    actor.gc_check(
        Array::alloc_size(num_elems),
        &mut [&mut fill_val]
    );

    let elems = actor.alloc.alloc_table(num_elems, Tag::ValueTable);
    unsafe { (&mut *elems).fill(fill_val); }
    let arr = Array { elems, len: num_elems };
    Ok(Value::array(actor.alloc.alloc(arr, Tag::Array)))
}

pub fn array_push(actor: &mut Actor, mut array: Value, mut val: Value) -> Result<Value, String>
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

pub fn array_pop(actor: &mut Actor, mut array: Value) -> Result<Value, String>
{
    Ok(unwrap_arr!(array).pop())
}

pub fn array_remove(actor: &mut Actor, mut array: Value, idx: Value) -> Result<Value, String>
{
    let idx = unwrap_usize!(idx);
    Ok(unwrap_arr!(array).remove(idx))
}

pub fn array_insert(actor: &mut Actor, mut array: Value, mut idx: Value, mut val: Value) -> Result<Value, String>
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

pub fn array_append(actor: &mut Actor, mut self_array: Value, mut other_array: Value) -> Result<Value, String>
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
