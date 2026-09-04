use std::mem::size_of;
use crate::alloc::HEADER_SIZE;
use crate::ast::*;
use crate::insns::Insn;
use crate::lexer::{ParseError, SrcPos};
use crate::str::Str;
use crate::symbols::Decl;
use crate::value::Value;
use crate::vm::Actor;

/// State that lives for as long as one function is being compiled.
///
/// The actor owns the instruction stream being appended to; everything
/// else here is codegen's own bookkeeping and is gone once the function
/// is compiled
struct CodeGen<'a>
{
    actor: &'a mut Actor,

    /// Position stamped onto the instructions being emitted. An
    /// expression sets this on the way in and puts back what it found on
    /// the way out, so the instructions a parent emits after its children
    /// are attributed to the parent and not to the last child
    cur_pos: SrcPos,
}

impl<'a> CodeGen<'a>
{
    fn new(actor: &'a mut Actor) -> Self
    {
        Self { actor, cur_pos: SrcPos::default() }
    }

    /// Emit an instruction, tagged with the position being generated for
    fn push_insn(&mut self, insn: Insn)
    {
        self.actor.push_insn(insn, self.cur_pos);
    }
}

/// Compiled function object
#[derive(Copy, Clone)]
pub struct CompiledFun
{
    pub entry_pc: usize,

    /// One past the last instruction. Every function compiles into the
    /// same instruction array, so this is what bounds one of them
    pub end_pc: usize,

    pub num_params: usize,

    /// Registers the frame occupies. Arguments come first, then locals,
    /// then the temporaries an expression needs while it is evaluated
    pub frame_size: usize,
}

/// Assigns registers within one function's frame.
///
/// Arguments occupy `r0..num_params`, locals the registers just above
/// them, and temporaries everything above that. Temporaries are handed
/// out and given back in stack order, so a caller notes the top before
/// generating subexpressions and frees back to it once it has consumed
/// their results.
struct Regs
{
    /// First register a temporary can use
    temp_base: u16,

    /// Next free temporary
    next: u16,

    /// High water mark, which is the frame size the function needs
    max: u16,
}

impl Regs
{
    /// Start a function, with the temporaries placed above its variables
    fn new(fun: &Function) -> Self
    {
        let temp_base: u16 = (fun.params.len() + fun.num_locals)
            .try_into()
            .expect("function has too many arguments and locals");

        // A frame always has room for r0: a call writes its return value
        // there, and a constructor reads self from it
        Self { temp_base, next: temp_base, max: std::cmp::max(temp_base, 1) }
    }

    /// First free register, which `free_to` takes back down to
    fn top(&self) -> u16
    {
        self.next
    }

    /// Give back every temporary handed out since `top` was taken
    fn free_to(&mut self, top: u16)
    {
        debug_assert!(top >= self.temp_base);
        self.next = top;
    }

    /// Reserve one register to hold an intermediate value
    fn alloc(&mut self) -> u16
    {
        self.alloc_n(1)
    }

    /// Reserve a run of consecutive registers, as a call's arguments need
    fn alloc_n(&mut self, n: u16) -> u16
    {
        let reg = self.next;
        self.next = self.next.checked_add(n).expect("frame is too large");
        self.max = std::cmp::max(self.max, self.next);
        reg
    }

    /// Reserve a run starting at a register that is already handed out, so
    /// that a call can put its frame there. Only the registers above it are
    /// taken, so nothing living in them may still be needed
    fn alloc_from(&mut self, start: u16, n: u16)
    {
        debug_assert!(start <= self.next);
        let end = start.checked_add(n).expect("frame is too large");
        self.next = std::cmp::max(self.next, end);
        self.max = std::cmp::max(self.max, self.next);
    }
}

/// Register an argument or local variable lives in
fn decl_reg(decl: &Decl, fun: &Function) -> u16
{
    match *decl {
        Decl::Arg { idx, .. } => idx as u16,
        Decl::Local { idx, .. } => fun.params.len() as u16 + idx as u16,
        _ => panic!("declaration does not live in a register")
    }
}

/// Emit an instruction and return its index, for later patching
fn emit(cg: &mut CodeGen, insn: Insn) -> usize
{
    cg.push_insn(insn);
    cg.actor.insns.len() - 1
}

/// Point a previously emitted jump at an instruction index. Displacements
/// count instructions from the one after the jump
fn patch_jump(cg: &mut CodeGen, jmp_idx: usize, dst_idx: usize)
{
    let disp = (dst_idx as i64) - (jmp_idx as i64) - 1;
    let disp = disp.try_into().expect("jump is out of range");
    cg.actor.insns[jmp_idx] = cg.actor.insns[jmp_idx].with_branch_disp(disp);
}

/// Point a set of jumps, as a test can produce more than one, at the same
/// instruction index
fn patch_jumps(cg: &mut CodeGen, jmp_idxs: &[usize], dst_idx: usize)
{
    for jmp_idx in jmp_idxs {
        patch_jump(cg, *jmp_idx, dst_idx);
    }
}

/// Load a value that needs no heap allocation. Immediates that fit the
/// instruction go inline, the rest through the constant pool
fn gen_value(cg: &mut CodeGen, dst: u16, val: Value)
{
    let raw = val.raw_bits();

    if fits_imm40(val) {
        emit(cg, Insn::load_imm40(dst, raw as i64));
    } else {
        let slot = cg.actor.push_const(val);
        emit(cg, Insn::load_const(dst, slot));
    }
}

/// Whether a value can be carried in an instruction's immediate field.
///
/// A heap pointer never can, whatever its bits happen to be. The
/// collector does not walk the instruction stream, so a pointer baked
/// into one would not be updated when the object moves. Heap values go
/// through the constant pool, which is a root the collector does trace.
///
/// Otherwise the field sign-extends, so a value round-trips only if its
/// top bits are already the sign extension of the low 40
fn fits_imm40(val: Value) -> bool
{
    if val.is_heap() {
        return false;
    }

    let raw = val.raw_bits();
    (((raw << 24) as i64 >> 24) as u64) == raw
}

/// The value an expression is known to hold at the time it is compiled.
///
/// Callers that need the value to fit in an instruction check that
/// themselves, since whether it has to is a property of what they are
/// emitting rather than of the expression
fn const_value(expr: &ExprBox, actor: &Actor) -> Option<Value>
{
    match expr.expr.as_ref() {
        Expr::Nil => Some(Value::NIL),
        Expr::True => Some(Value::TRUE),
        Expr::False => Some(Value::FALSE),
        Expr::Int64(v) => Value::try_fixnum(*v),
        Expr::Float64(v) => Value::try_flonum(*v),

        // Codegen runs when a function is first called, so a global the
        // unit has already initialized holds the value the compiled code
        // will see. A mutable one can change after this point, so only
        // immutable globals are constants
        Expr::Ref { decl: Decl::Global { idx, mutable: false }, .. } => {
            actor.global_value(*idx)
        }

        _ => None
    }
}

/// Load a heap constant, which always goes through the pool
fn gen_const(cg: &mut CodeGen, dst: u16, val: Value)
{
    let slot = cg.actor.push_const(val);
    emit(cg, Insn::load_const(dst, slot));
}

impl Function
{
    fn needs_final_return(&self) -> bool
    {
        if let Stmt::Block(stmts) = &self.body.stmt.as_ref() {
            if stmts.len() > 0 {
                let last_stmt = &stmts[stmts.len() - 1];

                if let Stmt::Return(_) = last_stmt.stmt.as_ref() {
                    return false;
                }
            }
        }

        return true;
    }

