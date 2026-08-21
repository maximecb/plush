use rustc_hash::FxHashSet;
use std::hash::{Hash, Hasher};
use std::mem::size_of;

use crate::alloc::{header_of, restore_header, set_forwarded};
use crate::alloc::{Alloc, Header, Tag, HEADER_SIZE};
use crate::array::Array;
use crate::bytearray::ByteArray;
use crate::closure::Closure;
use crate::dict::{Dict, TableSlot};
use crate::object::Object;
use crate::str::Str;
use crate::value::Value;

/// A string that has already been copied, keyed by its contents.
///
/// Forwarding pointers deduplicate by address, so without this two equal
/// strings in the source would end up with two copies. The copier keeps
/// a table of these for the duration of a copy so that equal strings
/// still share a single allocation.
#[derive(Clone, Copy)]
pub struct StrKey(*const Str);

// The copier is the only thing that touches these, and it runs on the
// actor thread that owns the heap
unsafe impl Send for StrKey {}

impl Hash for StrKey
{
    fn hash<H: Hasher>(&self, state: &mut H)
    {
        unsafe { (*self.0).as_str() }.hash(state);
    }
}

impl PartialEq for StrKey
{
    fn eq(&self, other: &Self) -> bool
    {
        unsafe { (*self.0).as_str() == (*other.0).as_str() }
    }
}

impl Eq for StrKey {}

pub type StrTable = FxHashSet<StrKey>;

/// A header that was overwritten with a forwarding address
pub struct UndoEntry
{
    block: *mut u8,
    header: Header,
}

unsafe impl Send for UndoEntry {}

pub type UndoLog = Vec<UndoEntry>;

/// Cheney-style copying collector.
///
/// The destination allocator doubles as the work queue: everything
/// between the scan pointer and the allocation pointer has been copied
/// but has not yet had its own references updated. Scanning that region
/// appends more blocks to the end, and the copy is done once the scan
/// pointer catches up with them.
///
/// Walking that region block by block is what the size and tag in each
/// block header are there for.
pub struct Copier<'a>
{
    // Allocator to copy into
    dst: &'a mut Alloc,

    // Offset of the next block to scan in the destination
    scan: usize,

    // Strings copied so far, so that equal strings can share
    strs: &'a mut StrTable,

    // Where to record overwritten headers, when the source heap has to
    // stay usable after the copy
    undo: Option<&'a mut UndoLog>,

    // Number of blocks copied, for reporting
    num_blocks: usize,
}

impl<'a> Copier<'a>
{
    /// Copier that leaves forwarding addresses behind in the source.
    /// Only valid when the source heap is about to be discarded.
    pub fn new(dst: &'a mut Alloc, strs: &'a mut StrTable) -> Self
    {
        strs.clear();

        Self {
            scan: dst.bytes_used(),
            dst,
            strs,
            undo: None,
            num_blocks: 0,
        }
    }

    /// Copier that records the headers it overwrites, so that they can
    /// be put back with undo_forwarding once the copy is done
    pub fn with_undo(
        dst: &'a mut Alloc,
        strs: &'a mut StrTable,
        undo: &'a mut UndoLog,
    ) -> Self
    {
        undo.clear();

        let mut copier = Self::new(dst, strs);
        copier.undo = Some(undo);
        copier
    }

    #[allow(dead_code)] // used by the log_gc cycle report
    pub fn num_blocks(&self) -> usize
    {
        self.num_blocks
    }

    /// Copy the value's referent into the destination and return the
    /// value with its pointer updated. A block that was already copied
    /// is not copied again: its old header holds the address of the copy.
    ///
    /// The value only says that it points at a block, so what kind of
    /// block it is comes from the header. Only strings need to be told
    /// apart here, because they are the one thing the copy deduplicates.
    pub fn forward(&mut self, val: Value) -> Value
    {
        if !val.is_heap() {
            return val;
        }

        let p = val.heap_ptr();
        let hdr = header_of(p);

        let new_p = if hdr.is_forwarded() {
            hdr.forward_addr()
        } else if hdr.tag() == Tag::Str {
            self.forward_str(p as *const Str) as *mut u8
        } else {
            self.copy(p)
        };

        val.with_heap_ptr(new_p)
    }

    /// Copy a string, sharing one that was already copied if it is equal
    fn forward_str(&mut self, p: *const Str) -> *const Str
    {
        let hdr = header_of(p as *const u8);

        if hdr.is_forwarded() {
            return hdr.forward_addr() as *const Str;
        }

        if let Some(shared) = self.strs.get(&StrKey(p)).map(|k| k.0) {
            self.record_undo(p as *mut u8, hdr);
            set_forwarded(p as *mut u8, shared as *mut u8);
            return shared;
        }

        let new_p = self.copy(p as *mut u8) as *const Str;
        self.strs.insert(StrKey(new_p));
        new_p
    }

