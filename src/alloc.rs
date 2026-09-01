use std::mem::{align_of, size_of};
use crate::value::Value;

/// Initial size for a new heap. Kept small so that actors are cheap to
/// spawn, since each one owns a heap. Heaps grow as needed.
pub const INIT_SIZE: usize = 4 * 1024 * 1024;

/// Initial size of a message allocator. These grow on demand, so this
/// only has to cover ordinary message traffic.
pub const MSG_INIT_SIZE: usize = 2 * 1024 * 1024;

/// Address space reserved for a message allocator. A message allocator
/// cannot be re-reserved while it holds messages, so it reserves enough
/// up front to grow into. Reserving costs address space but no memory,
/// and this is what bounds how large a single message can be.
pub const MSG_RESERVE_SIZE: usize = 16 * 1024 * 1024 * 1024;

/// Alignment of every allocation
const ALIGN: usize = 8;

/// What kind of block an allocation holds. The collector needs this to
/// know which references live inside a block it has walked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag
{
    // Objects with their variable-sized part stored inline
    Str = 1,
    Object,
    Closure,

    // Objects pointing to a separate table
    Array,
    ByteArray,
    Dict,

    // Captured variable, holds a single value
    Cell,

    // Boxed numbers that don't fit in an immediate value
    Int64,
    Float64,

    // Tables, referenced by the objects above
    ValueTable,
    SlotTable,
    Bytes,
}

impl Tag
{
    const LAST: u8 = Tag::Bytes as u8;

    fn from_u8(val: u8) -> Tag
    {
        assert!(val >= 1 && val <= Tag::LAST, "invalid block tag {}", val);
        unsafe { std::mem::transmute(val) }
    }
}

/// Header word preceding every allocation.
///
/// Bit 0 set means the block is live: bits 1..8 hold the tag and bits
/// 8..64 the payload size. Bit 0 clear means the block has been copied
/// and the word is the address the payload moved to, which is aligned
/// and so always has bit 0 clear.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Header(u64);

/// Size of the header preceding every allocation
pub const HEADER_SIZE: usize = size_of::<Header>();

impl Header
{
    fn new(tag: Tag, size: usize) -> Self
    {
        debug_assert!(size < (1 << 56), "block too large to fit in a header");
        Header(((size as u64) << 8) | ((tag as u64) << 1) | 1)
    }

    pub fn is_forwarded(&self) -> bool
    {
        self.0 & 1 == 0
    }

    /// Address the payload moved to. Only valid if the block is forwarded.
    pub fn forward_addr(&self) -> *mut u8
    {
        debug_assert!(self.is_forwarded());
        self.0 as *mut u8
    }

    pub fn tag(&self) -> Tag
    {
        debug_assert!(!self.is_forwarded());
        Tag::from_u8(((self.0 >> 1) & 0x7F) as u8)
    }

    /// Payload size in bytes, already rounded up to the alignment
    pub fn size(&self) -> usize
    {
        debug_assert!(!self.is_forwarded());
        (self.0 >> 8) as usize
    }
}

/// Read the header of the block whose payload starts at a given address
pub fn header_of(payload: *const u8) -> Header
{
    unsafe { *(payload as *const Header).sub(1) }
}

/// Record that a block has been copied, and where its payload moved to
pub fn set_forwarded(payload: *mut u8, new_payload: *mut u8)
{
    debug_assert!(new_payload as usize % ALIGN == 0);
    unsafe { *(payload as *mut Header).sub(1) = Header(new_payload as u64) };
}

/// Put back a header that was overwritten with a forwarding address
pub fn restore_header(payload: *mut u8, header: Header)
{
    unsafe { *(payload as *mut Header).sub(1) = header };
}

/// Round a size up to the allocation alignment
fn align_up(size: usize) -> usize
{
    (size + (ALIGN - 1)) & !(ALIGN - 1)
}

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

    // Whether allocation may commit more memory on demand. Message
    // allocators do; heaps do not, because the collector decides when
    // they grow.
    growable: bool,

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

