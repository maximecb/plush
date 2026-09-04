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
///
/// The order matters: the tag also says how to read the rest of the
/// header, and the three layouts below are told apart by comparing
/// against FIRST_SLOTS and FIRST_FIXED, so each group has to stay
/// contiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag
{
    // Raw blocks: the header holds the size, and nothing else needs a
    // field of its own. Strings keep their inline bytes here, and the
    // tables the containers below point at are raw blocks too.
    Str = 1,
    ValueTable,
    SlotTable,
    Bytes,

    // Blocks whose payload is a run of slots, with the id of what
    // describes them in the header alongside the slot count
    Object,
    Closure,

    // Blocks with an eight-byte payload, so the header holds a length
    // instead of a size. The first three are a thin pointer to a table,
    // the rest a single value.
    Array,
    Dict,
    ByteArray,
    Cell,
    Int64,
    Float64,
}

impl Tag
{
    const FIRST_SLOTS: u8 = Tag::Object as u8;
    const FIRST_FIXED: u8 = Tag::Array as u8;
    const LAST: u8 = Tag::Float64 as u8;

    fn from_u8(val: u8) -> Tag
    {
        assert!(val >= 1 && val <= Tag::LAST, "invalid block tag {}", val);
        unsafe { std::mem::transmute(val) }
    }
}

// Field positions within a live header. The tag says which of the three
// layouts applies, and the reserved bits sit at the same place in all of
// them so that a collector can mark a block without decoding the tag.
const TAG_SHIFT: u32 = 1;
const TAG_BITS: u64 = 0x7F;

const RAW_SHIFT: u32 = 8;
const RAW_BITS: u64 = (1 << 46) - 1;
// Bits 54..58 of a raw header are unused

const AUX24_SHIFT: u32 = 8;
const AUX24_BITS: u64 = (1 << 24) - 1;
const SLOTS_SHIFT: u32 = 32;
const SLOTS_BITS: u64 = 0xFFFF;

const LEN_SHIFT: u32 = 8;
const LEN_BITS: u64 = (1 << 50) - 1;

/// Largest values each field can hold, checked where blocks are made
pub const MAX_AUX24: usize = AUX24_BITS as usize;
pub const MAX_NUM_SLOTS: usize = SLOTS_BITS as usize;
pub const MAX_FIXED_LEN: usize = LEN_BITS as usize;

/// Header word preceding every allocation.
///
/// Bit 0 set means the block is live. Bit 0 clear means the block has
/// been copied and the whole word is the address the payload moved to,
/// which is aligned and so always has bit 0 clear. That is why the tag
/// starts at bit 1 rather than filling the low byte: bit 0 is what tells
/// a live header from a forwarding address, and the eight-alignment of
/// every block is what guarantees a real address never collides with it.
///
/// Every live header carries the tag in bits 1..8 and leaves bits 58..64
/// for a future collector, at the same place whatever the tag, so that a
/// mark or a lock can be set without decoding what the block is. The tag
/// says how to read the bits between, in one of three layouts:
///
/// ```text
///                 1     8              32        48   54   58      64
///                 +-----+--------------+---------+----+----+-------+
/// raw    live tag | bytes                        |  --     | resvd |
/// slots  live tag | class or fun id   | num_slots|   --    | resvd |
/// fixed  live tag | length                            | resvd |
///                 +-----+--------------+---------+----+----+-------+
/// ```
///
/// A raw header counts the bytes the block was asked for, not the
/// rounded block size, so nothing has to be stored to recover an exact
/// length; size() rounds up on the way out.
///
/// And per block kind:
///
/// ```text
/// Str         tag(7) | len in bytes(46) | ---(4) | reserved(6)
///   Bytes inline, at the block address, and the length is the field
///   itself. 64 TB, against a 16 GB cap on a single message.
///
/// ValueTable  tag(7) | size in bytes(46) | ---(4) | reserved(6)
/// SlotTable   tag(7) | size in bytes(46) | ---(4) | reserved(6)
/// Bytes       tag(7) | size in bytes(46) | ---(4) | reserved(6)
///   An array's, dict's or bytearray's table. The owner reads its
///   capacity back out of this size, which is why it can point at the
///   table with a plain pointer.
///
/// Object      tag(7) | class_id(24) | num_slots(16) | ---(10) | resvd(6)
/// Closure     tag(7) | fun_id(24)   | num_slots(16) | ---(10) | resvd(6)
///   Slots inline, at the block address. The low 32 bits are the tag and
///   the id together, which is the whole of a field or method cache
///   guard: one compare proves both the kind and the class.
///
/// Array       tag(7) | len in values(50)  | reserved(6)
/// Dict        tag(7) | len in entries(50) | reserved(6)
/// ByteArray   tag(7) | len in bytes(50)   | reserved(6)
///   Payload is one thin pointer to the table above. The unit of the
///   length differs per kind and is the block's own business; nothing
///   generic reads it, because the size of these blocks is a constant.
///
/// Cell        tag(7) | ---(50) | reserved(6)
/// Int64       tag(7) | ---(50) | reserved(6)
/// Float64     tag(7) | ---(50) | reserved(6)
///   Payload is the single value they box, and there is no length.
/// ```
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Header(u64);