    /// Copy a block into the destination, leaving a forwarding address
    /// behind in the header of the original
    fn copy(&mut self, p: *mut u8) -> *mut u8
    {
        let hdr = header_of(p);

        if hdr.is_forwarded() {
            return hdr.forward_addr();
        }

        let size = hdr.size();
        let new_p = self.dst.alloc_bytes(size, hdr.tag());
        unsafe { std::ptr::copy_nonoverlapping(p, new_p, size) };

        self.record_undo(p, hdr);
        set_forwarded(p, new_p);

        self.num_blocks += 1;
        new_p
    }

    /// Relocate a table, keeping its length
    fn copy_table<T>(&mut self, table: *mut [T]) -> *mut [T]
    {
        let len = table.len();
        let new_p = self.copy(table as *mut T as *mut u8);

        std::ptr::slice_from_raw_parts_mut(new_p as *mut T, len)
    }

    /// Relocate a table, dropping the capacity past its live length.
    ///
    /// A table has exactly one owner, so there can be no other reference
    /// to the block left behind for the shorter copy to invalidate.
    fn copy_table_prefix<T>(&mut self, table: *mut [T], len: usize, tag: Tag) -> *mut [T]
    {
        debug_assert!(len <= table.len());

        let p = table as *mut T as *mut u8;
        let hdr = header_of(p);
        debug_assert!(!hdr.is_forwarded(), "table reachable from two owners");

        let new_table = self.dst.alloc_table::<T>(len, tag);
        unsafe { std::ptr::copy_nonoverlapping(
            p as *const T,
            new_table as *mut T,
            len
        )};

        self.record_undo(p, hdr);
        set_forwarded(p, new_table as *mut T as *mut u8);

        self.num_blocks += 1;
        new_table
    }

    fn record_undo(&mut self, p: *mut u8, hdr: Header)
    {
        if let Some(undo) = self.undo.as_mut() {
            undo.push(UndoEntry { block: p, header: hdr });
        }
    }

    /// Copy everything reachable from the values forwarded so far
    pub fn run(&mut self)
    {
        while self.scan < self.dst.bytes_used() {
            let p = self.dst.block_at(self.scan);
            let hdr = header_of(p);
            self.scan += HEADER_SIZE + hdr.size();

            self.scan_block(hdr, p);
        }
    }

    /// Update the references held by a block that has been copied
    fn scan_block(&mut self, hdr: Header, p: *mut u8)
    {
        match hdr.tag() {
            // Strings and raw bytes hold no references
            Tag::Str | Tag::Bytes | Tag::Int64 | Tag::Float64 => {}

            Tag::Object => {
                let obj = unsafe { &mut *(p as *mut Object) };

                for i in 0..obj.num_slots() {
                    let val = self.forward(obj.get(i));
                    obj.set(i, val);
                }
            }

            Tag::Closure => {
                let clos = unsafe { &mut *(p as *mut Closure) };

                for i in 0..clos.num_slots() {
                    let val = self.forward(clos.get(i));
                    clos.set(i, val);
                }
            }

            Tag::Cell => {
                let cell = unsafe { &mut *(p as *mut Value) };
                *cell = self.forward(*cell);
            }

            // Arrays and bytearrays hold spare capacity, which the copy
            // drops. A dict keeps its capacity: its table has to stay
            // bigger than its entry count for lookups to terminate.
            Tag::Array => {
                let arr = unsafe { &mut *(p as *mut Array) };

                // Keep room to grow into, so that pushing again does not
                // immediately have to reallocate the table. Doubling
                // never leaves more than this, so a growing array keeps
                // its capacity and only one shrunk by pop or remove
                // gives any of it back.
                let capacity = (2 * arr.len()).min(arr.capacity());
                let elems = self.copy_table_prefix(arr.elems, capacity, Tag::ValueTable);
                arr.elems = elems;
            }

            Tag::ByteArray => {
                let ba = unsafe { &mut *(p as *mut ByteArray) };
                let num_bytes = ba.num_bytes();
                let bytes = self.copy_table_prefix(ba.bytes, num_bytes, Tag::Bytes);
                ba.bytes = bytes;
            }

            Tag::Dict => {
                let dict = unsafe { &mut *(p as *mut Dict) };
                let table = self.copy_table(dict.table);
                dict.table = table;
            }

            Tag::ValueTable => {
                let vals = p as *mut Value;

                for i in 0..hdr.size() / size_of::<Value>() {
                    let val = self.forward(unsafe { *vals.add(i) });
                    unsafe { *vals.add(i) = val };
                }
            }

            Tag::SlotTable => {
                let slots = p as *mut TableSlot;

                for i in 0..hdr.size() / size_of::<TableSlot>() {
                    let slot = unsafe { &mut *slots.add(i) };

                    // Empty slots have a null key and no value
                    if slot.key.is_null() {
                        continue;
                    }

                    let key = self.forward_str(slot.key);
                    let val = self.forward(slot.val);
                    slot.key = key;
                    slot.val = val;
                }
            }
        }
    }
}