    pub fn gen_code(
        &self,
        actor: &mut Actor,
    ) -> Result<CompiledFun, ParseError>
    {
        // The context lives exactly as long as this one compilation, so
        // nothing codegen tracks can outlive it or be reached from the VM
        let cg = &mut CodeGen::new(actor);

        // Entry address of the compiled function
        let entry_pc = cg.actor.insns.len();

        cg.cur_pos = self.pos;

        let mut regs = Regs::new(self);

        // Compile the function body
        self.body.gen_code(self, &mut regs, &mut vec![], &mut vec![], cg)?;

        // If the body needs a final return
        if self.needs_final_return() {
            if self.is_ctor() {
                // A constructor hands back the object it was called on,
                // which is its first argument
                cg.push_insn(Insn::ret(0));
            } else {
                cg.push_insn(Insn::ret_imm40(Value::NIL.raw_bits() as i64));
            }
        }

        Ok(CompiledFun {
            entry_pc,
            end_pc: cg.actor.insns.len(),
            num_params: self.params.len(),
            frame_size: regs.max as usize,
        })
    }
}

impl StmtBox
{
    fn gen_code(
        &self,
        fun: &Function,
        regs: &mut Regs,
        break_idxs: &mut Vec<usize>,
        cont_idxs: &mut Vec<usize>,
        cg: &mut CodeGen,
    ) -> Result<(), ParseError>
    {
        let prev_pos = cg.cur_pos;
        cg.cur_pos = self.pos;

        let result = self.gen_code_inner(fun, regs, break_idxs, cont_idxs, cg);

        cg.cur_pos = prev_pos;
        result
    }