/// Size of the header preceding every allocation
pub const HEADER_SIZE: usize = size_of::<Header>();

/// Payload size of every fixed-layout block: one pointer or one value
pub const FIXED_SIZE: usize = 8;

impl Header
{
    /// Header for a raw block holding a given number of bytes.
    ///
    /// The count is the exact byte length asked for, not the rounded
    /// block size, so a string reads its length straight back out and a
    /// table of bytes reports the capacity it was actually given. The
    /// rounding up to a whole word is recovered by size() below.
    fn new_raw(tag: Tag, num_bytes: usize) -> Self
    {
        debug_assert!((tag as u8) < Tag::FIRST_SLOTS);
        assert!(num_bytes as u64 <= RAW_BITS, "block too large to fit in a header");

        Header(((num_bytes as u64) << RAW_SHIFT) | ((tag as u64) << TAG_SHIFT) | 1)
    }

    /// Header for a block of slots, tagged with the class or function
    /// the slots belong to
    fn new_slots(tag: Tag, aux24: usize, num_slots: usize) -> Self
    {
        debug_assert!((tag as u8) >= Tag::FIRST_SLOTS && (tag as u8) < Tag::FIRST_FIXED);
        assert!(aux24 <= MAX_AUX24, "class or function id too large for a header");
        assert!(num_slots <= MAX_NUM_SLOTS, "too many slots to fit in a header");

        Header(
            ((num_slots as u64) << SLOTS_SHIFT) |
            ((aux24 as u64) << AUX24_SHIFT) |
            ((tag as u64) << TAG_SHIFT) | 1
        )
    }

