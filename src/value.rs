//! Tagged value representation.
//!
//! A `Value` is a single 64-bit word. The low bits carry the tag:
//!
//! ```text
//!   bits 2..0   class
//!   x00         fixnum, 62-bit signed integer, stored as n << 2
//!   001         pointer compared by identity: Object Array ByteArray Dict Closure Cell
//!   011         pointer compared by value:    Str Int64 Float64
//!   101         immediate: nil true false undef Fun Class HostFn
//!   x10         flonum (see below)
//!   111         reserved
//! ```
//!
//! Bit 1 is the *compare by value* bit. It is set only for the types whose
//! equality is not bitwise: flonums (+0.0 vs -0.0, NaN), strings (structural)
//! and boxed numbers (int/float cross-comparison). So `eq` is a mask test
//! plus a word compare, and every other combination falls out correct.
//!
//! Pointers only record *that* they point at a heap block. The kind comes
//! from the block header, in the word right before the payload, so the two
//! pointer tags exist solely to classify equality.
//!
//! Immediates hold a 5-bit subtag in bits 7..3, which makes the whole low
//! byte a per-type constant, and a 56-bit payload in bits 63..8.
//!
//! # Fixnums
//!
//! Tag 0 in bits 1..0 means `add`, `sub`, comparisons and bitwise and/or/xor
//! work directly on the tagged words, and a 64-bit overflow on `add`/`sub`
//! is exactly the case where the result no longer fits in 62 bits.
//!
//! # Flonums
//!
//! Doubles are self-tagged with the additive scheme from arXiv:2411.16544:
//! https://arxiv.org/html/2411.16544v3
//!
//! `rotl(bits + BIAS, 4)`, kept inline when the rotation happens to land the
//! flonum tag in the low bits, boxed otherwise. Nothing is destroyed, so the
//! mapping is exact and decoding is a rotate and a subtract.
//!
//! Rotating by 4 constrains exponent bits 9..8, which covers two bands of 256
//! exponents, 1024 apart. Since 1023 + 1024 == 2047, centering one band on
//! 1.0 puts the other one across the zero/infinity wrap, which fixes BIAS.
//! Covered: +-0.0, all subnormals, |x| in [2^-128, 2^128), |x| >= 2^896,
//! +-inf and all NaNs. Only |x| in [2^-896, 2^-128) and [2^128, 2^896) is
//! boxed, so every special value and every normal float32 stays inline.
//! Only float32 subnormals below 2^-128 box.
//!
//! # Invariants
//!
//! Representations are canonical, which is what makes the `eq` fast path
//! sound:
//! - an Int64 box never holds a value that fits in a fixnum
//! - a Float64 box never holds a double that fits in a flonum
//! - unused immediate payload bits are zero
//!
//! # Assumptions
//!
//! - Heap blocks are 8-byte aligned (`alloc::ALIGN`), leaving 3 low bits.
//! - Host functions are held as a table index, not a pointer, so that the
//!   whole value fits the immediate range an instruction can carry.
//! - A heap pointer is never null, so no tagged word collides with an
//!   immediate or with `nil`.
//! - Accessors hand out references into the heap. The collector moves
//!   blocks, so those are only valid until the next allocation.

use std::fmt;
use crate::alloc::{header_of, Tag};
use crate::array::Array;
use crate::ast::{ClassId, FunId};
use crate::bytearray::ByteArray;
use crate::closure::Closure;
use crate::dict::Dict;
use crate::host::{HostFn, HostFnId, NUM_HOST_FNS};
use std::mem::transmute;
use crate::object::Object;
use crate::str::Str;

const TAG_MASK: u64 = 0b111;

const TAG_PTR_ID: u64 = 0b001;
const TAG_PTR_VAL: u64 = 0b011;
const TAG_IMM: u64 = 0b101;

/// Fixnums and flonums leave bit 2 free, so they are tested on bits 1..0
const NUM_MASK: u64 = 0b011;
const TAG_FIXNUM: u64 = 0b000;
const TAG_FLONUM: u64 = 0b010;

