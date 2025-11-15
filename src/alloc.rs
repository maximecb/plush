use std::{alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout}, f32::consts::E};
use crate::vm::Value;

pub struct Alloc
{
    mem_block: *mut u8,
    mem_size: usize,
    next_idx: usize,
    is_message_alloc: bool,
}

fn pointer_as_string(ptr: *mut String) -> Value {
    Value::String(ptr as *const String)
}

impl Alloc
{
    pub fn new() -> Self
    {
        let mem_size = 128 * 1024 * 1024;
        let layout = Layout::from_size_align(mem_size, 8).unwrap();

        let mem_block = unsafe { alloc_zeroed(layout) };
        if mem_block.is_null() {
            panic!();
        }

        Self {
            mem_block,
            mem_size,
            next_idx: 0,
            is_message_alloc: false,
        }
    }

    pub fn new_message() -> Self {
        let mut alloc = Self::new();
        alloc.is_message_alloc = true;
        alloc
    }

    // Allocate a block of a given size
    pub fn alloc_bytes(&mut self, size_bytes: usize) -> *mut u8
    {
        let align_bytes = 8;

        // Align the current alloc index
        let obj_pos = (self.next_idx + (align_bytes - 1)) & !(align_bytes - 1);

        // Bump the next allocation index
        let next_idx = obj_pos + size_bytes;
        if next_idx >= self.mem_size {
            panic!("allocator out of memory");
        }
        self.next_idx = next_idx;

        unsafe {
            self.mem_block.add(obj_pos)
        }
    }

    // Allocate a new object of a given type
    pub fn alloc<T>(&mut self, obj: T, value_wrapper: fn(*mut T) -> Value, roots: impl Iterator<Item = Value>) -> Value
    {
        let num_bytes = std::mem::size_of::<T>();
        let bytes = self.alloc_bytes(num_bytes);
        let p = bytes as *mut T;

        // Write object at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::write(p, obj) };

        value_wrapper(p)
    }


    pub fn str_val(&mut self, s: String, roots: impl Iterator<Item = Value>) -> Value
    {
        self.alloc(s, pointer_as_string, roots)
    }
}

// Allow sending allocators between threads
// This is needed for the message allocator
unsafe impl Send for Alloc {}
unsafe impl Sync for Alloc {}