/// The virtual memory calls the allocator is built on. A range of
/// address space is reserved up front, and pages within it are
/// committed and decommitted as the heap grows and shrinks.
#[cfg(unix)]
mod sys
{
    /// Reserve address space without committing memory to it. None of
    /// the range is accessible until it is committed. Returns null if
    /// the reservation fails.
    pub fn reserve(size: usize) -> *mut u8
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
            return std::ptr::null_mut();
        }

        p as *mut u8
    }

    /// Make a range inside a reservation readable and writable. The
    /// pages it hands back are zero-filled.
    pub fn commit(addr: *mut u8, size: usize) -> bool
    {
        let ret = unsafe { libc::mprotect(
            addr as *mut libc::c_void,
            size,
            libc::PROT_READ | libc::PROT_WRITE
        )};

        ret == 0
    }

    /// Release the physical pages backing a range, leaving it reserved.
    /// Replacing the mapping is what frees the pages.
    pub fn decommit(addr: *mut u8, size: usize) -> bool
    {
        let p = unsafe { libc::mmap(
            addr as *mut libc::c_void,
            size,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
            -1,
            0
        )};

        p != libc::MAP_FAILED
    }

    /// Give up a whole reservation, committed pages and all
    pub fn release(addr: *mut u8, size: usize)
    {
        unsafe { libc::munmap(addr as *mut libc::c_void, size) };
    }

    pub fn page_size() -> usize
    {
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }
}

#[cfg(windows)]
mod sys
{
    use core::ffi::c_void;
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc,
        VirtualFree,
        MEM_COMMIT,
        MEM_DECOMMIT,
        MEM_RELEASE,
        MEM_RESERVE,
        PAGE_NOACCESS,
        PAGE_READWRITE,
    };
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};

    /// Reserve address space without committing memory to it. None of
    /// the range is accessible until it is committed. Returns null if
    /// the reservation fails.
    pub fn reserve(size: usize) -> *mut u8
    {
        unsafe { VirtualAlloc(
            std::ptr::null(),
            size,
            MEM_RESERVE,
            PAGE_NOACCESS
        ) as *mut u8 }
    }

    /// Make a range inside a reservation readable and writable. The
    /// pages it hands back are zero-filled.
    pub fn commit(addr: *mut u8, size: usize) -> bool
    {
        let p = unsafe { VirtualAlloc(
            addr as *const c_void,
            size,
            MEM_COMMIT,
            PAGE_READWRITE
        )};

        !p.is_null()
    }

    /// Release the physical pages backing a range, leaving it reserved
    pub fn decommit(addr: *mut u8, size: usize) -> bool
    {
        unsafe { VirtualFree(addr as *mut c_void, size, MEM_DECOMMIT) != 0 }
    }

    /// Give up a whole reservation, committed pages and all. This only
    /// works on the address the reservation started at, and the size
    /// has to be zero.
    pub fn release(addr: *mut u8, _size: usize)
    {
        unsafe { VirtualFree(addr as *mut c_void, 0, MEM_RELEASE) };
    }

    pub fn page_size() -> usize
    {
        let mut info = SYSTEM_INFO::default();
        unsafe { GetSystemInfo(&mut info) };
        info.dwPageSize as usize
    }
}

/// Reserve a range of address space. None of it is accessible until
/// it is committed.
fn reserve_range(size: usize) -> *mut u8
{
    let p = sys::reserve(size);

    if p.is_null() {
        panic!("could not reserve {} bytes of address space", size);
    }

    p
}

impl Alloc
{
    pub fn new() -> Self
    {
        Self::with_size(INIT_SIZE)
    }

    /// Allocator for incoming messages. These grow on demand as messages
    /// are copied in, and are reset once the receiver has drained them.
    pub fn for_messages() -> Self
    {
        Self::with_reserve(MSG_INIT_SIZE, MSG_RESERVE_SIZE, true)
    }

    pub fn with_size(mem_size_bytes: usize) -> Self
    {
        // Reserve twice the initial size, so that the heap has room to
        // grow in place before the reservation has to be replaced
        Self::with_reserve(mem_size_bytes, 2 * mem_size_bytes, false)
    }

