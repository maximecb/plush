use std::mem::size_of;
use crate::alloc::HEADER_SIZE;

/// Immutable string. The utf-8 bytes are stored inline, right after it.
#[repr(C, align(8))]
pub struct Str
{
    len: usize,
}

impl Str {
    pub fn new(len: usize) -> Self {
        Str { len }
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
