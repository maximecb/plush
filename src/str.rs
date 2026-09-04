use crate::alloc::{align_up, header_of, Alloc, Tag, HEADER_SIZE};
use crate::value::Value;

/// Immutable string. Nothing is stored here: the utf-8 bytes start at
/// the address of the string itself, and the length comes from the
/// block header.
#[repr(C, align(8))]
pub struct Str;

impl Str {
    /// Allocate a string, copying the utf-8 bytes into the block
    pub fn new(s: &str, alloc: &mut Alloc) -> Value {
        let p = alloc.alloc_raw(Tag::Str, s.len());

        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
        }

        Value::string(p as *const Str)
    }

    /// Bytes a string of a given length occupies, header included
    pub fn alloc_size(len: usize) -> usize {
        HEADER_SIZE + align_up(len)
    }

    fn bytes(&self) -> *const u8 {
        self as *const Str as *const u8
    }

    pub fn as_str(&self) -> &str {
        unsafe {
            std::str::from_utf8_unchecked(
                std::slice::from_raw_parts(self.bytes(), self.len())
            )
        }
    }

    pub fn len(&self) -> usize {
        header_of(self.bytes()).num_bytes()
    }
}
