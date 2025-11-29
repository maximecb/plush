use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use crate::object::Object;
use crate::str::Str;
use crate::vm::Value;
use crate::ast::ClassId;

pub struct Alloc
{
    mem_block: *mut u8,
    mem_size: usize,
    next_idx: usize,
    layout: Layout,
}

impl Alloc
{
    pub fn new() -> Self
    {
        Self::with_size(16 * 1024 * 1024)
    }

    pub fn with_size(mem_size_bytes: usize) -> Self
    {
        let layout = Layout::from_size_align(mem_size_bytes, 8).unwrap();

        let mem_block = unsafe { alloc_zeroed(layout) };
        if mem_block.is_null() {
            panic!();
        }

        Self {
            mem_block,
            mem_size: mem_size_bytes,
            next_idx: 0,
            layout,
        }
    }

    pub fn mem_size(&self) -> usize
    {
        self.mem_size
    }

    pub fn bytes_used(&self) -> usize
    {
        self.next_idx
    }

    pub fn bytes_free(&self) -> usize
    {
        assert!(self.next_idx <= self.mem_size);
        self.mem_size - self.next_idx
    }

    /// Shrink the available memory to a smaller size
    /// This is primarily used to test the GC
    pub fn shrink_to(&mut self, new_size: usize)
    {
        assert!(self.next_idx <= new_size);
        self.mem_size = new_size;

        // TODO: try to realloc to a smaller size?
    }

    /// Allocate a block of a given size
    fn alloc_bytes(&mut self, size_bytes: usize) -> Result<*mut u8, ()>
    {
        let align_bytes = 8;

        // Align the current alloc index
        let obj_pos = (self.next_idx + (align_bytes - 1)) & !(align_bytes - 1);

        // Bump the next allocation index
        let next_idx = obj_pos + size_bytes;
        if next_idx > self.mem_size {
            return Err(())
        }
        self.next_idx = next_idx;

        Ok(unsafe { self.mem_block.add(obj_pos) })
    }

    /// Allocate a variable-sized table of elements of a given type
    pub fn alloc_table<T>(&mut self, num_elems: usize) -> Result<*mut [T], ()>
    {
        let num_bytes = num_elems * std::mem::size_of::<T>();
        let bytes = self.alloc_bytes(num_bytes)?;
        let p = bytes as *mut T;

        Ok(std::ptr::slice_from_raw_parts_mut(p, num_elems))
    }

    /// Allocate a new object of a given type
    pub fn alloc<T>(&mut self, obj: T) -> Result<*mut T, ()>
    {
        let num_bytes = std::mem::size_of::<T>();
        let bytes = self.alloc_bytes(num_bytes)?;
        let p = bytes as *mut T;

        // Write object at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::write(p, obj) };

        Ok(p)
    }

    pub fn raw_str(&mut self, s: &str) -> Result<Str, ()>
    {
        let bytes = self.alloc_bytes(s.len())?;
        let p = bytes as *mut u8;

        // Write string bytes at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len()) };
        let raw_str = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(p, s.len()))
        };
        Ok(Str::new(raw_str as *const str))
    }

    pub fn str(&mut self, s: &str) -> Result<*const Str, ()>
    {
        let inner = self.raw_str(s)?;
        let p_str = self.alloc(inner)?;
        Ok(p_str)
    }

    pub fn str_val(&mut self, s: &str) -> Result<Value, ()>
    {
        Ok(Value::String(self.str(s)?))
    }
}

impl Drop for Alloc
{
    fn drop(&mut self)
    {
        //println!("dropping alloc");

        // In debug mode, fill the allocator's memory with 0xFE when dropping so that
        // we can find out quickly if any memory did not get copied in a GC cycle
        #[cfg(debug_assertions)]
        unsafe { std::ptr::write_bytes(self.mem_block, 0xFEu8, self.mem_size) }

        // Deallocate the memory block
        unsafe { dealloc(self.mem_block, self.layout) };
    }
}

// Allow sending allocators between threads
// This is needed for the message allocator
unsafe impl Send for Alloc {}
unsafe impl Sync for Alloc {}