    fn gen_code_inner(
        &self,
        fun: &Function,
        regs: &mut Regs,
        break_idxs: &mut Vec<usize>,
        cont_idxs: &mut Vec<usize>,
        cg: &mut CodeGen,
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_ref() {
            Stmt::Expr(expr) => {
                let top = regs.top();

                match expr.expr.as_ref() {
                    // An omitted clause, as a while loop has in place of
                    // the init and increment a for loop would carry
                    Expr::Nil => {}

                    // For assignment expressions as statements, the
                    // assigned value itself is not needed
                    Expr::Binary { op: BinOp::Assign, lhs, rhs } => {
                        gen_assign(lhs, rhs, fun, regs, cg, false)?;
                    }

                    _ => {
                        expr.gen_code(fun, regs, cg, None)?;
                    }
                }

                regs.free_to(top);
            }

            Stmt::Break => {
                break_idxs.push(emit(cg, Insn::jmp(0)));
            }

            Stmt::Continue => {
                cont_idxs.push(emit(cg, Insn::jmp(0)));
            }

            Stmt::Return(expr) => {
                // Returning a constant carries it in the instruction,
                // rather than loading it into a register first
                match const_value(expr, cg.actor).filter(|val| fits_imm40(*val)) {
                    Some(val) => {
                        cg.push_insn(Insn::ret_imm40(val.raw_bits() as i64));
                    }

                    None => {
                        let top = regs.top();
                        let src = expr.gen_code(fun, regs, cg, None)?;
                        cg.push_insn(Insn::ret(src));
                        regs.free_to(top);
                    }
                }
            }

            Stmt::Block(stmts) => {
                // Closures are created before the block runs, so that they
                // can capture each other
                if !fun.is_unit {
                    // For each closure declaration
                    for stmt in stmts {
                        if let Stmt::Let { init_expr, decl, .. } = stmt.stmt.as_ref() {
                            if let Expr::Fun { fun_id, captured } = init_expr.expr.as_ref() {
                                let top = regs.top();
                                let decl = decl.as_ref().unwrap();

                                // Create the closure. The slots are filled
                                // in by the Let below
                                let clos = write_target(decl, fun, regs);
                                cg.push_insn(Insn::clos_new(
                                    clos,
                                    usize::from(*fun_id) as u32,
                                    captured.len().try_into().unwrap(),
                                ));
                                // Initialize the local variable for this closure
                                gen_var_write(decl, fun, regs, cg, clos);

                                regs.free_to(top);
                            }
                        }
                    }
                }

                for stmt in stmts {
                    stmt.gen_code(fun, regs, break_idxs, cont_idxs, cg)?;
                }
            }

            Stmt::If { test_expr, then_stmt, else_stmt } => {
                // If false, jump to else stmt
                let if_idxs = gen_branch(test_expr, fun, regs, cg, false)?;

                if else_stmt.is_some() {
                    then_stmt.gen_code(fun, regs, break_idxs, cont_idxs, cg)?;
                    let jump_idx = emit(cg, Insn::jmp(0));

                    // Patch the test to jump to the else clause
                    let dst_idx = cg.actor.insns.len();
                    patch_jumps(cg, &if_idxs, dst_idx);

                    else_stmt.as_ref().unwrap().gen_code(fun, regs, break_idxs, cont_idxs, cg)?;

                    // Patch the jump instruction to jump after the else clause
                    let dst_idx = cg.actor.insns.len();
                    patch_jump(cg, jump_idx, dst_idx);
                }
                else
                {
                    then_stmt.gen_code(fun, regs, break_idxs, cont_idxs, cg)?;

                    let dst_idx = cg.actor.insns.len();
                    patch_jumps(cg, &if_idxs, dst_idx);
                }
            }

            Stmt::For { init_stmt, test_expr, incr_expr, body_stmt } => {
                // Generate code for the init statement
                init_stmt.gen_code(fun, regs, break_idxs, cont_idxs, cg)?;

                let mut break_idxs = Vec::new();
                let mut cont_idxs = Vec::new();

                // The test sits at the bottom of the loop, so a continuing
                // iteration takes one branch rather than a test and a jump
                // back. Entry jumps down to it, which also means the test
                // still runs before the body the first time round.
                //
                // Testing at the bottom asks whether to go round again,
                // which is the sense a comparison already branches on, so
                // the loop needs no negated test
                let entry_idx = emit(cg, Insn::jmp(0));
                let body_idx = cg.actor.insns.len();

                body_stmt.gen_code(fun, regs, &mut break_idxs, &mut cont_idxs, cg)?;

                // Continue will jump here
                let cont_idx = cg.actor.insns.len();

                // Evaluate the increment expression, which a while loop
                // does not have
                if !matches!(incr_expr.expr.as_ref(), Expr::Nil) {
                    let top = regs.top();
                    incr_expr.gen_code(fun, regs, cg, None)?;
                    regs.free_to(top);
                }

                // Evaluate the test, and go round again if it holds
                let test_idx = cg.actor.insns.len();
                patch_jump(cg, entry_idx, test_idx);

                let back_idxs = gen_branch(test_expr, fun, regs, cg, true)?;
                patch_jumps(cg, &back_idxs, body_idx);

                // Break will jump here
                let break_idx = cg.actor.insns.len();

                patch_jumps(cg, &cont_idxs, cont_idx);
                patch_jumps(cg, &break_idxs, break_idx);
            }

            Stmt::Assert { test_expr } => {
                let if_idxs = gen_branch(test_expr, fun, regs, cg, true)?;

                cg.push_insn(Insn::panic());

                let dst_idx = cg.actor.insns.len();
                patch_jumps(cg, &if_idxs, dst_idx);
            }

            // Variable declaration
            Stmt::Let { mutable: _, var_name: _, init_expr, decl } => {
                // Nothing to do for top-level functions
                if let Some(Decl::Fun { .. }) = decl {
                    return Ok(())
                }

                let decl = decl.as_ref().unwrap();
                let top = regs.top();

                match init_expr.expr.as_ref() {
                    Expr::Fun { fun_id: _, captured } => {
                        // Read the closure decl. The closure itself was
                        // created before the block, so this only fills in
                        // what it captures
                        let clos = gen_var_read(decl, fun, regs, cg, None);
                        gen_captures(captured, clos, fun, regs, cg);
                    }

                    _ => {
                        // An escaping variable holds a cell, and the cell
                        // is created below, so the value cannot be written
                        // straight into the variable's own register
                        let dst = if fun.escaping.contains(decl) {
                            None
                        } else {
                            Some(write_target(decl, fun, regs))
                        };

                        let src = init_expr.gen_code(fun, regs, cg, dst)?;

                        // If this is an escaping mutable variable
                        if fun.escaping.contains(decl) {
                            // Allocate a mutable closure cell for this variable
                            let cell = decl_reg(decl, fun);
                            cg.push_insn(Insn::cell_new(cell));
                            cg.push_insn(Insn::cell_set(cell, src));
                        } else {
                            // Initialize the local variable
                            gen_var_write(decl, fun, regs, cg, src);
                        }
                    }
                }

                regs.free_to(top);
            }

            Stmt::ClassDecl { .. } => {}
        }

        Ok(())
    }
}

impl ExprBox
{
    /// Generate code for an expression and return the register holding
    /// its value.
    ///
    /// `dst` is a hint: an expression that has to write somewhere writes
    /// there, but one whose value already sits in a register hands that
    /// register back untouched. Callers that need the value in a specific
    /// register go through `gen_into`.
    fn gen_code(
        &self,
        fun: &Function,
        regs: &mut Regs,
        cg: &mut CodeGen,
        dst: Option<u16>,
    ) -> Result<u16, ParseError>
    {
        // Stamp this expression's position onto the instructions it
        // generates, and put back the one the caller was using on the way
        // out, so that an instruction the caller emits after this
        // subexpression still points at the caller. Several cases below
        // return early, which is why this wraps them rather than sitting
        // inside the body
        let prev_pos = cg.cur_pos;
        cg.cur_pos = self.pos;

        let result = self.gen_code_inner(fun, regs, cg, dst);

        cg.cur_pos = prev_pos;
        result
    }