    fn with_reserve(mem_size_bytes: usize, reserve_size: usize, growable: bool) -> Self
    {
        let page_size = sys::page_size();
        assert!(page_size % 8 == 0);

        let reserve_size = page_round_up(reserve_size, page_size);

        let mut alloc = Self {
            mem_block: reserve_range(reserve_size),
            reserve_size,
            mem_size: 0,
            page_size,
            growable,
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
    #[allow(dead_code)] // used by the unit tests below
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
        sys::release(self.mem_block, self.reserve_size);

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
        let ok = sys::commit(
            unsafe { self.mem_block.add(self.mem_size) },
            new_size - self.mem_size
        );

        if !ok {
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

        // Decommitting releases the physical pages. The range stays
        // reserved, and reads back as zero if committed again.
        let ok = sys::decommit(
            unsafe { self.mem_block.add(new_size) },
            self.mem_size - new_size
        );

        if !ok {
            panic!("could not release memory from the heap");
        }

        self.mem_size = new_size;
    }

    /// Discard all allocations, leaving the memory as it was.
    ///
    /// The bytes are deliberately not zeroed here. Everything that fills
    /// a reset allocator writes every byte of what it allocates: the
    /// collector copies whole blocks in. What it does not reach has to be
    /// zeroed with zero_up_to before anything relying on zeroed memory
    /// allocates from it again.
    pub fn reset(&mut self)
    {
        self.next_idx = 0;
    }

    /// Zero the bytes between the allocation point and a given offset.
    ///
    /// This is how a reset allocator is made safe to allocate from
    /// again, once we know how much of it an incoming copy overwrote.
    /// Anything past the offset was never written and is already zero.
    pub fn zero_up_to(&mut self, end: usize)
    {
        let end = std::cmp::min(end, self.mem_size);

        if end > self.next_idx {
            unsafe { std::ptr::write_bytes(
                self.mem_block.add(self.next_idx),
                0,
                end - self.next_idx
            )};
        }
    }

    /// Payload pointer for the block starting at a given byte offset.
    /// The collector uses this to walk the heap block by block.
    pub fn block_at(&self, offset: usize) -> *mut u8
    {
        debug_assert!(offset + HEADER_SIZE <= self.next_idx);
        unsafe { self.mem_block.add(offset + HEADER_SIZE) }
    }

    /// Whether an address is the payload of a block in this heap.
    ///
    /// An empty block has its payload one past the last byte allocated,
    /// so it is the header that has to be inside the region, not the
    /// address itself.
    #[allow(dead_code)] // used by the verify_gc heap walk
    pub fn contains(&self, p: *const u8) -> bool
    {
        let addr = p as usize;
        let start = self.mem_block as usize;

        addr >= start + HEADER_SIZE && addr - HEADER_SIZE < start + self.next_idx
    }

    /// Allocate a block with a given payload size and tag.
    ///
    /// Sizes are rounded up to the alignment so that blocks sit back to
    /// back with no gaps between them. That is what lets the collector
    /// walk the heap linearly, one header at a time.
    pub fn alloc_bytes(&mut self, size_bytes: usize, tag: Tag) -> *mut u8
    {
        let size_bytes = align_up(size_bytes);

        // Every block is a header plus a rounded payload, so the next
        // allocation index stays aligned without any adjustment here
        debug_assert!(self.next_idx % ALIGN == 0);
        let obj_pos = self.next_idx + HEADER_SIZE;

        // Bump the next allocation index
        let next_idx = obj_pos + size_bytes;

        if next_idx > self.mem_size {
            // Heaps do not grow here. Callers make room first: the mutator
            // through gc_check, the collector by sizing its to-space up
            // front. Growing instead of failing would let a heap expand
            // silently rather than be collected.
            if !self.growable {
                panic!(
                    "allocator out of memory, could not allocate {} bytes ({} of {} used)",
                    size_bytes,
                    self.next_idx,
                    self.mem_size,
                );
            }

            // Double, or jump straight to what a large allocation needs
            self.grow(std::cmp::max(next_idx, self.mem_size * 2));
        }

        self.next_idx = next_idx;

        unsafe {
            let p = self.mem_block.add(obj_pos);
            std::ptr::write(
                p.sub(HEADER_SIZE) as *mut Header,
                Header::new(tag, size_bytes)
            );
            p
        }
    }

    /// Allocate a variable-sized table of elements of a given type
    pub fn alloc_table<T>(&mut self, num_elems: usize, tag: Tag) -> *mut [T]
    {
        debug_assert!(align_of::<T>() <= ALIGN);

        let num_bytes = num_elems * size_of::<T>();
        let bytes = self.alloc_bytes(num_bytes, tag);
        let p = bytes as *mut T;

        std::ptr::slice_from_raw_parts_mut(p, num_elems)
    }

    /// Allocate a new object of a given type
    pub fn alloc<T>(&mut self, obj: T, tag: Tag) -> *mut T
    {
        self.alloc_var(obj, 0, tag)
    }

    /// Allocate an object with a variable-sized tail stored right after it
    pub fn alloc_var<T>(&mut self, obj: T, tail_bytes: usize, tag: Tag) -> *mut T
    {
        debug_assert!(align_of::<T>() <= ALIGN);

        let bytes = self.alloc_bytes(size_of::<T>() + tail_bytes, tag);
        let p = bytes as *mut T;

        // Write object at location without calling drop
        // on what's currently at that location
        unsafe { std::ptr::write(p, obj) };

        p
    }

    /// Box an integer that is too large to be a fixnum
    pub fn heap_int64(&mut self, val: i64) -> Value
    {
        debug_assert!(!Value::fits_fixnum(val));
        Value::int64_box(self.alloc(val, Tag::Int64))
    }

    /// Box a double that has no inline flonum encoding
    pub fn heap_float64(&mut self, val: f64) -> Value
    {
        debug_assert!(Value::try_flonum(val).is_none());
        Value::float64_box(self.alloc(val, Tag::Float64))
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
        // Release the whole reserved range
        sys::release(self.mem_block, self.reserve_size);
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
        let p = alloc.alloc::<u64>(1337, Tag::Bytes);

        // An allocator can always grow into its own reservation
        let reserve_size = alloc.reserve_size();
        assert!(reserve_size >= 2 * 1024 * 1024);
        alloc.grow(reserve_size);

        assert!(unsafe { *p } == 1337);
        assert!(alloc.mem_size() == reserve_size);

        let table = alloc.alloc_table::<u64>(4096, Tag::Bytes);
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
        let table = alloc.alloc_table::<u64>(4 * 1024 * 1024, Tag::Bytes);
        assert!(unsafe { (*table).iter().all(|&x| x == 0) });
    }

    /// Growing the reservation moves the memory block, so it must not be
    /// allowed while the allocator still holds objects
    #[test]
    #[should_panic(expected = "holding objects")]
    fn grow_reserve_rejects_live_objects()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        alloc.alloc::<u64>(1337, Tag::Bytes);
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

        let table = alloc.alloc_table::<u64>(256 * 1024, Tag::Bytes);
        unsafe { (*table).fill(0xABAB_ABAB_ABAB_ABAB) };
        alloc.reset();

        // Release every page, so none of the old contents can come back
        alloc.shrink_to(0);
        assert!(alloc.mem_size() == 0);

        alloc.grow(8 * 1024 * 1024);
        let table = alloc.alloc_table::<u64>(256 * 1024, Tag::Bytes);
        assert!(unsafe { (*table).iter().all(|&x| x == 0) });
    }

    /// Resetting leaves the old bytes in place, and zero_up_to clears
    /// whatever was not allocated over in the meantime
    #[test]
    fn zero_up_to_clears_what_was_not_overwritten()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);

