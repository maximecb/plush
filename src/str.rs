#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Str(*const str);

impl Str {
    pub fn new(s: *const str) -> Self {
        Str(s)
    }

    pub fn as_str(&self) -> &str {
        unsafe { &*self.0 }
    }
}