    /// Header for an eight-byte block, whose header holds a length
    /// rather than a size. The unit of that length is the block's own
    /// business: values for an array, entries for a dict, bytes for a
    /// bytearray. Nothing generic reads it.
    fn new_fixed(tag: Tag, len: usize) -> Self
    {
        debug_assert!((tag as u8) >= Tag::FIRST_FIXED);
        assert!(len <= MAX_FIXED_LEN, "length too large to fit in a header");

        Header(((len as u64) << LEN_SHIFT) | ((tag as u64) << TAG_SHIFT) | 1)
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

    fn tag_u8(&self) -> u8
    {
        debug_assert!(!self.is_forwarded());
        ((self.0 >> TAG_SHIFT) & TAG_BITS) as u8
    }

    pub fn tag(&self) -> Tag
    {
        Tag::from_u8(self.tag_u8())
    }

    /// Payload size in bytes, rounded up to the alignment.
    ///
    /// This is the one field the collector reads without knowing what
    /// kind of block it is looking at, so the three layouts are decoded
    /// here with compares rather than a table: both candidates come out
    /// of the header word with no memory access of their own.
    pub fn size(&self) -> usize
    {
        let t = self.tag_u8();
        let raw = ((self.0 >> RAW_SHIFT) & RAW_BITS) as usize;
        let slots = ((self.0 >> SLOTS_SHIFT) & SLOTS_BITS) as usize;

        if t < Tag::FIRST_SLOTS {
            align_up(raw)
        } else if t < Tag::FIRST_FIXED {
            slots * size_of::<Value>()
        } else {
            FIXED_SIZE
        }
    }

    /// Bytes a raw block was asked for, before the rounding up to a
    /// word that size() reports. This is a string's length and a
    /// table's capacity.
    pub fn num_bytes(&self) -> usize
    {
        debug_assert!(self.tag_u8() < Tag::FIRST_SLOTS);
        ((self.0 >> RAW_SHIFT) & RAW_BITS) as usize
    }

    /// The class or function id of a block of slots
    pub fn aux24(&self) -> usize
    {
        debug_assert!(self.tag_u8() >= Tag::FIRST_SLOTS && self.tag_u8() < Tag::FIRST_FIXED);
        ((self.0 >> AUX24_SHIFT) & AUX24_BITS) as usize
    }

    pub fn num_slots(&self) -> usize
    {
        debug_assert!(self.tag_u8() >= Tag::FIRST_SLOTS && self.tag_u8() < Tag::FIRST_FIXED);
        ((self.0 >> SLOTS_SHIFT) & SLOTS_BITS) as usize
    }

    pub fn fixed_len(&self) -> usize
    {
        debug_assert!(self.tag_u8() >= Tag::FIRST_FIXED);
        ((self.0 >> LEN_SHIFT) & LEN_BITS) as usize
    }

    /// The low half of a slots header: the tag and the class or function
    /// id together. A field or method cache guards on this, which checks
    /// that the block is an object of the right class in one compare.
    pub fn guard_key(&self) -> u32
    {
        self.0 as u32
    }

    /// The key every object of a given class has. The slot count lives
    /// above the low half, so it plays no part in this.
    pub fn object_key(class_id: usize) -> u32
    {
        Header::new_slots(Tag::Object, class_id, 0).guard_key()
    }

    /// The class an object key was built from. A site that needs the
    /// class itself keeps the key and comes back through here, rather
    /// than carrying both.
    pub fn class_id_of_key(key: u32) -> usize
    {
        (key >> AUX24_SHIFT) as usize
    }
}

/// Read the header of the block whose payload starts at a given address
pub fn header_of(payload: *const u8) -> Header
{
    unsafe { *(payload as *const Header).sub(1) }
}

/// Number of elements a table block holds, read from its own header.
///
/// This is what lets an array or dict point at its table with a plain
/// pointer: the capacity is already recorded in the block it points at,
/// so there is nothing to carry alongside the pointer.
/// View a table block as a slice, taking its length from its own header
pub fn table_slice<'a, T>(p: *const T) -> &'a [T]
{
    unsafe { std::slice::from_raw_parts(p, table_len(p)) }
}

pub fn table_slice_mut<'a, T>(p: *mut T) -> &'a mut [T]
{
    unsafe { std::slice::from_raw_parts_mut(p, table_len(p)) }
}

pub fn table_len<T>(p: *const T) -> usize
{
    debug_assert!(!p.is_null());
    header_of(p as *const u8).num_bytes() / size_of::<T>()
}

/// Change the length recorded in the header of a fixed-layout block.
/// This is how an array or dict updates its length, since the header is
/// where that lives.
pub fn set_fixed_len(payload: *mut u8, len: usize)
{
    assert!(len <= MAX_FIXED_LEN, "length too large to fit in a header");

    unsafe {
        let p = (payload as *mut Header).sub(1);
        let bits = (*p).0;
        debug_assert!(bits & 1 == 1, "length written to a forwarded block");
        *p = Header((bits & !(LEN_BITS << LEN_SHIFT)) | ((len as u64) << LEN_SHIFT));
    }
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
pub fn align_up(size: usize) -> usize
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

    /// Carve a block of a given payload size out of the heap and put a
    /// header in front of it.
    ///
    /// Sizes are rounded up to the alignment so that blocks sit back to
    /// back with no gaps between them. That is what lets the collector
    /// walk the heap linearly, one header at a time. The header is built
    /// by the caller, because what goes in it depends on the tag.
    fn bump(&mut self, size_bytes: usize, hdr: Header) -> *mut u8
    {
        debug_assert!(size_bytes % ALIGN == 0);
        debug_assert!(hdr.size() == size_bytes);

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
            std::ptr::write(p.sub(HEADER_SIZE) as *mut Header, hdr);
            p
        }
    }

