#![allow(dead_code)]
#![allow(non_camel_case_types)]
use std::fmt;
use std::mem::transmute;
use crate::host::HostFnId;

// Instructions are one 64-bit word. The opcode occupies the low 8 bits.
// Operands are packed from bit 8 upwards, except the last one, which is
// placed at the top of the word. Top-aligning the last operand makes its
// extraction one shift instead of two, which is why signed immediates
// should be declared last.
const OPCODE_BITS: u32 = 8;

/// Width in bits of each operand kind
macro_rules! opnd_width {
    (out_reg) => { 16 };
    (reg) => { 16 };
    (u1) => { 1 };
    (u2) => { 2 };
    (u3) => { 3 };
    (u4) => { 4 };
    (u5) => { 5 };
    (u6) => { 6 };
    (u8) => { 8 };
    (u12) => { 12 };
    (u16) => { 16 };
    (u20) => { 20 };
    (u24) => { 24 };
    (u32) => { 32 };
    (i16) => { 16 };
    (i32) => { 32 };
    (disp16) => { 16 };
    (disp24) => { 24 };
    (disp32) => { 32 };
}

/// Rust type an operand decodes to
macro_rules! opnd_type {
    (out_reg) => { u16 };
    (reg) => { u16 };
    (u1) => { u8 };
    (u2) => { u8 };
    (u3) => { u8 };
    (u4) => { u8 };
    (u5) => { u8 };
    (u6) => { u8 };
    (u8) => { u8 };
    (u12) => { u16 };
    (u16) => { u16 };
    (u20) => { u32 };
    (u24) => { u32 };
    (u32) => { u32 };
    (i16) => { i16 };
    (i32) => { i32 };
    (disp16) => { i16 };
    (disp24) => { i32 };
    (disp32) => { i32 };
}

/// Whether an operand is sign-extended when decoded
macro_rules! opnd_signed {
    (i16) => { true };
    (i32) => { true };
    (disp16) => { true };
    (disp24) => { true };
    (disp32) => { true };
    ($other:ident) => { false };
}

/// 1 for operands the instruction writes to, 0 otherwise
macro_rules! opnd_is_out {
    (out_reg) => { 1 };
    ($other:ident) => { 0 };
}

/// 1 for branch displacement operands, 0 otherwise
macro_rules! opnd_is_disp {
    (disp16) => { 1 };
    (disp24) => { 1 };
    (disp32) => { 1 };
    ($other:ident) => { 0 };
}

// Keep the running result unless this operand is the branch displacement
macro_rules! opnd_pick_disp {
    (disp16, $prev:expr, $v:expr) => { Some($v as i32) };
    (disp24, $prev:expr, $v:expr) => { Some($v as i32) };
    (disp32, $prev:expr, $v:expr) => { Some($v as i32) };
    ($other:ident, $prev:expr, $v:expr) => { $prev };
}

// Re-encoding an instruction: pass the operand through, or substitute a new
// displacement if this is the branch displacement operand
macro_rules! opnd_or_disp {
    (disp16, $v:expr, $new:expr) => { $new as i16 };
    (disp24, $v:expr, $new:expr) => { $new };
    (disp32, $v:expr, $new:expr) => { $new };
    ($other:ident, $v:expr, $new:expr) => { $v };
}

/// How an operand is printed by the disassembler
macro_rules! opnd_fmt {
    (out_reg, $f:expr, $v:expr) => { write!($f, "r{}", $v) };
    (reg, $f:expr, $v:expr) => { write!($f, "r{}", $v) };
    ($other:ident, $f:expr, $v:expr) => { write!($f, "{}", $v) };
}

macro_rules! decode_opnd {
    ($insn:expr, $pos:expr, $kind:ident) => {
        if opnd_signed!($kind) {
            $insn.bits_i($pos, opnd_width!($kind)) as opnd_type!($kind)
        } else {
            $insn.bits_u($pos, opnd_width!($kind)) as opnd_type!($kind)
        }
    };
}

