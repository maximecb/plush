use std::mem::size_of;
use crate::alloc::{Alloc, Tag, HEADER_SIZE};
use crate::value::Value;

/// Immutable string. The utf-8 bytes are stored inline, right after it.
#[repr(C, align(8))]
pub struct Str
{
    len: usize,
}

impl Str {
    /// Allocate a string, copying the utf-8 bytes into the tail of the
    /// allocation, right after the string itself
    pub fn new(s: &str, alloc: &mut Alloc) -> Value {
        let p = alloc.alloc_var(Str { len: s.len() }, s.len(), Tag::Str);

        unsafe {
            let bytes = (p as *mut u8).add(size_of::<Str>());
            std::ptr::copy_nonoverlapping(s.as_ptr(), bytes, s.len());
        }

        Value::string(p)
    }

    /// Bytes a string of a given length occupies, header included
    pub fn alloc_size(len: usize) -> usize {
        HEADER_SIZE + size_of::<Str>() + len
    }

    pub fn as_str(&self) -> &str {
        unsafe {
            let bytes = (self as *const Str).add(1) as *const u8;
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(bytes, self.len))
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}