/// `v & HEAP_MASK == TAG_PTR_ID` holds for both pointer tags
const HEAP_MASK: u64 = 0b101;

/// Set on the types whose equality needs more than a word compare
const VAL_BIT: u64 = 0b010;

/// Immediates: 5-bit subtag in bits 7..3, payload in bits 63..8.
/// True and False are adjacent subtags so that they differ in bit 3 only.
const IMM_SHIFT: u32 = 8;
const IMM_NIL: u64 = 0x05;
const IMM_UNDEF: u64 = 0x0D;
const IMM_FALSE: u64 = 0x15;
const IMM_TRUE: u64 = 0x1D;
const IMM_FUN: u64 = 0x25;
const IMM_CLASS: u64 = 0x2D;
const IMM_HOSTFN: u64 = 0x35;
const BOOL_BIT: u64 = 0x08;

/// Flonum encoding constants, see the module docs
const FLONUM_BIAS: u64 = 0x6810_0000_0000_0000;
const FLONUM_ROT: u32 = 4;

/// Language-level type of a value, for cold paths that need to name it.
/// Fixnums and Int64 boxes are both `Int64`, flonums and Float64 boxes are
/// both `Float64`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Type
{
    Undef,
    Nil,
    Bool,
    Int64,
    Float64,
    String,
    Array,
    ByteArray,
    Dict,
    Object,
    Closure,
    Cell,
    Fun,
    Class,
    HostFn,
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Value(u64);

impl Value
{
    pub const NIL: Value = Value(IMM_NIL);
    pub const TRUE: Value = Value(IMM_TRUE);
    pub const FALSE: Value = Value(IMM_FALSE);
    pub const UNDEF: Value = Value(IMM_UNDEF);

    /// The fixnum tag is zero, so this is the all-zero word. That makes it
    /// the cheapest value to store: nil and friends have to be materialized
    /// into a register first, where zero is already there
    pub const FIXNUM_ZERO: Value = Value(TAG_FIXNUM);

    /// Largest and smallest integers representable without boxing
    pub const FIXNUM_MAX: i64 = (1 << 61) - 1;
    pub const FIXNUM_MIN: i64 = -(1 << 61);

    #[inline(always)]
    pub const fn from_raw_bits(bits: u64) -> Value
    {
        Value(bits)
    }

    #[inline(always)]
    pub const fn raw_bits(self) -> u64
    {
        self.0
    }

    #[inline(always)]
    fn tag(self) -> u64
    {
        self.0 & TAG_MASK
    }

    // Fixnums

    #[inline(always)]
    pub const fn fits_fixnum(val: i64) -> bool
    {
        val >= Value::FIXNUM_MIN && val <= Value::FIXNUM_MAX
    }

    #[inline(always)]
    pub fn fixnum(val: i64) -> Value
    {
        debug_assert!(Value::fits_fixnum(val));
        Value((val as u64) << 2)
    }

    #[inline(always)]
    pub fn try_fixnum(val: i64) -> Option<Value>
    {
        if Value::fits_fixnum(val) { Some(Value::fixnum(val)) } else { None }
    }

    #[inline(always)]
    pub fn is_fixnum(self) -> bool
    {
        self.0 & NUM_MASK == TAG_FIXNUM
    }

    #[inline(always)]
    pub fn as_fixnum(self) -> i64
    {
        debug_assert!(self.is_fixnum());
        (self.0 as i64) >> 2
    }

    // Flonums

    /// Encode a double inline, or `None` if it falls outside the covered set
    #[inline(always)]
    pub fn try_flonum(val: f64) -> Option<Value>
    {
        let bits = val.to_bits().wrapping_add(FLONUM_BIAS).rotate_left(FLONUM_ROT);
        if bits & NUM_MASK == TAG_FLONUM { Some(Value(bits)) } else { None }
    }

    #[inline(always)]
    pub fn is_flonum(self) -> bool
    {
        self.0 & NUM_MASK == TAG_FLONUM
    }

    #[inline(always)]
    pub fn as_flonum(self) -> f64
    {
        debug_assert!(self.is_flonum());
        f64::from_bits(self.0.rotate_right(FLONUM_ROT).wrapping_sub(FLONUM_BIAS))
    }

    // Immediates

    #[inline(always)]
    pub fn bool_val(b: bool) -> Value
    {
        if b { Value::TRUE } else { Value::FALSE }
    }

    #[inline(always)]
    pub fn is_nil(self) -> bool { self.0 == IMM_NIL }

    #[inline(always)]
    pub fn is_true(self) -> bool { self.0 == IMM_TRUE }

    #[inline(always)]
    pub fn is_false(self) -> bool { self.0 == IMM_FALSE }

    #[inline(always)]
    pub fn is_undef(self) -> bool { self.0 == IMM_UNDEF }

    #[inline(always)]
    pub fn is_bool(self) -> bool
    {
        // True and False differ only in bit 3
        self.0 & !BOOL_BIT == IMM_FALSE
    }

    #[inline(always)]
    pub fn as_bool(self) -> bool
    {
        debug_assert!(self.is_bool());
        self.0 == IMM_TRUE
    }

    #[inline(always)]
    pub fn to_bool(self) -> Option<bool>
    {
        if self.is_bool() { Some(self.as_bool()) } else { None }
    }

    #[inline(always)]
    pub fn fun(fun_id: FunId) -> Value
    {
        Value(((usize::from(fun_id) as u64) << IMM_SHIFT) | IMM_FUN)
    }

    #[inline(always)]
    pub fn is_fun(self) -> bool { self.0 as u8 as u64 == IMM_FUN }

    #[inline(always)]
    pub fn as_fun(self) -> FunId
    {
        debug_assert!(self.is_fun());
        FunId::from((self.0 >> IMM_SHIFT) as usize)
    }

    #[inline(always)]
    pub fn to_fun(self) -> Option<FunId>
    {
        if self.is_fun() { Some(self.as_fun()) } else { None }
    }

    #[inline(always)]
    pub fn class(class_id: ClassId) -> Value
    {
        Value(((usize::from(class_id) as u64) << IMM_SHIFT) | IMM_CLASS)
    }

    #[inline(always)]
    pub fn is_class(self) -> bool { self.0 as u8 as u64 == IMM_CLASS }

    #[inline(always)]
    pub fn as_class(self) -> ClassId
    {
        debug_assert!(self.is_class());
        ClassId::from((self.0 >> IMM_SHIFT) as usize)
    }

    #[inline(always)]
    #[allow(dead_code)]
    pub fn to_class(self) -> Option<ClassId>
    {
        if self.is_class() { Some(self.as_class()) } else { None }
    }

    /// Host functions are held by table index rather than by pointer, so
    /// that the whole value fits in the immediate range an instruction
    /// can carry
    #[inline(always)]
    pub fn host_fn(id: HostFnId) -> Value
    {
        Value(((id as u64) << IMM_SHIFT) | IMM_HOSTFN)
    }

    #[inline(always)]
    pub fn is_host_fn(self) -> bool { self.0 as u8 as u64 == IMM_HOSTFN }

    #[inline(always)]
    pub fn as_host_fn_id(self) -> HostFnId
    {
        debug_assert!(self.is_host_fn());
        let idx = (self.0 >> IMM_SHIFT) as u16;
        debug_assert!((idx as usize) < NUM_HOST_FNS);
        unsafe { transmute(idx) }
    }

    #[inline(always)]
    pub fn as_host_fn(self) -> &'static HostFn
    {
        self.as_host_fn_id().get()
    }

    #[inline(always)]
    pub fn to_host_fn(self) -> Option<&'static HostFn>
    {
        if self.is_host_fn() { Some(self.as_host_fn()) } else { None }
    }

    // Heap pointers

    #[inline(always)]
    pub fn is_heap(self) -> bool
    {
        self.0 & HEAP_MASK == TAG_PTR_ID
    }

    #[inline(always)]
    pub fn heap_ptr(self) -> *mut u8
    {
        debug_assert!(self.is_heap());
        (self.0 & !TAG_MASK) as *mut u8
    }

    /// Kind of block this value points at, read from the block header
    #[inline(always)]
    pub fn heap_tag(self) -> Tag
    {
        header_of(self.heap_ptr()).tag()
    }

    /// Retag a pointer that the collector moved
    #[inline(always)]
    pub fn with_heap_ptr(self, p: *mut u8) -> Value
    {
        debug_assert!(self.is_heap());
        debug_assert!(p as u64 & TAG_MASK == 0);
        Value(p as u64 | self.tag())
    }

    #[inline(always)]
    fn ptr_id(p: *const u8) -> Value
    {
        debug_assert!(!p.is_null() && p as u64 & TAG_MASK == 0);
        Value(p as u64 | TAG_PTR_ID)
    }

    #[inline(always)]
    fn ptr_val(p: *const u8) -> Value
    {
        debug_assert!(!p.is_null() && p as u64 & TAG_MASK == 0);
        Value(p as u64 | TAG_PTR_VAL)
    }

    #[inline(always)]
    fn is_ptr_id(self, tag: Tag) -> bool
    {
        self.tag() == TAG_PTR_ID && self.heap_tag() == tag
    }

    #[inline(always)]
    fn is_ptr_val(self, tag: Tag) -> bool
    {
        self.tag() == TAG_PTR_VAL && self.heap_tag() == tag
    }

    // Boxed numbers

    #[inline(always)]
    pub fn int64_box(p: *mut i64) -> Value { Value::ptr_val(p as *const u8) }

    #[inline(always)]
    pub fn is_int64_box(self) -> bool { self.is_ptr_val(Tag::Int64) }

    #[inline(always)]
    pub fn float64_box(p: *mut f64) -> Value { Value::ptr_val(p as *const u8) }

    #[inline(always)]
    pub fn is_float64_box(self) -> bool { self.is_ptr_val(Tag::Float64) }

    // Heap objects

    #[inline(always)]
    pub fn string(p: *const Str) -> Value { Value::ptr_val(p as *const u8) }

    #[inline(always)]
    pub fn is_string(self) -> bool { self.is_ptr_val(Tag::Str) }

    #[inline(always)]
    pub fn as_string<'a>(self) -> &'a Str
    {
        debug_assert!(self.is_string());
        unsafe { &*(self.heap_ptr() as *const Str) }
    }

    #[inline(always)]
    pub fn as_str<'a>(self) -> &'a str
    {
        self.as_string().as_str()
    }

    #[inline(always)]
    pub fn to_str<'a>(self) -> Option<&'a str>
    {
        if self.is_string() { Some(self.as_str()) } else { None }
    }

    #[inline(always)]
    pub fn object(p: *mut Object) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_object(self) -> bool { self.is_ptr_id(Tag::Object) }

    #[inline(always)]
    pub fn as_obj<'a>(self) -> &'a mut Object
    {
        debug_assert!(self.is_object());
        unsafe { &mut *(self.heap_ptr() as *mut Object) }
    }

    #[inline(always)]
    pub fn to_obj<'a>(self) -> Option<&'a mut Object>
    {
        if self.is_object() { Some(self.as_obj()) } else { None }
    }

    #[inline(always)]
    pub fn array(p: *mut Array) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_array(self) -> bool { self.is_ptr_id(Tag::Array) }

    #[inline(always)]
    pub fn as_arr<'a>(self) -> &'a mut Array
    {
        debug_assert!(self.is_array());
        unsafe { &mut *(self.heap_ptr() as *mut Array) }
    }

    #[inline(always)]
    pub fn to_arr<'a>(self) -> Option<&'a mut Array>
    {
        if self.is_array() { Some(self.as_arr()) } else { None }
    }

    #[inline(always)]
    pub fn bytearray(p: *mut ByteArray) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_bytearray(self) -> bool { self.is_ptr_id(Tag::ByteArray) }

    #[inline(always)]
    pub fn as_ba<'a>(self) -> &'a mut ByteArray
    {
        debug_assert!(self.is_bytearray());
        unsafe { &mut *(self.heap_ptr() as *mut ByteArray) }
    }

    #[inline(always)]
    pub fn to_ba<'a>(self) -> Option<&'a mut ByteArray>
    {
        if self.is_bytearray() { Some(self.as_ba()) } else { None }
    }

    #[inline(always)]
    pub fn dict(p: *mut Dict) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_dict(self) -> bool { self.is_ptr_id(Tag::Dict) }

    #[inline(always)]
    pub fn as_dict<'a>(self) -> &'a mut Dict
    {
        debug_assert!(self.is_dict());
        unsafe { &mut *(self.heap_ptr() as *mut Dict) }
    }

    #[inline(always)]
    pub fn to_dict<'a>(self) -> Option<&'a mut Dict>
    {
        if self.is_dict() { Some(self.as_dict()) } else { None }
    }

    #[inline(always)]
    pub fn closure(p: *mut Closure) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_closure(self) -> bool { self.is_ptr_id(Tag::Closure) }

    #[inline(always)]
    pub fn as_clos<'a>(self) -> &'a mut Closure
    {
        debug_assert!(self.is_closure());
        unsafe { &mut *(self.heap_ptr() as *mut Closure) }
    }

    #[inline(always)]
    pub fn to_clos<'a>(self) -> Option<&'a mut Closure>
    {
        if self.is_closure() { Some(self.as_clos()) } else { None }
    }

    /// Function a call to this value enters, whether it holds one
    /// directly or through a closure. Host functions are not among
    /// these: they are called through `to_host_fn` instead.
    #[inline(always)]
    pub fn to_fun_id(self) -> Option<FunId>
    {
        match self.to_clos() {
            Some(clos) => Some(clos.fun_id),
            None => self.to_fun(),
        }
    }

    #[inline(always)]
    pub fn cell(p: *mut Value) -> Value { Value::ptr_id(p as *const u8) }

    #[inline(always)]
    pub fn is_cell(self) -> bool { self.is_ptr_id(Tag::Cell) }

    #[inline(always)]
    pub fn as_cell<'a>(self) -> &'a mut Value
    {
        debug_assert!(self.is_cell());
        unsafe { &mut *(self.heap_ptr() as *mut Value) }
    }

    #[inline(always)]
    pub fn to_cell<'a>(self) -> Option<&'a mut Value>
    {
        if self.is_cell() { Some(self.as_cell()) } else { None }
    }

    // Numbers

    #[inline(always)]
    pub fn is_int64(self) -> bool
    {
        self.is_fixnum() || self.is_int64_box()
    }

    #[inline(always)]
    pub fn is_float64(self) -> bool
    {
        self.is_flonum() || self.is_float64_box()
    }

    #[inline(always)]
    pub fn is_num(self) -> bool
    {
        self.is_int64() || self.is_float64()
    }

    #[inline(always)]
    pub fn to_i64(self) -> Option<i64>
    {
        if self.is_fixnum() {
            Some(self.as_fixnum())
        } else if self.is_int64_box() {
            Some(unsafe { *(self.heap_ptr() as *const i64) })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn to_f64(self) -> Option<f64>
    {
        if self.is_flonum() {
            Some(self.as_flonum())
        } else if self.is_float64_box() {
            Some(unsafe { *(self.heap_ptr() as *const f64) })
        } else {
            None
        }
    }

    /// Numeric value as a double, whatever the representation
    #[inline(always)]
    pub fn num_as_f64(self) -> f64
    {
        debug_assert!(self.is_num());

        match self.to_i64() {
            Some(v) => v as f64,
            None => self.to_f64().unwrap(),
        }
    }

    #[inline(always)]
    pub fn to_u8(self) -> Option<u8> { self.to_i64().and_then(|v| u8::try_from(v).ok()) }

    // Kept for symmetry with the other width conversions, though no host
    // function currently takes an i32 parameter
    #[allow(dead_code)]
    #[inline(always)]
    pub fn to_i32(self) -> Option<i32> { self.to_i64().and_then(|v| i32::try_from(v).ok()) }

    #[inline(always)]
    pub fn to_u32(self) -> Option<u32> { self.to_i64().and_then(|v| u32::try_from(v).ok()) }

    #[inline(always)]
    pub fn to_u64(self) -> Option<u64> { self.to_i64().and_then(|v| u64::try_from(v).ok()) }

    #[inline(always)]
    pub fn to_usize(self) -> Option<usize> { self.to_i64().and_then(|v| usize::try_from(v).ok()) }

    /// Language-level type, for cold paths. Hot paths should test the
    /// specific type they expect instead.
    pub fn type_of(self) -> Type
    {
        match self.tag() {
            0b000 | 0b100 => Type::Int64,
            0b010 | 0b110 => Type::Float64,

            TAG_PTR_ID | TAG_PTR_VAL => match self.heap_tag() {
                Tag::Str => Type::String,
                Tag::Object => Type::Object,
                Tag::Closure => Type::Closure,
                Tag::Array => Type::Array,
                Tag::ByteArray => Type::ByteArray,
                Tag::Dict => Type::Dict,
                Tag::Cell => Type::Cell,
                Tag::Int64 => Type::Int64,
                Tag::Float64 => Type::Float64,
                tag => panic!("value points at a {:?} block", tag),
            },

            TAG_IMM => match self.0 as u8 as u64 {
                IMM_NIL => Type::Nil,
                IMM_TRUE | IMM_FALSE => Type::Bool,
                IMM_UNDEF => Type::Undef,
                IMM_FUN => Type::Fun,
                IMM_CLASS => Type::Class,
                IMM_HOSTFN => Type::HostFn,
                _ => panic!("invalid immediate {:#x}", self.0),
            },

            _ => panic!("invalid value tag {:#x}", self.0),
        }
    }
}

impl PartialEq for Value
{
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool
    {
        // Bitwise equality decides every type that isn't compared by value
        if (self.0 | other.0) & VAL_BIT == 0 {
            return self.0 == other.0;
        }

        slow_eq(*self, *other)
    }
}

impl Eq for Value {}

/// Equality for the cases a word compare can't settle: strings compare
/// structurally so that the collector can intern them, and numbers compare
/// across representations and across int/float.
#[cold]
fn slow_eq(a: Value, b: Value) -> bool
{
    if a.is_string() && b.is_string() {
        return a.raw_bits() == b.raw_bits() || a.as_str() == b.as_str();
    }

    if a.is_num() && b.is_num() {
        return match (a.to_i64(), b.to_i64()) {
            (Some(a), Some(b)) => a == b,
            _ => a.num_as_f64() == b.num_as_f64(),
        };
    }

    false
}

impl fmt::Debug for Value
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match self.type_of() {
            Type::Undef => write!(f, "undef"),
            Type::Nil => write!(f, "nil"),
            Type::Bool => write!(f, "{}", self.as_bool()),
            Type::Int64 => write!(f, "{}", self.to_i64().unwrap()),
            Type::Float64 => write!(f, "{}", self.to_f64().unwrap()),
            Type::String => write!(f, "{:?}", self.as_str()),
            Type::Fun => write!(f, "Fun({:?})", self.as_fun()),
            Type::Class => write!(f, "Class({:?})", self.as_class()),
            Type::HostFn => write!(f, "HostFn({})", self.as_host_fn().name),
            t => write!(f, "{:?}({:p})", t, self.heap_ptr()),
        }
    }
}

impl From<bool> for Value {
    fn from(val: bool) -> Self { Value::bool_val(val) }
}

// Integer types narrow enough to always fit in a fixnum. i64, u64 and usize
// are deliberately absent: those need the allocator to box on overflow.
impl From<u8> for Value {
    fn from(val: u8) -> Self { Value::fixnum(val as i64) }
}

impl From<i32> for Value {
    fn from(val: i32) -> Self { Value::fixnum(val as i64) }
}

impl From<u32> for Value {
    fn from(val: u32) -> Self { Value::fixnum(val as i64) }
}

/// Unwrap a value or report a type error. `$conv` is one of the `to_*`
/// methods and `$expect` names the type in the error message.
macro_rules! unwrap_val {
    ($conv:ident, $expect:literal, $val:expr, $req:literal) => {
        match $val.$conv() {
            Some(v) => v,
            None => error!($req, "expected {} value but got {:?}", $expect, $val)
        }
    }
}

pub(crate) use unwrap_val;

/// The `unwrap_*` macros take an instruction name when used inside the
/// interpreter loop, and nothing when used inside a host function. They
/// report a type error where the old ones panicked or matched by hand.
macro_rules! unwrap_i64 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_i64, "int64", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_i64, "int64", $val, "") };
}