        let table = alloc.alloc_table::<u64>(1024, Tag::Bytes);
        unsafe { (*table).fill(0xABAB_ABAB_ABAB_ABAB) };
        let dirty_bytes = alloc.bytes_used();
        alloc.reset();

        // Allocate over the first half, the way a copy would
        let kept = alloc.alloc_table::<u64>(512, Tag::Bytes);
        unsafe { (*kept).fill(1337) };

        alloc.zero_up_to(dirty_bytes);

        // What we wrote is untouched, and the tail is back to zero
        assert!(unsafe { (*kept).iter().all(|&x| x == 1337) });
        let rest = alloc.alloc_table::<u64>(510, Tag::Bytes);
        assert!(unsafe { (*rest).iter().all(|&x| x == 0) });
    }

    /// Zeroing must stop at the committed size, not run off the end
    #[test]
    fn zero_up_to_is_clamped_to_the_heap()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        let mem_size = alloc.mem_size();

        alloc.alloc_table::<u64>(8, Tag::Bytes);
        alloc.zero_up_to(usize::MAX);

        assert!(alloc.mem_size() == mem_size);
    }

    /// A block costs its header plus its own rounded size and nothing
    /// else, so copying a heap into another cannot make it grow, whatever
    /// order the copy visits the objects in
    #[test]
    fn copy_cannot_grow_the_heap()
    {
        // Sizes that would force padding if blocks were not rounded up
        let sizes: Vec<usize> = (0..20000).map(|i| match i % 4 {
            0 => 3, 1 => 8, 2 => 24, _ => 17
        }).collect();

        let mut src = Alloc::with_size(INIT_SIZE);
        for &s in &sizes {
            src.alloc_bytes(s, Tag::Bytes);
        }

        // Copy the same allocations back in a different order
        let mut dst = Alloc::with_size(INIT_SIZE);
        dst.grow_reserve(2 * src.bytes_used());
        dst.grow(2 * src.bytes_used());

        let mut reordered = sizes.clone();
        reordered.sort();
        for &s in &reordered {
            dst.alloc_bytes(s, Tag::Bytes);
        }

        assert!(dst.bytes_used() == src.bytes_used());
    }

    /// Blocks sit back to back, so a linear walk must land on every one
    /// of them and recover the tag and size it was allocated with
    #[test]
    fn heap_walk_visits_every_block()
    {
        let blocks: Vec<(usize, Tag)> = (0..1000).map(|i| match i % 4 {
            0 => (3, Tag::Bytes),
            1 => (8, Tag::ValueTable),
            2 => (24, Tag::Object),
            _ => (17, Tag::Str),
        }).collect();

        let mut alloc = Alloc::with_size(INIT_SIZE);
        for &(size, tag) in &blocks {
            alloc.alloc_bytes(size, tag);
        }

        let mut offset = 0;
        let mut visited = 0;

        while offset < alloc.bytes_used() {
            let p = alloc.block_at(offset);
            let hdr = header_of(p);
            let (size, tag) = blocks[visited];

            assert!(alloc.contains(p));
            assert!(!hdr.is_forwarded());
            assert!(hdr.tag() == tag);
            assert!(hdr.size() == align_up(size));

            offset += HEADER_SIZE + hdr.size();
            visited += 1;
        }

        assert!(visited == blocks.len());
    }

    /// A forwarded header reads back as the address the block moved to,
    /// and can be put back the way it was
    #[test]
    fn forwarding_round_trips()
    {
        let mut alloc = Alloc::with_size(INIT_SIZE);
        let p = alloc.alloc_bytes(24, Tag::Object);
        let q = alloc.alloc_bytes(24, Tag::Object);

        let hdr = header_of(p);
        assert!(!hdr.is_forwarded());

        set_forwarded(p, q);
        assert!(header_of(p).is_forwarded());
        assert!(header_of(p).forward_addr() == q);

        restore_header(p, hdr);
        assert!(!header_of(p).is_forwarded());
        assert!(header_of(p).tag() == Tag::Object);
        assert!(header_of(p).size() == 24);
    }

    /// Tables start out zeroed and the collector scans every slot of the
    /// ones it walks, so zeroed memory has to read back as a value that
    /// holds no reference
    #[test]
    fn zeroed_memory_holds_no_reference()
    {
        let mut alloc = Alloc::with_size(INIT_SIZE);
        let table = alloc.alloc_table::<Value>(64, Tag::ValueTable);

        for val in unsafe { &*table } {
            assert!(!val.is_heap());
            assert!(*val == Value::fixnum(0));
        }
    }
}
