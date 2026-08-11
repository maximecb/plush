use crate::str::Str;
use crate::vm::Value;

/// Initial size for a new allocator. Kept small so that actors are
/// cheap to spawn, since each one owns a heap.
pub const INIT_SIZE: usize = 4 * 1024 * 1024;

pub struct Alloc
{
    // Start of the reserved address range
    mem_block: *mut u8,

    // Size of the reserved address range
    reserve_size: usize,

    // Size of the committed, accessible region
    mem_size: usize,

    // System page size
    page_size: usize,

    next_idx: usize,
}

/// Round a size up to a multiple of the page size
fn page_round_up(size: usize, page_size: usize) -> usize
{
    let rem = size % page_size;

    if rem == 0 {
        size
    } else {
        size + (page_size - rem)
    }
}

/// Reserve a range of address space. PROT_NONE means that none of
/// it is accessible until it is committed.
fn reserve_range(size: usize) -> *mut u8
{
    let p = unsafe { libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_NONE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0
    )};

    if p == libc::MAP_FAILED {
        panic!("could not reserve {} bytes of address space", size);
    }

    p as *mut u8
}

impl Alloc
{
    pub fn new() -> Self
    {
        Self::with_size(INIT_SIZE)
    }

    pub fn with_size(mem_size_bytes: usize) -> Self
    {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        assert!(page_size % 8 == 0);

        // Reserve twice the initial size, so that the heap has room to
        // grow in place before the reservation has to be replaced
        let reserve_size = page_round_up(2 * mem_size_bytes, page_size);

        let mut alloc = Self {
            mem_block: reserve_range(reserve_size),
            reserve_size,
            mem_size: 0,
            page_size,
            next_idx: 0,
        };

        alloc.grow(mem_size_bytes);
        alloc
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

    /// Size of the reserved address range
    pub fn reserve_size(&self) -> usize
    {
        self.reserve_size
    }

    /// Grow the reserved address space to at least a given size.
    ///
    /// This replaces the reservation, which moves the memory block, so it
    /// is only legal while the allocator is empty. That is enough for the
    /// GC, which only ever grows a to-space before it starts copying into
    /// it. Everything committed is discarded.
    pub fn grow_reserve(&mut self, new_reserve: usize)
    {
        if new_reserve <= self.reserve_size {
            return;
        }

        assert!(
            self.next_idx == 0,
            "cannot grow the reservation of an allocator holding objects"
        );

        let new_reserve = page_round_up(new_reserve, self.page_size);
        let mem_block = reserve_range(new_reserve);

        // Release the old range. The allocator is empty, so the only thing
        // lost is memory we would have had to zero before reusing anyway.
        unsafe { libc::munmap(self.mem_block as *mut libc::c_void, self.reserve_size) };

        self.mem_block = mem_block;
        self.reserve_size = new_reserve;
        self.mem_size = 0;
    }

    /// Grow the accessible memory to at least a given size.
    /// Existing allocations keep their addresses, and the newly
    /// committed memory is guaranteed to be zeroed.
    pub fn grow(&mut self, new_size: usize)
    {
        let new_size = page_round_up(new_size, self.page_size);

        if new_size <= self.mem_size {
            return;
        }

        assert!(
            new_size <= self.reserve_size,
            "heap size {} exceeds the reserved address space",
            new_size
        );

        // Make the new pages accessible. They are zero-filled by the
        // kernel on first touch, so there is nothing to clear here.
        let ret = unsafe { libc::mprotect(
            self.mem_block.add(self.mem_size) as *mut libc::c_void,
            new_size - self.mem_size,
            libc::PROT_READ | libc::PROT_WRITE
        )};

        if ret != 0 {
            panic!("could not commit memory for the heap");
        }

        self.mem_size = new_size;
    }

    /// Shrink the available memory to a smaller size, releasing the
    /// physical pages back to the system
    /// This is primarily used to test the GC
    pub fn shrink_to(&mut self, new_size: usize)
    {
        assert!(new_size <= self.mem_size);
        assert!(self.next_idx <= new_size);

        let new_size = page_round_up(new_size, self.page_size);

        if new_size >= self.mem_size {
            return;
        }

        // Replacing the mapping releases the physical pages. The range
        // stays reserved, and reads back as zero if committed again.
        let p = unsafe { libc::mmap(
            self.mem_block.add(new_size) as *mut libc::c_void,
            self.mem_size - new_size,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
            -1,
            0
        )};

        if p == libc::MAP_FAILED {
            panic!("could not release memory from the heap");
        }

        self.mem_size = new_size;
    }

    /// Clear/erase all allocations
    pub fn clear(&mut self)
    {
        // Clear the memory up to the next allocation index
        // Some objects rely on uninitialized memory being zero
        unsafe { std::ptr::write_bytes(self.mem_block, 0, self.next_idx) }

        // Reset the next allocation index
        self.next_idx = 0;
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

    pub fn str(&mut self, s: &str) -> Result<*const Str, ()>
    {
        let bytes = self.alloc_bytes(s.len())?;
        let p = bytes as *mut u8;

        // Write string bytes at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len()) };
        let raw_str = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(p, s.len()))
        };
        let raw_str_ptr = raw_str as *const str;

        let p_str = self.alloc(Str::new(raw_str_ptr))?;
        Ok(p_str)
    }