// Bind each operand to a local, walking the field list to accumulate bit
// positions. The last operand is top-aligned, so it gets a fixed position.
macro_rules! gen_decode {
    // No operands left, nothing to bind
    ($insn:ident, $pos:expr,) => {};

    // Last operand: read it from the top of the word
    ($insn:ident, $pos:expr, $fname:ident : $kind:ident,) => {
        let $fname = decode_opnd!($insn, 64 - opnd_width!($kind), $kind);
    };

    // Any other operand: read it at the current position, then advance by its width
    ($insn:ident, $pos:expr, $fname:ident : $kind:ident, $($rest:tt)+) => {
        let $fname = decode_opnd!($insn, $pos, $kind);
        gen_decode!($insn, $pos + opnd_width!($kind), $($rest)+);
    };
}

macro_rules! gen_encode {
    // No operands left, the word is complete
    ($word:ident, $pos:expr,) => {};

    // Last operand: write it to the top of the word
    ($word:ident, $pos:expr, $fname:ident : $kind:ident,) => {
        $word = $word.with_bits(64 - opnd_width!($kind), opnd_width!($kind), $fname as i64 as u64);
    };

    // Any other operand: write it at the current position, then advance by its width
    ($word:ident, $pos:expr, $fname:ident : $kind:ident, $($rest:tt)+) => {
        $word = $word.with_bits($pos, opnd_width!($kind), $fname as i64 as u64);
        gen_encode!($word, $pos + opnd_width!($kind), $($rest)+);
    };
}

// Check that an operand value is in range for the field it is encoded
// into. This is a hard assert because encoding happens at compile time,
// not in the interpreter loop, and an operand that silently gets truncated
// here produces a wrong instruction rather than a crash
macro_rules! check_opnd {
    ($fname:ident, $kind:ident) => {
        assert!(
            Insn::fits($fname as i64, opnd_width!($kind), opnd_signed!($kind)),
            concat!("operand `", stringify!($fname), "` does not fit in ", stringify!($kind))
        );
    };
}