    fn gen_code_inner(
        &self,
        fun: &Function,
        regs: &mut Regs,
        cg: &mut CodeGen,
        dst: Option<u16>,
    ) -> Result<u16, ParseError>
    {
        // Register to write into, for the cases that produce a new value
        macro_rules! out {
            () => { match dst { Some(dst) => dst, None => regs.alloc() } }
        }

        let reg = match self.expr.as_ref() {
            Expr::Nil => { let d = out!(); gen_value(cg, d, Value::NIL); d }
            Expr::True => { let d = out!(); gen_value(cg, d, Value::TRUE); d }
            Expr::False => { let d = out!(); gen_value(cg, d, Value::FALSE); d }

            Expr::HostFn(f) => {
                let d = out!();
                gen_value(cg, d, Value::host_fn(*f));
                d
            }

            // Constants that don't fit in an immediate are boxed in the
            // heap the code is compiled for, and held by the constant
            // pool, which is what the collector traces them from
            Expr::Int64(v) => {
                let d = out!();

                match Value::try_fixnum(*v) {
                    Some(val) => gen_value(cg, d, val),
                    None => {
                        cg.actor.gc_check(HEADER_SIZE + size_of::<i64>(), &mut []);
                        let val = cg.actor.alloc.heap_int64(*v);
                        gen_const(cg, d, val);
                    }
                }

                d
            }

            Expr::Float64(v) => {
                let d = out!();

                match Value::try_flonum(*v) {
                    Some(val) => gen_value(cg, d, val),
                    None => {
                        cg.actor.gc_check(HEADER_SIZE + size_of::<f64>(), &mut []);
                        let val = cg.actor.alloc.heap_float64(*v);
                        gen_const(cg, d, val);
                    }
                }

                d
            }

            Expr::String(s) => {
                let d = out!();
                cg.actor.gc_check(Str::alloc_size(s.len()), &mut []);
                let val = Str::new(&s, &mut cg.actor.alloc);
                gen_const(cg, d, val);
                d
            }

            Expr::ByteArray(bytes) => {
                use crate::bytearray::ByteArray;
                let d = out!();
                cg.actor.gc_check(ByteArray::alloc_size(bytes.len()), &mut []);
                let ba = ByteArray::with_size(bytes.len(), &mut cg.actor.alloc);
                unsafe { ba.as_ba().get_slice_mut(0, bytes.len()).copy_from_slice(&bytes) };
                gen_const(cg, d, ba);
                cg.push_insn(Insn::ba_clone(d, d));
                d
            }

            Expr::Array { exprs } => {
                let d = out!();
                cg.push_insn(Insn::arr_new(d, exprs.len() as u32));

                for expr in exprs {
                    let top = regs.top();
                    let val = expr.gen_code(fun, regs, cg, None)?;
                    cg.push_insn(Insn::arr_push(d, val));
                    regs.free_to(top);
                }

                d
            }

            Expr::Dict { pairs } => {
                let d = out!();
                cg.push_insn(Insn::dict_new(d));

                // For each field
                for (name, expr) in pairs {
                    let top = regs.top();
                    let val = expr.gen_code(fun, regs, cg, None)?;
                    let cache = cg.actor.new_prop_cache(name);
                    cg.push_insn(Insn::set_field(d, val, cache));
                    regs.free_to(top);
                }

                d
            }

            Expr::Ref { decl, .. } => {
                gen_var_read(decl, fun, regs, cg, dst)
            }

            Expr::Index { base, index } => {
                let top = regs.top();
                let arr = base.gen_code(fun, regs, cg, None)?;
                let idx = index.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                let d = out!();
                cg.push_insn(Insn::get_index(d, arr, idx));
                d
            }

            Expr::Member { base, field } => {
                let top = regs.top();
                let obj = base.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                let d = out!();
                let cache = cg.actor.new_prop_cache(field);
                cg.push_insn(Insn::get_field(d, obj, cache));
                d
            }

            Expr::InstanceOf { val, class_id, .. } => {
                let top = regs.top();
                let v = val.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                let d = out!();
                cg.push_insn(Insn::instanceof(d, v, usize::from(*class_id) as u32));
                d
            }

            Expr::Unary { op, child } => {
                let top = regs.top();
                let src = child.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                let d = out!();

                match op {
                    // Negation is a multiply, which keeps the numeric
                    // tower in one place
                    UnOp::Minus => cg.push_insn(Insn::mul_imm16(d, src, -1)),

                    // Logical negation
                    UnOp::Not => cg.push_insn(Insn::not(d, src)),
                }

                d
            }

            Expr::Binary { op, lhs, rhs } => {
                // A condition used as a value is built from the branch it
                // would compile to, so that the two shapes agree
                if is_cond_op(op) {
                    return gen_cond_value(self, fun, regs, cg, dst);
                }

                return gen_bin_op(op, lhs, rhs, fun, regs, cg, dst);
            }

            Expr::Ternary { test_expr, then_expr, else_expr } => {
                // Both arms have to land in the same register
                let d = out!();

                let if_idxs = gen_branch(test_expr, fun, regs, cg, false)?;

                // Evaluate the then expression
                gen_into(then_expr, fun, regs, cg, d)?;
                let jump_idx = emit(cg, Insn::jmp(0));

                // Patch the test to jump to the else clause
                let dst_idx = cg.actor.insns.len();
                patch_jumps(cg, &if_idxs, dst_idx);

                // Evaluate the else expression
                gen_into(else_expr, fun, regs, cg, d)?;

                // Patch the jump over the else expression
                let dst_idx = cg.actor.insns.len();
                patch_jump(cg, jump_idx, dst_idx);

                d
            }

            Expr::Call { callee, args } => {
                return gen_call(callee, args, fun, regs, cg, None);
            }

            // Function expression
            Expr::Fun { fun_id, captured } => {
                let d = out!();

                // If this is not a closure
                if captured.len() == 0 {
                    gen_value(cg, d, Value::fun(*fun_id));
                    return Ok(d);
                }

                cg.push_insn(Insn::clos_new(
                    d,
                    usize::from(*fun_id) as u32,
                    captured.len().try_into().unwrap(),
                ));
                gen_captures(captured, d, fun, regs, cg);
                d
            }

            _ => todo!("{:?}", self)
        };

        Ok(reg)
    }
}

/// Whether an operator produces a condition, which is generated as a
/// branch and only turned back into a value where one is needed
fn is_cond_op(op: &BinOp) -> bool
{
    use BinOp::*;
    matches!(op, And | Or | Eq | Ne | Lt | Le | Gt | Ge)
}

/// Materialize a condition as a boolean value. The branch it compiles to
/// is what decides the result, so a comparison costs no more here than it
/// does as a test
fn gen_cond_value(
    expr: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    dst: Option<u16>,
) -> Result<u16, ParseError>
{
    let d = match dst { Some(dst) => dst, None => regs.alloc() };

    let jumps = gen_branch(expr, fun, regs, cg, true)?;
    gen_value(cg, d, Value::FALSE);
    let jmp_idx = emit(cg, Insn::jmp(0));

    // The branch lands on the true case
    let dst_idx = cg.actor.insns.len();
    patch_jumps(cg, &jumps, dst_idx);
    gen_value(cg, d, Value::TRUE);

    let dst_idx = cg.actor.insns.len();
    patch_jump(cg, jmp_idx, dst_idx);

    Ok(d)
}

/// Emit a test that transfers control when `expr` evaluates to
/// `jump_if_true`, and return the jumps that have to be patched to the
/// target. A comparison compiles straight into a conditional branch here,
/// rather than being materialized into a boolean that is then tested.
fn gen_branch(
    expr: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    jump_if_true: bool,
) -> Result<Vec<usize>, ParseError>
{
    match expr.expr.as_ref() {
        // Negation folds into the sense of the branch
        Expr::Unary { op: UnOp::Not, child } => {
            return gen_branch(child, fun, regs, cg, !jump_if_true);
        }

        // Short-circuiting operators branch directly, with no boolean
        // in between. One of the two senses needs the first test to skip
        // over the second, which is what the local patch below is for
        Expr::Binary { op: BinOp::And, lhs, rhs } => {
            if jump_if_true {
                // Jump only if both hold
                let skip = gen_branch(lhs, fun, regs, cg, false)?;
                let jumps = gen_branch(rhs, fun, regs, cg, true)?;

                let dst_idx = cg.actor.insns.len();
                patch_jumps(cg, &skip, dst_idx);
                return Ok(jumps);
            }

            // Jump if either fails
            let mut jumps = gen_branch(lhs, fun, regs, cg, false)?;
            jumps.append(&mut gen_branch(rhs, fun, regs, cg, false)?);
            return Ok(jumps);
        }

        Expr::Binary { op: BinOp::Or, lhs, rhs } => {
            if jump_if_true {
                // Jump if either holds
                let mut jumps = gen_branch(lhs, fun, regs, cg, true)?;
                jumps.append(&mut gen_branch(rhs, fun, regs, cg, true)?);
                return Ok(jumps);
            }

            // Jump only if both fail
            let skip = gen_branch(lhs, fun, regs, cg, true)?;
            let jumps = gen_branch(rhs, fun, regs, cg, false)?;

            let dst_idx = cg.actor.insns.len();
            patch_jumps(cg, &skip, dst_idx);
            return Ok(jumps);
        }

        Expr::Binary { op, lhs, rhs } if cmp_branch_insn(op, jump_if_true).is_some() => {
            // An integer literal small enough to encode goes in the
            // instruction rather than a register. A literal on the left is
            // mirrored to the right, which is what swapping the comparison
            // around does. Literals have no side effects, so dropping one
            // leaves the order the other operand is evaluated in untouched
            let imm_form = if let Some(imm) = as_imm16(rhs) {
                Some((*op, lhs, imm))
            } else if let Some(imm) = as_imm16(lhs) {
                Some((mirror_cmp(op), rhs, imm))
            } else {
                None
            };

            if let Some((imm_op, tested, imm)) = imm_form {
                let top = regs.top();
                let a = tested.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                // The guard accepted this operator and mirroring keeps it
                // in the same set, so an immediate form exists for it
                let insn = cmp_branch_imm(&imm_op, jump_if_true, a, imm).unwrap();
                return Ok(vec![emit(cg, insn)]);
            }

            let top = regs.top();
            let a = lhs.gen_code(fun, regs, cg, None)?;
            let b = rhs.gen_code(fun, regs, cg, None)?;
            regs.free_to(top);

            let build = cmp_branch_insn(op, jump_if_true).unwrap();
            return Ok(vec![emit(cg, build(a, b, 0))]);
        }

        _ => {}
    }

    // Anything else is evaluated and tested as a boolean
    let top = regs.top();
    let test = expr.gen_code(fun, regs, cg, None)?;
    let insn = if jump_if_true { Insn::if_true(test, 0) } else { Insn::if_false(test, 0) };
    let idx = emit(cg, insn);
    regs.free_to(top);

    Ok(vec![idx])
}

/// The branch a comparison compiles to, for either sense of the test.
/// The constructor handed back takes the two registers to compare and the
/// branch displacement, which is patched in once the target is known.
///
/// NaN is unordered, so an ordered comparison and its opposite are both
/// false for it, and neither is the negation of the other. Each sense
/// therefore has its own instruction. Equality does invert exactly, NaN
/// included, so `!=` serves as the negation of `==`
fn cmp_branch_insn(op: &BinOp, jump_if_true: bool) -> Option<fn(u16, u16, i32) -> Insn>
{
    use BinOp::*;

    let insn: fn(u16, u16, i32) -> Insn = match (op, jump_if_true) {
        (Eq, true) | (Ne, false) => Insn::jeq,
        (Ne, true) | (Eq, false) => Insn::jne,

        (Lt, true) => Insn::jlt,
        (Le, true) => Insn::jle,
        (Gt, true) => Insn::jgt,
        (Ge, true) => Insn::jge,

        (Lt, false) => Insn::jnlt,
        (Le, false) => Insn::jnle,
        (Gt, false) => Insn::jngt,
        (Ge, false) => Insn::jnge,

        _ => return None,
    };

    Some(insn)
}

/// Compare register `reg_a` against the literal `imm` and branch, for
/// either sense of the test. Covers the same operators `cmp_branch_insn`
/// does.
///
/// That one hands back a constructor because the match guard calls it to
/// ask whether an operator branches at all, before there are any operands
/// to give it. Here the operands are in hand, so this builds the
/// instruction. The displacement is filled in later, when the jump is
/// patched to its target
fn cmp_branch_imm(op: &BinOp, jump_if_true: bool, reg_a: u16, imm: i16) -> Option<Insn>
{
    use BinOp::*;

    let insn = match (op, jump_if_true) {
        (Eq, true) | (Ne, false) => Insn::jeq_imm16(reg_a, imm, 0),
        (Ne, true) | (Eq, false) => Insn::jne_imm16(reg_a, imm, 0),

        (Lt, true) => Insn::jlt_imm16(reg_a, imm, 0),
        (Le, true) => Insn::jle_imm16(reg_a, imm, 0),
        (Gt, true) => Insn::jgt_imm16(reg_a, imm, 0),
        (Ge, true) => Insn::jge_imm16(reg_a, imm, 0),

        (Lt, false) => Insn::jnlt_imm16(reg_a, imm, 0),
        (Le, false) => Insn::jnle_imm16(reg_a, imm, 0),
        (Gt, false) => Insn::jngt_imm16(reg_a, imm, 0),
        (Ge, false) => Insn::jnge_imm16(reg_a, imm, 0),

        _ => return None,
    };

    Some(insn)
}

/// An integer literal that fits the immediate a compare and branch carries
fn as_imm16(expr: &ExprBox) -> Option<i16>
{
    match expr.expr.as_ref() {
        Expr::Int64(v) => i16::try_from(*v).ok(),
        _ => None,
    }
}

/// The comparison that holds when the operands are swapped, for a literal
/// sitting on the left of a compare and branch.
///
/// Swapping is exact even for NaN: `a < b` and `b > a` are both false when
/// either side is unordered. It is negating a comparison that NaN breaks,
/// which is what the jn* instructions are for. So this holds for any
/// operands, not just the integer literal it is used with here
fn mirror_cmp(op: &BinOp) -> BinOp
{
    use BinOp::*;

    match op {
        Lt => Gt,
        Gt => Lt,
        Le => Ge,
        Ge => Le,
        other => *other,
    }
}

/// Generate an expression into a specific register
fn gen_into(
    expr: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    dst: u16,
) -> Result<(), ParseError>
{
    let top = regs.top();
    let src = expr.gen_code(fun, regs, cg, Some(dst))?;

    if src != dst {
        cg.push_insn(Insn::mov(dst, src));
    }

    regs.free_to(top);
    Ok(())
}

/// Fill in the slots of a closure that has already been created
fn gen_captures(
    captured: &Vec<Decl>,
    clos: u16,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
)
{
    // For each variable captured by the closure
    for (idx, decl) in captured.iter().enumerate() {
        let top = regs.top();

        // Copy variables and cells captured by the closure.
        // A mutable local is captured by its cell, not by its value
        let src = match decl {
            Decl::Local { idx, mutable: true, .. } => fun.params.len() as u16 + *idx as u16,
            _ => gen_var_read(decl, fun, regs, cg, None)
        };

        cg.push_insn(Insn::clos_set(clos, src, idx.try_into().unwrap()));
        regs.free_to(top);
    }
}

/// Generate a call, whose arguments have to land in consecutive registers
/// starting at the callee's frame base. The return value comes back in
/// that same register
/// Generate a call. `window` names a register to start the callee's frame
/// at, which is how a call that is itself an operand of another call gets
/// its return value into the slot that call wants it in. It has to be a
/// register whose successors are all free, since the callee's frame runs
/// over them
fn gen_call(
    callee: &ExprBox,
    args: &Vec<ExprBox>,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    window: Option<u16>,
) -> Result<u16, ParseError>
{
    let n_args = args.len();

    // A method call and a constructor both pass self as argument zero. A
    // dynamic call parks the callee just above the arguments, where the
    // callee's own frame overwrites it once the call is under way
    let (argc, extra_slot): (usize, u16) = match callee.expr.as_ref() {
        Expr::Member { .. } => (n_args + 1, 0),
        Expr::Ref { decl: Decl::Class { .. }, .. } => (n_args + 1, 0),
        Expr::Ref { decl: Decl::Fun { .. }, .. } => (n_args, 0),
        Expr::HostFn(_) => (n_args, 0),
        _ => (n_args, 1),
    };

    // The count stays a usize until here so that adding the self argument
    // above can't overflow. This is the exact limit the parser can only
    // approximate, because whether a self is passed is known only here
    let argc: u8 = match argc.try_into() {
        Ok(argc) => argc,
        Err(_) => return ParseError::with_pos(
            "too many arguments in function call",
            &callee.pos
        )
    };

    // A call with no arguments still needs the register its return value
    // comes back in
    let top = regs.top();
    let n_regs = std::cmp::max(argc as u16 + extra_slot, 1);

    let start = match window {
        Some(reg) => { regs.alloc_from(reg, n_regs); reg }
        None => regs.alloc_n(n_regs),
    };

    match callee.expr.as_ref() {
        // New class instance. The object is created into the callee's
        // frame base and handed to the constructor as self
        Expr::Ref { decl: Decl::Class { id }, .. } => {
            // Evaluate the arguments
            for (i, arg) in args.iter().enumerate() {
                gen_call_opnd(arg, fun, regs, cg, start + 1 + i as u16)?;
            }

            cg.push_insn(Insn::new(usize::from(*id) as u32, start, argc));
        }

        // Callee has form a.b
        Expr::Member { base, field } => {
            // Evaluate the self argument
            gen_call_opnd(base, fun, regs, cg, start)?;

            for (i, arg) in args.iter().enumerate() {
                gen_call_opnd(arg, fun, regs, cg, start + 1 + i as u16)?;
            }

            // A static method on a core class is settled by the class
            // alone, and symbol resolution has already checked that it
            // exists and that its arity matches. A class is the one
            // receiver a call site never learns anything about, since it
            // is not an object and its methods don't follow from the type
            // tag, so the site would look the method up on every call
            let host_fn = match base.expr.as_ref() {
                Expr::Ref { decl: Decl::Class { id }, .. } =>
                    crate::runtime::get_method(Value::class(*id), field),
                _ => None
            };

            if let Some(host_fn) = host_fn {
                cg.push_insn(Insn::call_host(host_fn as u16, start, argc));
            } else {
                let cache = cg.actor.new_call_cache(field);
                cg.push_insn(Insn::call_method(start, argc, cache));
            }
        }

        // Call to a known function
        Expr::Ref { decl: Decl::Fun { id }, .. } => {
            for (i, arg) in args.iter().enumerate() {
                gen_call_opnd(arg, fun, regs, cg, start + i as u16)?;
            }

            cg.push_insn(Insn::call_direct(usize::from(*id) as u32, start, argc));
        }

        // Call to a host function, which the VM provides
        Expr::HostFn(host_fn) => {
            for (i, arg) in args.iter().enumerate() {
                gen_call_opnd(arg, fun, regs, cg, start + i as u16)?;
            }

            cg.push_insn(Insn::call_host(*host_fn as u16, start, argc));
        }

        // Plain regular call
        _ => {
            for (i, arg) in args.iter().enumerate() {
                gen_call_opnd(arg, fun, regs, cg, start + i as u16)?;
            }

            gen_call_opnd(callee, fun, regs, cg, start + argc as u16)?;

            let cache = cg.actor.new_call_cache("");
            cg.push_insn(Insn::call_opnd(start, argc, cache));
        }
    }

    // The return value comes back in the callee's frame base
    regs.free_to(top);

    // A frame placed in a caller's register sits below what this call had
    // to hand out, so there is nothing left to reserve for the result
    match window {
        Some(reg) => Ok(reg),
        None => Ok(regs.alloc()),
    }
}

/// Generate one operand of a call into the register it has to occupy.
///
/// An operand that is itself a call has its frame started at that register,
/// so its return value lands there rather than being copied down. Operands
/// are generated in order, so everything above the register belongs to an
/// operand that has not been evaluated yet and is free for the callee to
/// run over
fn gen_call_opnd(
    expr: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    dst: u16,
) -> Result<(), ParseError>
{
    if let Expr::Call { callee, args } = expr.expr.as_ref() {
        gen_call(callee, args, fun, regs, cg, Some(dst))?;
        return Ok(());
    }

    gen_into(expr, fun, regs, cg, dst)
}

fn gen_bin_op(
    op: &BinOp,
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    dst: Option<u16>,
) -> Result<u16, ParseError>
{
    use BinOp::*;

    // Assignments are different from other kinds of expressions
    // because we don't evaluate the lhs the same way
    if *op == Assign {
        return gen_assign(lhs, rhs, fun, regs, cg, true);
    }

    // A mask that is one run of set bits, as the position of its lowest
    // set bit and the length of the run. Runs that reach past what a
    // fixnum holds are rejected, so the mask never needs boxing
    fn bit_run(mask: i64) -> Option<(u8, u8)>
    {
        if mask <= 0 {
            return None;
        }

        let lo = mask.trailing_zeros();
        let len = (mask >> lo).trailing_ones();

        // Anything left above the run means the set bits are not contiguous
        if (mask >> lo) >> (len - 1) >> 1 != 0 || lo + len > 62 {
            return None;
        }

        Some((lo as u8, len as u8))
    }

    // And with a constant mask folds into the instruction when the mask is
    // a single run of bits, which is what every mask worth folding is
    if *op == BitAnd {
        let run = const_value(rhs, cg.actor)
            .filter(|v| v.is_fixnum())
            .and_then(|v| bit_run(v.as_fixnum()));

        if let Some((lo, len)) = run {
            // A constant shift feeding the mask folds in as well, which is
            // what pulling a field out of a packed word looks like
            let shifted = match lhs.expr.as_ref() {
                Expr::Binary { op: RShift, lhs: inner, rhs: amount } => {
                    const_value(amount, cg.actor)
                        .filter(|v| v.is_fixnum())
                        .map(|v| v.as_fixnum())
                        .filter(|sh| *sh >= 0 && *sh < 64)
                        .map(|sh| (inner, sh as u8))
                }

                _ => None
            };

            let top = regs.top();

            let a = match shifted {
                Some((inner, _)) => inner.gen_code(fun, regs, cg, None)?,
                None => lhs.gen_code(fun, regs, cg, None)?,
            };

            regs.free_to(top);

            let d = match dst { Some(dst) => dst, None => regs.alloc() };

            cg.push_insn(match shifted {
                Some((_, sh)) => Insn::rshift_mask(d, a, sh, lo, len),
                None => Insn::bit_and_mask(d, a, lo, len),
            });

            return Ok(d);
        }
    }

    // A shift by a constant amount folds into the instruction. Anything
    // below the word size is accepted, which is what lets the instruction
    // itself skip checking the amount
    if matches!(op, LShift | RShift) {
        let shift = const_value(rhs, cg.actor)
            .filter(|v| v.is_fixnum())
            .map(|v| v.as_fixnum())
            .filter(|s| *s >= 0 && *s < 64);

        if let Some(shift) = shift {
            let top = regs.top();
            let a = lhs.gen_code(fun, regs, cg, None)?;
            regs.free_to(top);

            let d = match dst { Some(dst) => dst, None => regs.alloc() };
            let shift = shift as u8;

            cg.push_insn(match op {
                LShift => Insn::lshift_imm(d, a, shift),
                _ => Insn::rshift_imm(d, a, shift),
            });

            return Ok(d);
        }
    }

    // If the rhs is a constant integer that fits an immediate operand
    if let Some(imm) = const_value(rhs, cg.actor).filter(|v| v.is_fixnum()) {
        let imm: Result<i16, _> = imm.as_fixnum().try_into();

        if let Ok(imm) = imm {
            let build = match op {
                Add => Some(Insn::add_imm16 as fn(u16, u16, i16) -> Insn),
                Sub => Some(Insn::sub_imm16 as fn(u16, u16, i16) -> Insn),
                Mul => Some(Insn::mul_imm16 as fn(u16, u16, i16) -> Insn),
                _ => None,
            };

            if let Some(build) = build {
                let top = regs.top();
                let a = lhs.gen_code(fun, regs, cg, None)?;
                regs.free_to(top);

                let d = match dst { Some(dst) => dst, None => regs.alloc() };
                cg.push_insn(build(d, a, imm));
                return Ok(d);
            }
        }
    }

    let top = regs.top();
    let a = lhs.gen_code(fun, regs, cg, None)?;
    let b = rhs.gen_code(fun, regs, cg, None)?;
    regs.free_to(top);

    let d = match dst { Some(dst) => dst, None => regs.alloc() };

    let insn = match op {
        BitAnd => Insn::bit_and(d, a, b),
        BitOr => Insn::bit_or(d, a, b),
        BitXor => Insn::bit_xor(d, a, b),
        LShift => Insn::lshift(d, a, b),
        RShift => Insn::rshift(d, a, b),

        Add => Insn::add(d, a, b),
        Sub => Insn::sub(d, a, b),
        Mul => Insn::mul(d, a, b),
        Div => Insn::div(d, a, b),
        Mod => Insn::modulo(d, a, b),

        _ => todo!("{:?}", op),
    };

    cg.push_insn(insn);
    Ok(d)
}

/// Register a variable can be written into directly, if it has one. A
/// global, a captured variable or an escaping local goes through an
/// instruction instead, so those get a temporary
fn write_target(decl: &Decl, fun: &Function, regs: &mut Regs) -> u16
{
    match decl {
        Decl::Local { .. } | Decl::Arg { .. } if !fun.escaping.contains(decl) => {
            decl_reg(decl, fun)
        }
        _ => regs.alloc()
    }
}

/// Generate a write of a value held in `src` to a variable
fn gen_var_write(
    decl: &Decl,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    src: u16,
)
{
    match *decl {
        Decl::Global { idx, .. } => {
            cg.push_insn(Insn::set_global(src, idx));
        }

        Decl::Local { .. } | Decl::Arg { .. } => {
            let reg = decl_reg(decl, fun);

            // An escaping variable holds a cell, and the value goes in it
            if fun.escaping.contains(decl) {
                cg.push_insn(Insn::cell_set(reg, src));
            } else if reg != src {
                cg.push_insn(Insn::mov(reg, src));
            }
        }

        Decl::Captured { idx, mutable } => {
            assert!(mutable);
            let cell = regs.alloc();
            cg.push_insn(Insn::clos_get(cell, idx.try_into().unwrap()));
            cg.push_insn(Insn::cell_set(cell, src));
        }

        _ => todo!()
    }
}

/// Generate a read of a variable, returning the register its value ends
/// up in. A local that is not escaping is already in a register, so it is
/// handed back as it is
fn gen_var_read(
    decl: &Decl,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    dst: Option<u16>,
) -> u16
{
    macro_rules! out {
        () => { match dst { Some(dst) => dst, None => regs.alloc() } }
    }

    match *decl {
        Decl::Fun { id } => {
            let d = out!();
            gen_value(cg, d, Value::fun(id));
            d
        }

        Decl::Class { id } => {
            let d = out!();
            gen_value(cg, d, Value::class(id));
            d
        }

        Decl::Global { idx, mutable } => {
            let d = out!();

            // An immutable global the unit has already initialized holds
            // the same value the compiled code will see
            match if mutable { None } else { cg.actor.global_value(idx) } {
                Some(val) if fits_imm40(val) => gen_value(cg, d, val),
                _ => cg.push_insn(Insn::get_global(d, idx)),
            }

            d
        }

        Decl::Arg { .. } => decl_reg(decl, fun),

        Decl::Local { .. } => {
            let reg = decl_reg(decl, fun);

            if fun.escaping.contains(decl) {
                let d = out!();
                cg.push_insn(Insn::cell_get(d, reg));
                d
            } else {
                reg
            }
        }

        Decl::Captured { idx, mutable } => {
            let d = out!();
            cg.push_insn(Insn::clos_get(d, idx.try_into().unwrap()));

            if mutable {
                cg.push_insn(Insn::cell_get(d, d));
            }

            d
        }
    }
}

fn gen_assign(
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    regs: &mut Regs,
    cg: &mut CodeGen,
    need_value: bool,
) -> Result<u16, ParseError>
{
    match lhs.expr.as_ref() {
        Expr::Ref { decl, .. } => {
            // Writing straight into the variable's own register is what
            // keeps a plain assignment down to the value's own code
            let target = write_target(decl, fun, regs);
            let src = rhs.gen_code(fun, regs, cg, Some(target))?;
            gen_var_write(decl, fun, regs, cg, src);
            Ok(src)
        }

        Expr::Member { base, field } => {
            // The assigned value is the result of the expression, so it
            // is allocated before the operands and outlives them
            let d = if need_value { Some(regs.alloc()) } else { None };

            let top = regs.top();
            let obj = base.gen_code(fun, regs, cg, None)?;
            let src = rhs.gen_code(fun, regs, cg, d)?;

            let cache = cg.actor.new_prop_cache(field);
            cg.push_insn(Insn::set_field(obj, src, cache));

            if let Some(d) = d {
                if d != src {
                    cg.push_insn(Insn::mov(d, src));
                }
            }

            regs.free_to(top);
            Ok(d.unwrap_or(src))
        }

        Expr::Index { base, index } => {
            let d = if need_value { Some(regs.alloc()) } else { None };

            let top = regs.top();
            let arr = base.gen_code(fun, regs, cg, None)?;
            let idx = index.gen_code(fun, regs, cg, None)?;
            let src = rhs.gen_code(fun, regs, cg, d)?;

            cg.push_insn(Insn::set_index(arr, idx, src));

            if let Some(d) = d {
                if d != src {
                    cg.push_insn(Insn::mov(d, src));
                }
            }

            regs.free_to(top);
            Ok(d.unwrap_or(src))
        }

        _ => todo!()
    }
}
