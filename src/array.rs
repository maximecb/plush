use crate::vm::{Value, Actor};
use crate::alloc::Alloc;
use crate::host::HostFn;

pub struct Array
{
    elems: *mut [Value],
    len: usize,
}

impl Array
{
    pub fn with_capacity(capacity: usize, alloc: &mut Alloc) -> Result<Self, ()>
    {
        let capacity = std::cmp::max(capacity, 1);
        let table = alloc.alloc_table(capacity)?;
        Ok(Array { elems: table, len: 0 })
    }

    pub fn clone(&self, alloc: &mut Alloc) -> Result<Self, ()>
    {
        let table = alloc.alloc_table(self.len)?;
        let mut new_arr = Array { elems: table, len: self.len };
        new_arr.items_mut().copy_from_slice(self.items());
        Ok(new_arr)
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

    pub fn push(&mut self, val: Value, alloc: &mut Alloc) -> Result<(), ()>
    {
        assert!(self.len <= self.elems.len());

        let elems = self.items_mut();

        // If we are at capacity
        if self.len == self.elems.len() {
            let new_len = self.len * 2;
            let new_elems = unsafe { &mut *alloc.alloc_table(new_len)? };
            new_elems[..self.len].copy_from_slice(self.items());
            self.elems = new_elems;
        }

        unsafe {
            (&mut *self.elems)[self.len] = val;
            self.len += 1;
        }

        Ok(())
    }

    pub fn insert(&mut self, idx: usize, val: Value, alloc: &mut Alloc) -> Result<(), ()>
    {
        // If we are at capacity
        if self.len == self.elems.len() {
            let new_len = self.len * 2;
            let new_elems = unsafe { &mut *alloc.alloc_table(new_len)? };
            new_elems[..self.len].copy_from_slice(self.items());
            self.elems = new_elems;
        }

        unsafe {
            (&mut *self.elems).copy_within(idx..self.len, idx + 1);
            (&mut *self.elems)[idx] = val;
            self.len += 1;
        }

        Ok(())
    }

    pub fn remove(&mut self, idx: usize) -> Value
    {
        if idx >= self.len {
            return Value::Nil;
        }

        let removed = unsafe { (&mut *self.elems)[idx] };
        unsafe {
            (&mut *self.elems).copy_within(idx + 1..self.len, idx);
        }

        self.len -= 1;
        removed
    }

    pub fn extend(&mut self, other: &Array, alloc: &mut Alloc) -> Result<(), ()> {
        let other_elems = other.items();
        let cur_len = self.len();

        if self.len + other_elems.len() > self.elems.len() {
            let new_len = self.len + other_elems.len();
            let new_elems = unsafe { &mut *alloc.alloc_table(new_len)? };

            let elems = self.items();
            new_elems[..cur_len].copy_from_slice(elems);
            new_elems[cur_len..].copy_from_slice(other_elems);
            self.elems = new_elems;
        } else {
            let mut elems = self.items_mut();
            elems[cur_len..].copy_from_slice(other.items());
        }

        self.len += other_elems.len();

        Ok(())
    }

    pub fn pop(&mut self) -> Value
    {
        if self.len == 0 {
            return Value::Nil;
        }

        self.len -= 1;
        unsafe { (*self.elems) [self.len] }
    }
}

pub fn array_with_size(actor: &mut Actor, _self: Value, num_elems: Value, fill_val: Value) -> Result<Value, String>
{
    let num_elems = num_elems.unwrap_usize();
    let mut elems = actor.alloc.alloc_table(num_elems).unwrap();
    unsafe { (&mut *elems).fill(fill_val); }
    let arr = Array { elems, len: num_elems };
    Ok(Value::Array(actor.alloc.alloc(arr).unwrap()))
}

pub fn array_push(actor: &mut Actor, mut array: Value, mut val: Value) -> Result<Value, String>
{
    let arr = array.unwrap_arr();

    if arr.len() == arr.capacity() {
        actor.gc_check(
            size_of::<Array>() + size_of::<Value>() * arr.capacity() * 2,
            &mut [&mut array, &mut val]
        )
    }

    let arr = array.unwrap_arr();
    arr.push(val, &mut actor.alloc).unwrap();
    Ok(Value::Nil)
}

pub fn array_pop(actor: &mut Actor, mut array: Value) -> Result<Value, String>
{
    Ok(array.unwrap_arr().pop())
}

pub fn array_remove(actor: &mut Actor, mut array: Value, idx: Value) -> Result<Value, String>
{
    let idx = idx.unwrap_usize();
    Ok(array.unwrap_arr().remove(idx))
}

pub fn array_insert(actor: &mut Actor, mut array: Value, idx: Value, val: Value) -> Result<Value, String>
{
    let idx = idx.unwrap_usize();
    array.unwrap_arr().insert(idx, val, &mut actor.alloc).unwrap();
    Ok(Value::Nil)
}

pub fn array_append(actor: &mut Actor, mut self_array: Value, mut other_array: Value) -> Result<Value, String>
{
    let other_elems = other_array.unwrap_arr();
    self_array.unwrap_arr().extend(other_elems, &mut actor.alloc).unwrap();
    Ok(Value::Nil)
}