// Helper macro to define instruction opcodes and their operands
macro_rules! def_opcodes {
    (
        $(
            $(#[doc = $doc:expr])*
            $name:ident $(= $val:literal)? {
                $($fname:ident : $kind:ident),* $(,)?
            },
        )*
    ) => {
        pub const NUM_OPCODES: usize = 0 $( + { let _ = stringify!($name); 1 } )*;

        // Opcodes are transmuted from a byte, so they have to cover it
        // densely. The highest opcode value stays below 255, which leaves
        // that value free for a future extension mechanism.
        const _: () = assert!(NUM_OPCODES <= 255);

        /// Instruction opcodes
        #[derive(PartialEq, Eq, Copy, Clone, Debug)]
        #[repr(u8)]
        pub enum Opcode
        {
            $(
                $(#[doc = $doc])*
                $name $(= $val)?,
            )*
        }

        impl Opcode
        {
            pub fn num_opnds(self) -> usize
            {
                match self {
                    $(
                        Opcode::$name => 0 $(+ { let _ = stringify!($fname); 1 })*,
                    )*
                }
            }

            pub fn num_outs(self) -> usize
            {
                match self {
                    $(
                        Opcode::$name => 0 $(+ opnd_is_out!($kind))*,
                    )*
                }
            }

            pub fn name(self) -> &'static str
            {
                match self {
                    $(
                        Opcode::$name => stringify!($name),
                    )*
                }
            }

            pub fn from_str(s: &str) -> Option<Self>
            {
                match s {
                    $(
                        stringify!($name) => Some(Opcode::$name),
                    )*
                    _ => None
                }
            }

            /// Whether this instruction has a pc-relative branch displacement
            pub fn is_branch(self) -> bool
            {
                match self {
                    $(
                        Opcode::$name => (0 $(+ opnd_is_disp!($kind))*) > 0,
                    )*
                }
            }
        }

        // Decoded operands, one struct per opcode
        $(
            $(#[doc = $doc])*
            #[derive(PartialEq, Eq, Copy, Clone, Debug)]
            pub struct $name
            {
                $(pub $fname: opnd_type!($kind),)*
            }

            const _: () = assert!(
                OPCODE_BITS as usize $(+ opnd_width!($kind) as usize)* <= 64,
                concat!("operands of `", stringify!($name), "` do not fit in one word")
            );

            impl $name
            {
                #[inline(always)]
                pub fn decode(insn: Insn) -> Self
                {
                    debug_assert_eq!(insn.opcode(), Opcode::$name);
                    gen_decode!(insn, OPCODE_BITS, $($fname: $kind,)*);
                    Self { $($fname,)* }
                }
            }
        )*

        impl Insn
        {
            $(
                $(#[doc = $doc])*
                #[inline(always)]
                pub fn $name($($fname: opnd_type!($kind)),*) -> Insn
                {
                    $(check_opnd!($fname, $kind);)*
                    #[allow(unused_mut)]
                    let mut word = Insn(Opcode::$name as u64);
                    gen_encode!(word, OPCODE_BITS, $($fname: $kind,)*);
                    word
                }
            )*

            /// Get the branch displacement of a jump instruction, if it has one
            #[allow(unused_mut, unused_assignments, unused_variables)]
            pub fn branch_disp(self) -> Option<i32>
            {
                match self.opcode() {
                    $(
                        Opcode::$name => {
                            let opnds = $name::decode(self);
                            let mut disp: Option<i32> = None;
                            $(disp = opnd_pick_disp!($kind, disp, opnds.$fname);)*
                            disp
                        }
                    )*
                }
            }

            /// Replace the branch displacement of a jump instruction.
            /// The displacement is counted in instructions, relative to the
            /// one following the branch, so a displacement of zero falls
            /// through. Instructions with no displacement are returned
            /// unchanged.
            #[allow(unused_variables)]
            pub fn with_branch_disp(self, disp: i32) -> Insn
            {
                debug_assert!(self.opcode().is_branch(), "not a branch instruction");

                match self.opcode() {
                    $(
                        Opcode::$name => {
                            let opnds = $name::decode(self);
                            Insn::$name($(opnd_or_disp!($kind, opnds.$fname, disp)),*)
                        }
                    )*
                }
            }
        }

        impl fmt::Display for Insn
        {
            #[allow(unused_mut, unused_assignments, unused_variables)]
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
            {
                match self.opcode() {
                    $(
                        Opcode::$name => {
                            let opnds = $name::decode(*self);
                            write!(f, "{}", stringify!($name))?;
                            let mut first = true;
                            $(
                                write!(f, "{}", if first { " " } else { ", " })?;
                                first = false;
                                opnd_fmt!($kind, f, opnds.$fname)?;
                            )*
                            Ok(())
                        }
                    )*
                }
            }
        }
    }
}

// Notes:
// - No instructions should directly handle system resources such as
//   input and output devices in order to enable sandboxing. This should be
//   done through host functions instead.
// - Future rest arguments will be handled by allocating an array,
//   which avoids the complexity of frames with variable arg counts
// - Calls:
//   - The design uses register windows like Lua
//   - The callee's frame base is caller_bp + start_reg
//   - The return value is written to the callee's frame base, which is
//     start_reg in the caller. Calls therefore need no destination operand
//   - argc counts every register the call occupies from start_reg on.
//     For a method call, self is the first of those, so it lands in the
//     callee's r0 and is counted in argc
//   - Future tail calls can just mutate the frame in-place
// - Field and method names are NameIds, indices into an interned name
//   table that lives outside the heap. Instructions hold no heap pointers,
//   so the instruction stream is not scanned by the collector
// - Sites that cache a lookup hold a CacheIdx instead of the cached data,
//   which keeps them within one word and lets cache entries grow later
def_opcodes! {
    // Halt execution and produce an error. The source position is packed
    // into the operands so that the error can be reported without a side
    // table. The fields are wider than any real source file needs, but a
    // generated one could overflow them, so the caller saturates rather
    // than letting the encoding assert fire.
    // Panic is zero so that jumping to uninitialized memory causes panic
    panic = 0 { file_id: u12, line_no: u24, col_no: u20 },

    // Debugger breakpoint
    breakpoint {},

    // Can be used for code patching, or to disable breakpoints
    nop {},

    // Load a sign-extended immediate into a register as the raw bits of a
    // value. Covers nil, undef, booleans and fixnums in the +/- 2^29 range
    load_imm32 { dst: out_reg, imm: i32 },

    // Copy a value from one register to another
    mov { dst: out_reg, src: reg },

    // Load a constant from a constant slot into a register
    load_const { dst: out_reg, slot: u32 },

    // Global variable access.
    get_global { dst: out_reg, idx: u32 },
    set_global { src: reg, idx: u32 },

    // Bitwise operations
    lshift { dst: out_reg, a: reg, b: reg },
    rshift { dst: out_reg, a: reg, b: reg },
    bit_and { dst: out_reg, a: reg, b: reg },
    bit_or { dst: out_reg, a: reg, b: reg },
    bit_xor { dst: out_reg, a: reg, b: reg },
    bit_not { dst: out_reg, src: reg },

    add { dst: out_reg, a: reg, b: reg },
    sub { dst: out_reg, a: reg, b: reg },
    mul { dst: out_reg, a: reg, b: reg },
    div { dst: out_reg, a: reg, b: reg },
    div_int { dst: out_reg, a: reg, b: reg },
    modulo { dst: out_reg, a: reg, b: reg },

    // Arithmetic ops with fixnum immediates
    add_imm16 { dst: out_reg, a: reg, imm: i16 },
    sub_imm16 { dst: out_reg, a: reg, imm: i16 },
    mul_imm16 { dst: out_reg, a: reg, imm: i16 },

    // Logical negation of a boolean
    not { dst: out_reg, src: reg },

    // Closure operations.
    clos_new { dst: out_reg, fun_id: u24, num_slots: u12 },
    clos_set { clos: reg, src: reg, idx: u12 },
    clos_get { dst: out_reg, idx: u12 },

    // Mutable cell operations. A new cell starts out holding nil.
    cell_new { dst: out_reg },
    cell_set { cell: reg, src: reg },
    cell_get { dst: out_reg, cell: reg },

    // Check if instance of class
    instanceof { dst: out_reg, val: reg, class_id: u24 },

    // The field name, class id and slot index all live in the PropCache
    // entry, which is what keeps these within one word
    get_field { dst: out_reg, obj: reg, cache: u24 },
    set_field { obj: reg, src: reg, cache: u24 },

    // Get/set indexed element
    get_index { dst: out_reg, arr: reg, idx: reg },
    set_index { arr: reg, idx: reg, src: reg },

    // Create a new dictionary
    dict_new { dst: out_reg },

    // Array operations
    arr_new { dst: out_reg, capacity: u32 },
    arr_push { arr: reg, val: reg },

    // Clone a bytearray
    ba_clone { dst: out_reg, src: reg },

    // Jump if true/false
    if_true { val: reg, disp: disp32 },
    if_false { val: reg, disp: disp32 },

    // The displacement takes the 24 bits left over by the two registers,
    // which is more range than any function will need
    jeq { a: reg, b: reg, disp: disp24 },
    jne { a: reg, b: reg, disp: disp24 },
    jlt { a: reg, b: reg, disp: disp24 },
    jle { a: reg, b: reg, disp: disp24 },
    jgt { a: reg, b: reg, disp: disp24 },
    jge { a: reg, b: reg, disp: disp24 },

    // Unconditional jump
    jmp { disp: disp32 },

    // Call a function operand (dynamic call). The callee is not known, so
    // the site caches the last one it resolved and guards on it
    call_opnd { start_reg: reg, argc: u8, cache: u24 },

    // Call a known host function provided by the VM
    call_host { host_fn: u16, start_reg: reg, argc: u8 },

    // Call a known function by its id
    // This gets optimized into call_pc once the callee has been compiled
    call_direct { fun: u24, start_reg: reg, argc: u8 },

    // The callee is statically known, so there is nothing to guard on.
    // We still use a cache entry because we can't fit all of the parameters
    // directly into one 64-bit instruction, but this instruction will
    // not deoptimize.
    call_pc { start_reg: reg, argc: u8, cache: u24 },

    // Call a method on the object in start_reg. The method name, and the
    // class it was last looked up on, live in the CallCache entry
    call_method { start_reg: reg, argc: u8, cache: u24 },

    // Call a host method, guarded on the type tag of the value in
    // start_reg. This is used for primitive types such as Int64, Float64
    // and String. The guard is an immediate, so a miss costs no loads.
    // The host function itself lives in the CallCache entry, which leaves
    // room for the cache index: this site deoptimizes back into
    // call_method, and the two forms take the same operands so that the
    // switch is a write of the opcode byte
    call_method_host { start_reg: reg, argc: u8, type_tag: u8, cache: u24 },

    // Creating a class instance runs a constructor, so it is a call
    new { class_id: u24, start_reg: reg, argc: u8 },
    new_known_ctor { start_reg: reg, argc: u8, cache: u24 },

    // Return the value found in a register
    ret { src: reg },

    // Return nil, as functions with no explicit return value do
    ret_nil {},

    // Return a sign-extended immediate as the raw bits of a value.
    // Covers nil, undef, booleans and fixnums in the +/- 2^29 range
    ret_imm32 { imm: i32 },
}

// Sketch of the side tables the instructions above index into. These will
// likely move next to the code they are used by once the VM is written.

/// Index into the program's interned name table, which holds field and
/// method names. Names are immortal and live outside the heap, so they
/// never move and never need to be traced
pub type NameId = u32;

/// Index into a function's array of inline cache entries
pub type CacheIdx = u32;

/// Cache for a field access site
pub struct PropCache
{
    pub name: NameId,

    // Class the field was last looked up on, and where it was found.
    // The slot index is only valid for objects of that class
    pub class_id: u32,
    pub slot_idx: u32,
}

/// Cache for a call site whose callee is not statically known. Dynamic
/// calls guard on the function they last resolved to, method calls on the
/// class they last looked the name up on. A statically known callee needs
/// no entry here: it becomes a call_pc instead
pub struct CallCache
{
    pub name: NameId,
    pub class_id: u32,

    // Callee the site resolved to. The function id is both the guard and
    // what the stack frame records, and the frame needs the entry point
    // and its own size
    pub fun_id: u32,
    pub entry_pc: u32,
    pub num_locals: u16,

    // Host function a call_method_host site resolved to. That site guards
    // on the type tag in the instruction, so this is only read on a hit
    pub host_fn: HostFnId,
}

/// Interpreter instruction
#[derive(PartialEq, Eq, Copy, Clone)]
pub struct Insn(u64);

impl Insn
{
    #[inline(always)]
    pub fn opcode(&self) -> Opcode
    {
        let Insn(word) = self;
        debug_assert!((*word as u8 as usize) < NUM_OPCODES);
        unsafe { transmute(*word as u8) }
    }

    /// Get the full u64 instruction word
    #[inline(always)]
    pub fn word_u64(&self) -> u64
    {
        let Insn(word) = self;
        *word
    }

    /// Extract an unsigned operand field
    #[inline(always)]
    const fn bits_u(&self, pos: u32, width: u32) -> u64
    {
        (self.0 >> pos) & (u64::MAX >> (64 - width))
    }

    /// Extract a sign-extended operand field
    #[inline(always)]
    const fn bits_i(&self, pos: u32, width: u32) -> i64
    {
        ((self.0 << (64 - pos - width)) as i64) >> (64 - width)
    }

    #[inline(always)]
    const fn with_bits(self, pos: u32, width: u32, val: u64) -> Insn
    {
        Insn(self.0 | ((val & (u64::MAX >> (64 - width))) << pos))
    }

    /// Whether a value is representable in a field of a given width
    const fn fits(val: i64, width: u32, signed: bool) -> bool
    {
        if signed {
            val >= -(1i64 << (width - 1)) && val < (1i64 << (width - 1))
        } else {
            val >= 0 && (val as u64) < (1u64 << width)
        }
    }
}

impl fmt::Debug for Insn
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        write!(f, "{}", self)
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn opcode_count()
    {
        // Update this when adding opcodes, it is here to make the size of
        // the instruction set visible as it grows
        assert_eq!(NUM_OPCODES, 59);

        // Opcodes are dense from zero, so this is the highest opcode value.
        // It has to stay below 255 to leave room for a future extension.
        assert!(NUM_OPCODES - 1 < 255);
        assert_eq!(Opcode::ret_imm32 as usize, NUM_OPCODES - 1);
    }

    #[test]
    fn no_operands()
    {
        let insn = Insn::nop();
        assert_eq!(insn.opcode(), Opcode::nop);
        assert_eq!(insn.word_u64(), Opcode::nop as u64);
        assert_eq!(Opcode::nop.num_opnds(), 0);
    }

    #[test]
    fn panic_is_zero()
    {
        assert_eq!(Insn::panic(0, 0, 0).word_u64(), 0);
    }

    #[test]
    fn panic_src_pos()
    {
        // The three fields fill the word exactly, so each one has to come
        // back out without disturbing the others
        let insn = Insn::panic((1 << 12) - 1, (1 << 24) - 1, (1 << 20) - 1);
        assert_eq!(
            panic::decode(insn),
            panic { file_id: (1 << 12) - 1, line_no: (1 << 24) - 1, col_no: (1 << 20) - 1 }
        );
        assert_eq!(insn.word_u64(), u64::MAX & !0xFF);

        assert_eq!(panic::decode(Insn::panic(3, 0, 0)), panic { file_id: 3, line_no: 0, col_no: 0 });
        assert_eq!(panic::decode(Insn::panic(0, 3, 0)), panic { file_id: 0, line_no: 3, col_no: 0 });
        assert_eq!(panic::decode(Insn::panic(0, 0, 3)), panic { file_id: 0, line_no: 0, col_no: 3 });
    }

    #[test]
    #[should_panic(expected = "does not fit")]
    fn out_of_range_operand()
    {
        // Range checks are hard asserts, so this fires in release too
        Insn::panic(1 << 12, 0, 0);
    }

    #[test]
    fn three_registers()
    {
        let insn = Insn::add(1, 2, 3);
        assert_eq!(insn.opcode(), Opcode::add);
        assert_eq!(add::decode(insn), add { dst: 1, a: 2, b: 3 });
    }

    #[test]
    fn register_extremes()
    {
        let insn = Insn::add(u16::MAX, 0, u16::MAX);
        assert_eq!(add::decode(insn), add { dst: u16::MAX, a: 0, b: u16::MAX });
    }

    #[test]
    fn signed_immediates_round_trip()
    {
        for imm in [0, 1, -1, i16::MAX, i16::MIN] {
            let insn = Insn::add_imm16(7, 8, imm);
            assert_eq!(add_imm16::decode(insn), add_imm16 { dst: 7, a: 8, imm });
        }

        for imm in [0, 1, -1, i32::MAX, i32::MIN] {
            assert_eq!(ret_imm32::decode(Insn::ret_imm32(imm)), ret_imm32 { imm });
            assert_eq!(load_imm32::decode(Insn::load_imm32(3, imm)), load_imm32 { dst: 3, imm });
        }
    }

    #[test]
    fn last_operand_is_top_aligned()
    {
        // The branch displacement occupies the top 24 bits, the imm32 the top 32
        assert_eq!((Insn::jlt(1, 2, -3).word_u64() as i64 >> 40) as i32, -3);
        assert_eq!((Insn::load_imm32(1, -7).word_u64() >> 32) as i32, -7);
    }

    #[test]
    fn branch_disp_range()
    {
        // A conditional jump uses the whole word, 24 bits of it for the
        // displacement
        for disp in [0, 1, -1, (1 << 23) - 1, -(1 << 23)] {
            let insn = Insn::jlt(1, 2, disp);
            assert_eq!(jlt::decode(insn), jlt { a: 1, b: 2, disp });
        }
    }

    #[test]
    fn branch_helpers()
    {
        assert!(Opcode::jlt.is_branch());
        assert!(Opcode::jmp.is_branch());
        assert!(!Opcode::add.is_branch());

        assert_eq!(Insn::jlt(1, 2, -3).branch_disp(), Some(-3));
        assert_eq!(Insn::jmp(1234).branch_disp(), Some(1234));
        assert_eq!(Insn::add(1, 2, 3).branch_disp(), None);

        // Patching a forward jump leaves the other operands alone
        let patched = Insn::jlt(1, 2, 0).with_branch_disp(-3);
        assert_eq!(patched, Insn::jlt(1, 2, -3));
        assert_eq!(Insn::jmp(0).with_branch_disp(7), Insn::jmp(7));
    }

    #[test]
    fn operands_do_not_overlap()
    {
        // Each operand set on its own must leave the others zero
        assert_eq!(call_host::decode(Insn::call_host(u16::MAX, 0, 0)).start_reg, 0);
        assert_eq!(call_host::decode(Insn::call_host(0, u16::MAX, 0)).host_fn, 0);
        assert_eq!(call_host::decode(Insn::call_host(0, 0, u8::MAX)).host_fn, 0);
        assert_eq!(
            call_host::decode(Insn::call_host(1, 2, 3)),
            call_host { host_fn: 1, start_reg: 2, argc: 3 }
        );

        // A top-aligned u24 sitting above a u8
        assert_eq!(call_opnd::decode(Insn::call_opnd(0, 0, (1 << 24) - 1)).argc, 0);
        assert_eq!(call_opnd::decode(Insn::call_opnd(0, u8::MAX, 0)).cache, 0);

        // call_method_host fills the word exactly, and deoptimizes into
        // call_method, so the operands the two share have to line up
        let host = Insn::call_method_host(1, 2, 3, 4);
        assert_eq!(
            call_method_host::decode(host),
            call_method_host { start_reg: 1, argc: 2, type_tag: 3, cache: 4 }
        );
        let opnds = call_method::decode(Insn::call_method(1, 2, 4));
        assert_eq!((opnds.start_reg, opnds.argc, opnds.cache), (1, 2, 4));
    }

    #[test]
    fn narrow_operand_kinds()
    {
        // The narrow kinds are declared but unused by the table above, so
        // exercise the width arithmetic directly
        assert!(Insn::fits(1, 1, false));
        assert!(!Insn::fits(2, 1, false));
        assert!(Insn::fits(7, 3, false));
        assert!(!Insn::fits(8, 3, false));
        assert!(Insn::fits(15, 4, false));
        assert!(!Insn::fits(16, 4, false));
        assert!(Insn::fits(31, 5, false));
        assert!(!Insn::fits(32, 5, false));
        assert!(Insn::fits(63, 6, false));
        assert!(!Insn::fits(64, 6, false));
        assert!(Insn::fits(4095, 12, false));
        assert!(!Insn::fits(4096, 12, false));
        assert!(Insn::fits((1 << 24) - 1, 24, false));
        assert!(!Insn::fits(1 << 24, 24, false));
        assert!(Insn::fits(-8, 4, true));
        assert!(!Insn::fits(-9, 4, true));
    }

    #[test]
    fn u32_operand()
    {
        let insn = Insn::get_global(3, u32::MAX);
        assert_eq!(get_global::decode(insn), get_global { dst: 3, idx: u32::MAX });
    }

    #[test]
    fn opcode_metadata()
    {
        assert_eq!(Opcode::add.num_opnds(), 3);
        assert_eq!(Opcode::add.num_outs(), 1);
        assert_eq!(Opcode::jlt.num_opnds(), 3);
        assert_eq!(Opcode::jlt.num_outs(), 0);
        assert_eq!(Opcode::ret_nil.num_opnds(), 0);
    }

    #[test]
    fn opcode_names()
    {
        assert_eq!(Opcode::from_str("add"), Some(Opcode::add));
        assert_eq!(Opcode::from_str("nope"), None);
        assert_eq!(Opcode::add.name(), "add");
    }

    #[test]
    fn disassembly()
    {
        assert_eq!(Insn::add(1, 2, 3).to_string(), "add r1, r2, r3");
        assert_eq!(Insn::add_imm16(1, 2, -3).to_string(), "add_imm16 r1, r2, -3");
        assert_eq!(Insn::jlt(4, 5, -6).to_string(), "jlt r4, r5, -6");
        assert_eq!(Insn::ret_nil().to_string(), "ret_nil");
        assert_eq!(Insn::call_host(7, 8, 9).to_string(), "call_host 7, r8, 9");
    }
}