macro_rules! unwrap_f64 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_f64, "float64", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_f64, "float64", $val, "") };
}

macro_rules! unwrap_u8 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_u8, "byte-sized integer", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_u8, "byte-sized integer", $val, "") };
}

// Kept for symmetry with the other numeric unwrappers, though nothing
// currently takes an i32 parameter
#[allow(unused_macros)]
macro_rules! unwrap_i32 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_i32, "i32-sized integer", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_i32, "i32-sized integer", $val, "") };
}

macro_rules! unwrap_u32 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_u32, "u32-sized integer", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_u32, "u32-sized integer", $val, "") };
}

macro_rules! unwrap_u64 {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_u64, "non-negative integer", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_u64, "non-negative integer", $val, "") };
}

macro_rules! unwrap_usize {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_usize, "non-negative integer", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_usize, "non-negative integer", $val, "") };
}

#[allow(unused_macros)]
macro_rules! unwrap_bool {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_bool, "boolean", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_bool, "boolean", $val, "") };
}

macro_rules! unwrap_str {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_str, "string", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_str, "string", $val, "") };
}

#[allow(unused_macros)]
macro_rules! unwrap_obj {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_obj, "object", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_obj, "object", $val, "") };
}

macro_rules! unwrap_arr {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_arr, "array", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_arr, "array", $val, "") };
}