    /// Allocate a raw block: a string's inline bytes, or a table. The
    /// four spare header bits are the tag's to use.
    pub fn alloc_raw(&mut self, tag: Tag, num_bytes: usize) -> *mut u8
    {
        self.bump(align_up(num_bytes), Header::new_raw(tag, num_bytes))
    }

    /// Allocate a block of slots, tagged with the class or function id
    /// the slots belong to
    pub fn alloc_slots(&mut self, tag: Tag, aux24: usize, num_slots: usize) -> *mut Value
    {
        let size_bytes = num_slots * size_of::<Value>();
        let p = self.bump(size_bytes, Header::new_slots(tag, aux24, num_slots));
        p as *mut Value
    }

    /// Allocate an eight-byte block whose header carries a length.
    ///
    /// The payload is cleared, because a container is allocated before
    /// the table it points at and the bytes here are whatever the last
    /// heap left behind. Anything walking the heap in between would
    /// otherwise find a stale pointer to follow.
    pub fn alloc_fixed(&mut self, tag: Tag, len: usize) -> *mut u8
    {
        let p = self.bump(FIXED_SIZE, Header::new_fixed(tag, len));
        unsafe { std::ptr::write(p as *mut u64, 0) };
        p
    }

    /// Allocate room for a block the collector is copying, keeping the
    /// header it already has. Everything the header records beyond the
    /// size survives the copy this way, with nothing to rebuild.
    pub fn alloc_copy(&mut self, hdr: Header) -> *mut u8
    {
        self.bump(hdr.size(), hdr)
    }

    /// Allocate a variable-sized table of elements of a given type.
    /// The element count is recovered from the block header with
    /// table_len, so it is not carried alongside the pointer.
    pub fn alloc_table<T>(&mut self, num_elems: usize, tag: Tag) -> *mut T
    {
        debug_assert!(align_of::<T>() <= ALIGN);

        let num_bytes = num_elems * size_of::<T>();
        self.alloc_raw(tag, num_bytes) as *mut T
    }

    /// Allocate an eight-byte block holding a single value of a given
    /// type, such as a captured variable or a boxed number
    pub fn alloc_boxed<T>(&mut self, val: T, tag: Tag) -> *mut T
    {
        debug_assert!(size_of::<T>() <= FIXED_SIZE && align_of::<T>() <= ALIGN);

        // Straight to the bump: the value below covers the whole payload,
        // so there is nothing for alloc_fixed's clearing to do
        let p = self.bump(FIXED_SIZE, Header::new_fixed(tag, 0)) as *mut T;

        // Write the value without dropping what is currently there
        unsafe { std::ptr::write(p, val) };

        p
    }

    /// Box an integer that is too large to be a fixnum
    pub fn heap_int64(&mut self, val: i64) -> Value
    {
        debug_assert!(!Value::fits_fixnum(val));
        Value::int64_box(self.alloc_boxed(val, Tag::Int64))
    }

