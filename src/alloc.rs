use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};

pub struct Alloc
{
    mem_block: *mut u8,
    mem_size: usize,
    next_idx: usize,
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
        }
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
    pub fn alloc<T>(&mut self, obj: T) -> *mut T
    {
        let num_bytes = std::mem::size_of::<T>();
        let bytes = self.alloc_bytes(num_bytes);
        let p = bytes as *mut T;

        // Write object at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::write(p, obj) };

        p
    }

    // Allocate an immutable string
    pub fn str_const(&mut self, s: String) -> *const String
    {
        let s_ptr = self.alloc(s);
        s_ptr as *const String
    }
}

// Allow sending allocators between threads
// This is needed for the message allocator
unsafe impl Send for Alloc {}
unsafe impl Sync for Alloc {}