    pub fn str_val(&mut self, s: &str) -> Result<Value, ()>
    {
        Ok(Value::String(self.str(s)?))
    }
}

// Allow sending allocators between threads
// This is needed for the message allocator
unsafe impl Send for Alloc {}
unsafe impl Sync for Alloc {}

impl Drop for Alloc
{
    fn drop(&mut self)
    {
        // In debug mode, fill the allocator's memory with 0xFE when dropping so that
        // we can find out quickly if any memory did not get copied in a GC cycle
        #[cfg(debug_assertions)]
        unsafe { std::ptr::write_bytes(self.mem_block, 0xFEu8, self.next_idx) }

        // Release the whole reserved range
        unsafe { libc::munmap(self.mem_block as *mut libc::c_void, self.reserve_size) };
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    /// Growing must not move objects, and new memory must be zeroed
    #[test]
    fn grow_keeps_addresses_and_zeroes()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        let p = alloc.alloc::<u64>(1337).unwrap();

        // An allocator can always grow into its own reservation
        let reserve_size = alloc.reserve_size();
        assert!(reserve_size >= 2 * 1024 * 1024);
        alloc.grow(reserve_size);

        assert!(unsafe { *p } == 1337);
        assert!(alloc.mem_size() == reserve_size);

        let table = alloc.alloc_table::<u64>(4096).unwrap();
        assert!(unsafe { (*table).iter().all(|&x| x == 0) });
    }

    /// An empty allocator can be given a larger reservation
    #[test]
    fn grow_reserve_on_empty_allocator()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        assert!(alloc.reserve_size() < 64 * 1024 * 1024);

        alloc.grow_reserve(64 * 1024 * 1024);
        assert!(alloc.reserve_size() >= 64 * 1024 * 1024);

        // The new reservation must be usable and zeroed
        alloc.grow(64 * 1024 * 1024);
        let table = alloc.alloc_table::<u64>(4 * 1024 * 1024).unwrap();
        assert!(unsafe { (*table).iter().all(|&x| x == 0) });
    }

    /// Growing the reservation moves the memory block, so it must not be
    /// allowed while the allocator still holds objects
    #[test]
    #[should_panic(expected = "holding objects")]
    fn grow_reserve_rejects_live_objects()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        alloc.alloc::<u64>(1337).unwrap();
        alloc.grow_reserve(64 * 1024 * 1024);
    }

    /// Growing to a smaller size must not shrink the allocator
    #[test]
    fn grow_is_monotonic()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        let mem_size = alloc.mem_size();
        alloc.grow(1024);
        assert!(alloc.mem_size() == mem_size);
    }

    /// Shrinking releases pages, which must read back as zero if reused
    #[test]
    fn shrink_then_grow_is_zeroed()
    {
        let mut alloc = Alloc::with_size(8 * 1024 * 1024);

        let table = alloc.alloc_table::<u64>(256 * 1024).unwrap();
        unsafe { (*table).fill(0xABAB_ABAB_ABAB_ABAB) };
        alloc.clear();

        alloc.shrink_to(64 * 1024);
        assert!(alloc.mem_size() <= 128 * 1024);

        alloc.grow(8 * 1024 * 1024);
        let table = alloc.alloc_table::<u64>(256 * 1024).unwrap();
        assert!(unsafe { (*table).iter().all(|&x| x == 0) });
    }

    /// Copying a heap into a to-space sized the way the GC sizes it must
    /// never run out of room, however the allocations are ordered
    #[test]
    fn copy_always_fits_in_twice_the_source()
    {
        // Sizes that force padding: interleaving odd and aligned sizes
        // costs more than grouping them, so the worst case depends on order
        let sizes: Vec<usize> = (0..20000).map(|i| match i % 4 {
            0 => 3, 1 => 8, 2 => 24, _ => 17
        }).collect();

        let mut src = Alloc::with_size(INIT_SIZE);
        for &s in &sizes {
            src.alloc_bytes(s).unwrap();
        }

        // Copy the same allocations back in the worst order we can pick
        let mut dst = Alloc::with_size(INIT_SIZE);
        dst.grow_reserve(2 * src.bytes_used());
        dst.grow(2 * src.bytes_used());

        let mut reordered = sizes.clone();
        reordered.sort();
        for &s in &reordered {
            assert!(dst.alloc_bytes(s).is_ok(), "copy ran out of space");
        }

        assert!(dst.bytes_used() <= 2 * src.bytes_used());
    }
}