/// Put back the headers a copier overwrote with forwarding addresses.
/// Needed when copying out of a heap that has to survive the copy.
pub fn undo_forwarding(undo: &mut UndoLog)
{
    for entry in undo.drain(..) {
        restore_header(entry.block, entry.header);
    }
}

/// Walk a heap and check that every block is well formed and every
/// reference points at a live block of the expected kind in the same
/// heap. This is a full heap walk, so it is only meant for testing.
#[cfg(feature = "verify_gc")]
pub fn verify_heap(alloc: &Alloc)
{
    fn check(alloc: &Alloc, p: *const u8, expected: Tag)
    {
        assert!(alloc.contains(p), "reference {:p} outside the heap", p);
        assert!(p as usize % 8 == 0, "misaligned reference {:p}", p);

        let hdr = header_of(p);
        assert!(!hdr.is_forwarded(), "reference {:p} to a forwarded block", p);
        assert!(
            hdr.tag() == expected,
            "reference {:p} has tag {:?}, expected {:?}",
            p, hdr.tag(), expected
        );
    }

    fn check_val(alloc: &Alloc, val: Value)
    {
        if !val.is_heap() {
            // Anything that is not a pointer still has to name a type
            val.type_of();
            return;
        }

        let p = val.heap_ptr();
        assert!(alloc.contains(p), "reference {:p} outside the heap", p);
        assert!(p as usize % 8 == 0, "misaligned reference {:p}", p);

        let hdr = header_of(p);
        assert!(!hdr.is_forwarded(), "reference {:p} to a forwarded block", p);

        // The pointer tag says how the value compares and the header says
        // what it points at, so the two have to agree
        let agrees = match hdr.tag() {
            Tag::Str => val.is_string(),
            Tag::Object => val.is_object(),
            Tag::Closure => val.is_closure(),
            Tag::Cell => val.is_cell(),
            Tag::Array => val.is_array(),
            Tag::ByteArray => val.is_bytearray(),
            Tag::Dict => val.is_dict(),
            Tag::Int64 => val.is_int64_box(),
            Tag::Float64 => val.is_float64_box(),
            _ => false,
        };

        assert!(agrees, "value points at a {:?} block", hdr.tag());
    }

    let mut offset = 0;

    while offset < alloc.bytes_used() {
        let p = alloc.block_at(offset);
        let hdr = header_of(p);
        assert!(!hdr.is_forwarded(), "forwarded block left in the heap");
        offset += HEADER_SIZE + hdr.size();

        match hdr.tag() {
            Tag::Str | Tag::Bytes | Tag::Int64 | Tag::Float64 => {}

            Tag::Object => {
                let obj = unsafe { &*(p as *const Object) };
                for i in 0..obj.num_slots() {
                    check_val(alloc, obj.get(i));
                }
            }

            Tag::Closure => {
                let clos = unsafe { &*(p as *const Closure) };
                for i in 0..clos.num_slots() {
                    check_val(alloc, clos.get(i));
                }
            }

            Tag::Cell => check_val(alloc, unsafe { *(p as *const Value) }),

            Tag::Array => {
                let arr = unsafe { &*(p as *const Array) };
                check(alloc, arr.elems as *const u8, Tag::ValueTable);
            }

            Tag::ByteArray => {
                let ba = unsafe { &*(p as *const ByteArray) };
                check(alloc, ba.bytes as *const u8, Tag::Bytes);
            }

            Tag::Dict => {
                let dict = unsafe { &*(p as *const Dict) };
                check(alloc, dict.table as *const u8, Tag::SlotTable);
            }

            Tag::ValueTable => {
                let vals = p as *const Value;
                for i in 0..hdr.size() / size_of::<Value>() {
                    check_val(alloc, unsafe { *vals.add(i) });
                }
            }

            Tag::SlotTable => {
                let slots = p as *const TableSlot;
                for i in 0..hdr.size() / size_of::<TableSlot>() {
                    let slot = unsafe { &*slots.add(i) };
                    if slot.key.is_null() {
                        continue;
                    }
                    check(alloc, slot.key as *const u8, Tag::Str);
                    check_val(alloc, slot.val);
                }
            }
        }
    }
}