    /// Box a double that has no inline flonum encoding
    pub fn heap_float64(&mut self, val: f64) -> Value
    {
        debug_assert!(Value::try_flonum(val).is_none());
        Value::float64_box(self.alloc_boxed(val, Tag::Float64))
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
        let p = alloc.alloc_boxed::<u64>(1337, Tag::Int64);

        // An allocator can always grow into its own reservation
        let reserve_size = alloc.reserve_size();
        assert!(reserve_size >= 2 * 1024 * 1024);
        alloc.grow(reserve_size);

        assert!(unsafe { *p } == 1337);
        assert!(alloc.mem_size() == reserve_size);

        let table = alloc.alloc_table::<u64>(4096, Tag::Bytes);
        assert!(table_slice_mut(table).iter().all(|&x| x == 0));
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
        assert!(table_slice_mut(table).iter().all(|&x| x == 0));
    }

    /// Growing the reservation moves the memory block, so it must not be
    /// allowed while the allocator still holds objects
    #[test]
    #[should_panic(expected = "holding objects")]
    fn grow_reserve_rejects_live_objects()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);
        alloc.alloc_boxed::<u64>(1337, Tag::Int64);
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
        table_slice_mut(table).fill(0xABAB_ABAB_ABAB_ABAB);
        alloc.reset();

        // Release every page, so none of the old contents can come back
        alloc.shrink_to(0);
        assert!(alloc.mem_size() == 0);

        alloc.grow(8 * 1024 * 1024);
        let table = alloc.alloc_table::<u64>(256 * 1024, Tag::Bytes);
        assert!(table_slice_mut(table).iter().all(|&x| x == 0));
    }

    /// Resetting leaves the old bytes in place, and zero_up_to clears
    /// whatever was not allocated over in the meantime
    #[test]
    fn zero_up_to_clears_what_was_not_overwritten()
    {
        let mut alloc = Alloc::with_size(1024 * 1024);

        let table = alloc.alloc_table::<u64>(1024, Tag::Bytes);
        table_slice_mut(table).fill(0xABAB_ABAB_ABAB_ABAB);
        let dirty_bytes = alloc.bytes_used();
        alloc.reset();

        // Allocate over the first half, the way a copy would
        let kept = alloc.alloc_table::<u64>(512, Tag::Bytes);
        table_slice_mut(kept).fill(1337);

        alloc.zero_up_to(dirty_bytes);

        // What we wrote is untouched, and the tail is back to zero
        assert!(table_slice_mut(kept).iter().all(|&x| x == 1337));
        let rest = alloc.alloc_table::<u64>(510, Tag::Bytes);
        assert!(table_slice_mut(rest).iter().all(|&x| x == 0));
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
            src.alloc_raw(Tag::Bytes, s);
        }

        // Copy the same allocations back in a different order
        let mut dst = Alloc::with_size(INIT_SIZE);
        dst.grow_reserve(2 * src.bytes_used());
        dst.grow(2 * src.bytes_used());

        let mut reordered = sizes.clone();
        reordered.sort();
        for &s in &reordered {
            dst.alloc_raw(Tag::Bytes, s);
        }

        assert!(dst.bytes_used() == src.bytes_used());
    }

    /// One block of each of the three header layouts, and the size each
    /// one should walk over
    fn sample_blocks() -> Vec<(Tag, usize)>
    {
        vec![
            (Tag::Bytes, 8),        // raw, 3 bytes rounded up
            (Tag::Str, 24),         // raw, 17 bytes rounded up
            (Tag::ValueTable, 8),   // raw
            (Tag::Object, 24),      // slots, 3 of them
            (Tag::Closure, 8),      // slots, 1 of them
            (Tag::Array, 8),        // fixed
            (Tag::Dict, 8),         // fixed
            (Tag::Int64, 8),        // fixed
        ]
    }

    fn alloc_sample(alloc: &mut Alloc, tag: Tag)
    {
        match tag {
            Tag::Bytes => { alloc.alloc_raw(tag, 3); }
            Tag::Str => { alloc.alloc_raw(tag, 17); }
            Tag::ValueTable => { alloc.alloc_raw(tag, 8); }
            Tag::Object => { alloc.alloc_slots(tag, 1337, 3); }
            Tag::Closure => { alloc.alloc_slots(tag, 42, 1); }
            _ => { alloc.alloc_fixed(tag, 7); }
        }
    }

    /// Blocks sit back to back, so a linear walk must land on every one
    /// of them and recover the tag and size it was allocated with. Each
    /// of the three header layouts has to walk correctly.
    #[test]
    fn heap_walk_visits_every_block()
    {
        let sample = sample_blocks();
        let blocks: Vec<(Tag, usize)> =
            (0..1000).map(|i| sample[i % sample.len()]).collect();

        let mut alloc = Alloc::with_size(INIT_SIZE);
        for &(tag, _) in &blocks {
            alloc_sample(&mut alloc, tag);
        }

        let mut offset = 0;
        let mut visited = 0;

        while offset < alloc.bytes_used() {
            let p = alloc.block_at(offset);
            let hdr = header_of(p);
            let (tag, size) = blocks[visited];

            assert!(alloc.contains(p));
            assert!(!hdr.is_forwarded());
            assert!(hdr.tag() == tag);
            assert!(hdr.size() == size, "{:?} walked {} bytes", tag, hdr.size());

            offset += HEADER_SIZE + hdr.size();
            visited += 1;
        }

        assert!(visited == blocks.len());
    }

    /// Every layout has to read back the fields it was built with
    #[test]
    fn header_fields_round_trip()
    {
        let mut alloc = Alloc::with_size(INIT_SIZE);

        // A string keeps an exact byte length across the rounding
        let p = alloc.alloc_raw(Tag::Str, 17);
        let hdr = header_of(p);
        assert!(hdr.num_bytes() == 17 && hdr.size() == 24);

        // An object keeps its class and slot count, and every object of
        // a class has the same guard key whatever its slot count
        let p = alloc.alloc_slots(Tag::Object, 1337, 3) as *mut u8;
        let hdr = header_of(p);
        assert!(hdr.aux24() == 1337 && hdr.num_slots() == 3);
        assert!(hdr.guard_key() == Header::object_key(1337));
        assert!(hdr.guard_key() != Header::object_key(1338));

        // A fixed block keeps a length that can be written again
        let p = alloc.alloc_fixed(Tag::Array, 5);
        assert!(header_of(p).fixed_len() == 5 && header_of(p).size() == FIXED_SIZE);
        set_fixed_len(p, MAX_FIXED_LEN);
        assert!(header_of(p).fixed_len() == MAX_FIXED_LEN);
        assert!(header_of(p).tag() == Tag::Array && header_of(p).size() == FIXED_SIZE);
    }

    /// A forwarded header reads back as the address the block moved to,
    /// and can be put back the way it was
    #[test]
    fn forwarding_round_trips()
    {
        let mut alloc = Alloc::with_size(INIT_SIZE);
        let p = alloc.alloc_slots(Tag::Object, 7, 3) as *mut u8;
        let q = alloc.alloc_slots(Tag::Object, 7, 3) as *mut u8;

        let hdr = header_of(p);
        assert!(!hdr.is_forwarded());

        set_forwarded(p, q);
        assert!(header_of(p).is_forwarded());
        assert!(header_of(p).forward_addr() == q);

        restore_header(p, hdr);
        assert!(!header_of(p).is_forwarded());
        assert!(header_of(p).tag() == Tag::Object);
        assert!(header_of(p).size() == 24);
        assert!(header_of(p).aux24() == 7);
    }

    /// Tables start out zeroed and the collector scans every slot of the
    /// ones it walks, so zeroed memory has to read back as a value that
    /// holds no reference
    #[test]
    fn zeroed_memory_holds_no_reference()
    {
        let mut alloc = Alloc::with_size(INIT_SIZE);
        let table = alloc.alloc_table::<Value>(64, Tag::ValueTable);
        assert!(table_len(table) == 64);

        for val in table_slice_mut(table) {
            assert!(!val.is_heap());
            assert!(*val == Value::fixnum(0));
        }
    }
}