macro_rules! unwrap_ba {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_ba, "byte array", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_ba, "byte array", $val, "") };
}

macro_rules! unwrap_dict {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_dict, "dict", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_dict, "dict", $val, "") };
}

#[allow(unused_macros)]
macro_rules! unwrap_clos {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_clos, "closure", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_clos, "closure", $val, "") };
}

#[allow(unused_macros)]
macro_rules! unwrap_fun {
    ($val: expr, $req: literal) => { $crate::value::unwrap_val!(to_fun, "function", $val, $req) };
    ($val: expr) => { $crate::value::unwrap_val!(to_fun, "function", $val, "") };
}

#[allow(unused_imports)]
pub(crate) use {
    unwrap_arr, unwrap_ba, unwrap_bool, unwrap_clos, unwrap_dict, unwrap_f64, unwrap_fun,
    unwrap_i32, unwrap_i64, unwrap_obj, unwrap_str, unwrap_u32, unwrap_u64, unwrap_u8,
    unwrap_usize,
};

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn immediates()
    {
        assert!(Value::NIL.is_nil());
        assert!(Value::TRUE.is_true() && Value::TRUE.is_bool());
        assert!(Value::FALSE.is_false() && Value::FALSE.is_bool());
        assert!(Value::UNDEF.is_undef());
        assert!(!Value::NIL.is_bool() && !Value::UNDEF.is_bool());
        assert!(Value::bool_val(true).as_bool());
        assert!(!Value::bool_val(false).as_bool());

        for v in [Value::NIL, Value::TRUE, Value::FALSE, Value::UNDEF] {
            assert!(!v.is_fixnum() && !v.is_flonum() && !v.is_heap());
            assert_eq!(v.tag(), TAG_IMM);
        }
    }

    #[test]
    fn fixnums()
    {
        for v in [0, 1, -1, 7, -7, 1 << 40, Value::FIXNUM_MAX, Value::FIXNUM_MIN] {
            let val = Value::fixnum(v);
            assert!(val.is_fixnum() && val.is_int64() && val.is_num());
            assert_eq!(val.as_fixnum(), v);
            assert_eq!(val.to_i64(), Some(v));
            assert_eq!(val.type_of(), Type::Int64);
        }

        assert!(Value::try_fixnum(Value::FIXNUM_MAX + 1).is_none());
        assert!(Value::try_fixnum(Value::FIXNUM_MIN - 1).is_none());
        assert!(Value::try_fixnum(i64::MAX).is_none());

        // Tagged words add directly and order like the integers they hold
        let a = Value::fixnum(7);
        let b = Value::fixnum(-3);
        assert_eq!(Value::from_raw_bits(a.raw_bits().wrapping_add(b.raw_bits())).as_fixnum(), 4);
        assert!((a.raw_bits() as i64) > (b.raw_bits() as i64));
    }

    #[test]
    fn flonums()
    {
        let covered = [
            0.0, -0.0, 1.0, -1.0, 2.0, 0.5, 3.14159, 1e-38, -1e38, 3.4e38,
            5e-324, 1e-300, f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
            f64::MIN_POSITIVE, 2.0f64.powi(-128), 2.0f64.powi(127),
        ];

        for v in covered {
            let val = Value::try_flonum(v).expect("should encode inline");
            assert!(val.is_flonum() && val.is_float64() && val.is_num());
            assert_eq!(val.as_flonum().to_bits(), v.to_bits());
            assert_eq!(val.type_of(), Type::Float64);
            assert!(!val.is_fixnum() && !val.is_heap());
        }

        for v in [1e39, 1e100, 1e-100, 2.0f64.powi(128), 2.0f64.powi(-129)] {
            assert!(Value::try_flonum(v).is_none());
        }
    }

    #[test]
    fn equality()
    {
        assert_eq!(Value::NIL, Value::NIL);
        assert_ne!(Value::NIL, Value::FALSE);
        assert_ne!(Value::NIL, Value::UNDEF);
        assert_ne!(Value::TRUE, Value::FALSE);
        assert_ne!(Value::TRUE, Value::fixnum(1));
        assert_eq!(Value::fixnum(42), Value::fixnum(42));
        assert_ne!(Value::fixnum(42), Value::fixnum(43));
        assert_ne!(Value::fixnum(0), Value::NIL);

        let flo = |v: f64| Value::try_flonum(v).unwrap();

        // Ints and floats compare across types
        assert_eq!(Value::fixnum(1), flo(1.0));
        assert_eq!(flo(1.0), Value::fixnum(1));
        assert_ne!(Value::fixnum(1), flo(1.5));
        assert_ne!(Value::fixnum(1), Value::NIL);

        // Float identities the bit pattern gets wrong on its own
        assert_eq!(flo(0.0), flo(-0.0));
        assert_ne!(flo(f64::NAN), flo(f64::NAN));
        assert_eq!(flo(f64::INFINITY), flo(f64::INFINITY));
    }

    #[test]
    fn ids()
    {
        let fun = Value::fun(FunId::from(1234usize));
        assert!(fun.is_fun() && !fun.is_class() && !fun.is_heap());
        assert_eq!(fun.as_fun(), FunId::from(1234usize));
        assert_eq!(fun.type_of(), Type::Fun);

        let class = Value::class(ClassId::from(7usize));
        assert!(class.is_class() && !class.is_fun());
        assert_eq!(class.as_class(), ClassId::from(7usize));
        assert_eq!(class.type_of(), Type::Class);

        assert_eq!(Value::fun(FunId::from(9usize)), Value::fun(FunId::from(9usize)));
        assert_ne!(Value::fun(FunId::from(9usize)), Value::class(ClassId::from(9usize)));
    }

    #[test]
    fn tags_are_disjoint()
    {
        // No flonum encoding can be mistaken for another class
        for i in 0..10000 {
            let v = i as f64 * 0.7 - 3000.0;

            if let Some(val) = Value::try_flonum(v) {
                assert!(!val.is_fixnum() && !val.is_heap());
                assert_eq!(val.tag() & NUM_MASK, TAG_FLONUM);
            }
        }
    }
}
