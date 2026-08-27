#![allow(dead_code)]
use std::mem::transmute;

// Helper macro to define instruction opcodes and associated metadata
macro_rules! def_opcodes {
    (
        $(
            $(#[doc = $doc:expr])*
            $name:ident {
                $(op = $val:expr,)?
                num_opnds = $num_opnds:expr
                $(, num_outs = $num_outs:expr)?
                $(,)?
            },
        )*
    ) => {
        pub const NUM_OPCODES: usize = 0 $( + { let _ = stringify!($name); 1 } )*;

        /// Instruction opcodes
        #[allow(non_camel_case_types)]
        #[derive(PartialEq, Copy, Clone, Debug)]
        #[repr(u16)]
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
                        Opcode::$name => $num_opnds,
                    )*
                }
            }

            pub fn num_outs(self) -> usize
            {
                match self {
                    $(
                        Opcode::$name => 0 $(+ $num_outs)?,
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
//   - Future tail calls can just mutate the frame in-place
def_opcodes! {
    // Halt execution and produce an error
    // Panic is zero so that jumping to uninitialized memory causes panic
    panic { op = 0, num_opnds = 0 },

    // Debugger breakpoint
    breakpoint { num_opnds = 0 },

    // Can be used for code patching, or to disable breakpoints
    nop { num_opnds = 0 },

    // Load a sign-extended immediate into a register as the raw bits of a value.
    // Covers nil, undef, booleans and fixnums in the +/- 2^29 range
    // load_imm32 | dst_reg: u16 | imm: i32
    load_imm32 { num_opnds = 2, num_outs = 1 },

    // Copy a value from one register to another
    // mov | dst_reg: u16 | src_reg: u16
    mov { num_opnds = 2, num_outs = 1 },

    // Load a constant from a constant slot into a register
    // load_const | dst_reg: u16 | slot_idx: u16
    load_const { num_opnds = 2, num_outs = 1 },

    // Note: we might want to be able to use a constant slot for the base?
    // load_uxx | dst_reg: u16 | base_reg: u16 | offset: u16
    load_u8 { num_opnds = 3, num_outs = 1 },
    load_u16 { num_opnds = 3, num_outs = 1 },
    load_u32 { num_opnds = 3, num_outs = 1 },
    load_u64 { num_opnds = 3, num_outs = 1 },

    // Store is encoded the same way as load for simplicity
    // store_uxx | src_reg: u16 | base_reg: u16 | offset: u16
    store_u8 { num_opnds = 3 },
    store_u16 { num_opnds = 3 },
    store_u32 { num_opnds = 3 },
    store_u64 { num_opnds = 3 },

    // bitwise operations
    //not,
    lshift { num_opnds = 3, num_outs = 1 },
    rshift { num_opnds = 3, num_outs = 1 },
    and { num_opnds = 3, num_outs = 1 },
    or { num_opnds = 3, num_outs = 1 },
    xor { num_opnds = 3, num_outs = 1 },

    // op | dst_reg | reg0 | reg1
    add { num_opnds = 3, num_outs = 1 },
    sub { num_opnds = 3, num_outs = 1 },
    mul { num_opnds = 3, num_outs = 1 },
    div { num_opnds = 3, num_outs = 1 },
    modulo { num_opnds = 3, num_outs = 1 },

    // op | dst_reg:u16 | reg0:u16 | imm:i16
    add_imm16 { num_opnds = 3, num_outs = 1 },
    sub_imm16 { num_opnds = 3, num_outs = 1 },
    mul_imm16 { num_opnds = 3, num_outs = 1 },

    // jxx | r0: u16 | r1: u16 | offset: i16
    jeq { num_opnds = 3 },
    jne { num_opnds = 3 },
    jlt { num_opnds = 3 },
    jle { num_opnds = 3 },
    jgt { num_opnds = 3 },
    jge { num_opnds = 3 },

    // Unconditional jump
    // jmp | offset: i32
    jmp { num_opnds = 1 },

    // call | slot_idx: u16 | start_reg: u16 | num_args: u8
    call { num_opnds = 3 },

    // call_host | host_fn_id: u16 | start_reg: u16 | num_args: u8
    call_host { num_opnds = 3 },

    // Return the value in a register
    // ret | src_reg: u16
    ret { num_opnds = 1 },

    // Return nil, as functions with no explicit return value do
    ret_nil { num_opnds = 0 },

    // Return a sign-extended immediate as the raw bits of a value.
    // Covers nil, undef, booleans and fixnums in the +/- 2^29 range
    // ret_imm32 | imm: i32
    ret_imm32 { num_opnds = 1 },
}

/// Interpreter instruction
#[derive(PartialEq, Copy, Clone, Debug)]
pub struct Insn(u64);

impl Insn
{
    #[inline(always)]
    pub fn opcode(&self) -> Opcode
    {
        let Insn(word) = self;
        unsafe { transmute(*word as u16) }
    }

    /// Get a register operand
    #[inline(always)]
    pub fn reg(&self, idx: u16) -> u16
    {
        assert!(idx < 3);
        let Insn(word) = self;
        let shift = (idx + 1) * 16;
        ((word >> shift) & 0xFFFF) as u16
    }

    /// Get an immediate i32 value at the end of the instruction word
    #[inline(always)]
    pub fn imm_i32(&self) -> i32
    {
        (self.0 >> 32) as i32
    }

    /// Get an immediate i16 value at the end of the instruction word
    #[inline(always)]
    pub fn imm_i16(&self) -> i16
    {
        (self.0 >> 48) as i16
    }

    /// Get an immediate u16 value at the end of the instruction word
    #[inline(always)]
    pub fn imm_u16(&self) -> u16
    {
        (self.0 >> 48) as u16
    }

    /// Get the full u64 instruction word
    #[inline(always)]
    pub fn word_u64(&self) -> u64
    {
        let Insn(word) = self;
        *word
    }

    pub fn encode_op(op: Opcode) -> Self
    {
        Insn(op as u64)
    }

    /// Encode an instruction with a i32 operand
    pub fn encode_i32(op: Opcode, imm: i32) -> Self
    {
        Insn(
            (op as u64) |
            (imm as u64) << 32
        )
    }

    /// Encode an instruction with a single register operand
    pub fn encode_1reg(op: Opcode, r0: u16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16
        )
    }

    /// Encode an instruction one register and one imm32 operand
    pub fn encode_reg_i32(op: Opcode, r0: u16, imm: i32) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (imm as u64) << 32
        )
    }

    /// Encode an instruction one register and one u16 operand
    pub fn encode_reg_u16(op: Opcode, r0: u16, imm: u16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (imm as u64) << 48
        )
    }

    /// Encode an instruction with 3 register operands
    pub fn encode_3reg(op: Opcode, r0: u16, r1: u16, r2: u16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (r1 as u64) << 32 |
            (r2 as u64) << 48
        )
    }

    /// Encode an instruction with 2 register operands
    pub fn encode_2reg(op: Opcode, r0: u16, r1: u16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (r1 as u64) << 32
        )
    }

    /// Encode an instruction with 2 register operands and one i16 imm
    pub fn encode_2reg_i16(op: Opcode, r0: u16, r1: u16, imm: i16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (r1 as u64) << 32 |
            (imm as u64) << 48
        )
    }

    /// Encode an instruction with 2 register operands and one u16 imm
    pub fn encode_2reg_u16(op: Opcode, r0: u16, r1: u16, imm: u16) -> Self
    {
        Insn(
            (op as u64) |
            (r0 as u64) << 16 |
            (r1 as u64) << 32 |
            (imm as u64) << 48
        )
    }

    // call | slot_idx: u16 | start_reg: u16 | num_args: u8
    pub fn encode_call(op: Opcode, slot_idx: u16, start_reg: u16, num_args: u8) -> Self
    {
        Insn(
            (op as u64) |
            (slot_idx as u64) << 16 |
            (start_reg as u64) << 32 |
            (num_args as u64) << 48
        )
    }
}
