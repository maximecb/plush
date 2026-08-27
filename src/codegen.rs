use std::mem::size_of;
use crate::ast::*;
use crate::lexer::ParseError;
use crate::symbols::Decl;
use crate::vm::Insn;
use crate::value::Value;
use crate::alloc::HEADER_SIZE;
use crate::str::Str;
use crate::vm::Actor;

/// Compiled function object
#[derive(Copy, Clone)]
pub struct CompiledFun
{
    pub entry_pc: usize,
    pub num_params: usize,
    pub num_locals: usize,
}

// Patch a jump instruction
fn patch_jump(code: &mut Vec<Insn>, jmp_idx: usize, dst_idx: usize)
{
    let jump_ofs = (dst_idx as i32) - (jmp_idx as i32) - 1;

    match &mut code[jmp_idx] {
        Insn::if_true { target_ofs } |
        Insn::if_false { target_ofs } |
        Insn::jump { target_ofs } => {
            *target_ofs = jump_ofs;
        }

        _ => todo!()
    }
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
        // Entry address of the compiled function
        let entry_pc = actor.insns.len();

        //let start_idx = actor.insns.len();

        // Compile the function body
        self.body.gen_code(self, &mut vec![], &mut vec![], actor)?;

        /*
        let end_idx = actor.insns.len();
        println!("# {}", self.name);
        for i in start_idx..end_idx {
            println!("{:?}", actor.insns[i]);
        }
        println!();
        */

        // If the body needs a final return
        if self.needs_final_return() {
            // If this is a constructor, return the self argument
            if self.is_ctor() {
                actor.insns.push(Insn::get_arg { idx: 0 });
            } else {
                actor.insns.push(Insn::push { val: Value::NIL });
            }

            actor.insns.push(Insn::ret);
        }

        Ok(CompiledFun {
            entry_pc,
            num_params: self.params.len(),
            num_locals: self.num_locals,
        })
    }
}

impl StmtBox
{
    fn gen_code(
        &self,
        fun: &Function,
        break_idxs: &mut Vec<usize>,
        cont_idxs: &mut Vec<usize>,
        actor: &mut Actor,
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_ref() {
            Stmt::Expr(expr) => {
                match expr.expr.as_ref() {
                    // For assignment expressions as statements,
                    // avoid generating output that we would then need to pop
                    Expr::Binary { op: BinOp::Assign, lhs, rhs } => {
                        gen_assign(lhs, rhs, fun, actor, false)?;
                    }

                    _ => {
                        expr.gen_code(fun, actor)?;
                        actor.insns.push(Insn::pop);
                    }
                }
            }

            Stmt::Break => {
                break_idxs.push(actor.insns.len());
                actor.insns.push(Insn::jump { target_ofs: 0});
            }

            Stmt::Continue => {
                cont_idxs.push(actor.insns.len());
                actor.insns.push(Insn::jump { target_ofs: 0});
            }

            Stmt::Return(expr) => {
                expr.gen_code(fun, actor)?;
                actor.insns.push(Insn::ret);
            }

            Stmt::Block(stmts) => {
                // For each closure declaration
                if !fun.is_unit {
                    for stmt in stmts {
                        if let Stmt::Let { init_expr, decl, .. } = stmt.stmt.as_ref() {
                            if let Expr::Fun { fun_id, captured } = init_expr.expr.as_ref() {
                                // Create the closure
                                actor.insns.push(Insn::clos_new {
                                    fun_id: *fun_id,
                                    num_slots: captured.len() as u32,
                                });

                                // Initialize the local variable for this closure
                                gen_var_write(decl.as_ref().unwrap(), fun, &mut actor.insns);
                            }
                        }
                    }
                }

                for stmt in stmts {
                    stmt.gen_code(fun, break_idxs, cont_idxs, actor)?;
                }
            }

            Stmt::If { test_expr, then_stmt, else_stmt } => {
                // Compile the test expression
                test_expr.gen_code(fun, actor)?;

                // If false, jump to else stmt
                let if_idx = actor.insns.len();
                actor.insns.push(Insn::if_false { target_ofs: 0 });

                if else_stmt.is_some() {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, actor)?;
                    let jump_idx = actor.insns.len();
                    actor.insns.push(Insn::jump { target_ofs: 0 });

                    // Patch the if_false to jump to the else clause
                    let dst_idx = actor.insns.len();
                    patch_jump(&mut actor.insns, if_idx, dst_idx);

                    else_stmt.as_ref().unwrap().gen_code(fun, break_idxs, cont_idxs, actor)?;

                    // Patch the jump instruction to jump after the else clause
                    let dst_idx = actor.insns.len();
                    patch_jump(&mut actor.insns, jump_idx, dst_idx);
                }
                else
                {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, actor)?;

                    // Patch the if_false to jump to the else clause
                    let jump_ofs = (actor.insns.len() as i32) - (if_idx as i32) - 1;
                    if let Insn::if_false { target_ofs } = &mut actor.insns[if_idx] {
                        *target_ofs = jump_ofs;
                    }
                }
            }

            Stmt::For { init_stmt, test_expr, incr_expr, body_stmt } => {
                // Generate code for the init statement
                init_stmt.gen_code(
                    fun,
                    break_idxs,
                    cont_idxs,
                    actor,
                )?;

                let mut break_idxs = Vec::new();
                let mut cont_idxs = Vec::new();

                // Evaluate the test expression
                let test_idx = actor.insns.len();
                test_expr.gen_code(fun, actor)?;

                // If the test fails, jump after the loop
                break_idxs.push(actor.insns.len());
                actor.insns.push(Insn::if_false { target_ofs: 0 });

                body_stmt.gen_code(
                    fun,
                    &mut break_idxs,
                    &mut cont_idxs,
                    actor,
                )?;

                // Continue will jump here
                let cont_idx = actor.insns.len();

                // Evaluate the increment expression
                incr_expr.gen_code(fun, actor)?;
                actor.insns.push(Insn::pop);

                // Jump back to the loop test
                actor.insns.push(Insn::jump { target_ofs: 0 });
                let jmp_idx = actor.insns.len() - 1;
                patch_jump(&mut actor.insns, jmp_idx, test_idx);

                // Break will jump here
                let break_idx = actor.insns.len();

                // Patch continue jumps
                for branch_idx in cont_idxs.iter() {
                    patch_jump(&mut actor.insns, *branch_idx, cont_idx);
                }

                // Patch break jumps
                for branch_idx in break_idxs.iter() {
                    patch_jump(&mut actor.insns, *branch_idx, break_idx);
                }
            }

            Stmt::Assert { test_expr } => {
                test_expr.gen_code(fun, actor)?;

                let if_idx = actor.insns.len();
                actor.insns.push(Insn::if_true { target_ofs: 0 });
                actor.insns.push(Insn::panic { pos: self.pos });
                let dst_idx = actor.insns.len();
                patch_jump(&mut actor.insns, if_idx, dst_idx);
            }

            // Variable declaration
            Stmt::Let { mutable: _, var_name: _, init_expr, decl } => {
                // Nothing to do for top-level functions
                if let Some(Decl::Fun { .. }) = decl {
                    return Ok(())
                }

                match init_expr.expr.as_ref() {
                    Expr::Fun { fun_id: _, captured } => {
                        // Read the closure decl
                        let decl = decl.as_ref().unwrap();
                        gen_var_read(decl, fun, &mut actor.insns);

                        // For each variable captured by the closure
                        for (idx, decl) in captured.iter().enumerate() {
                            actor.insns.push(Insn::dup);

                            // Copy variables and cells captured by the closure
                            match decl {
                                Decl::Local { idx, mutable: true, .. } => {
                                    actor.insns.push(Insn::get_local { idx: *idx });
                                }
                                _ => gen_var_read(decl, fun, &mut actor.insns)
                            }
                            actor.insns.push(Insn::clos_set { idx: idx as u32 });
                        }
                    }

                    _ => init_expr.gen_code(fun, actor)?
                }

                // If this is an escaping mutable variable
                if fun.escaping.contains(decl.as_ref().unwrap()) {
                    let local_idx = match decl.unwrap() {
                        Decl::Local { idx, .. } => idx,
                        _ => panic!()
                    };

                    // Allocate a mutable closure cell for this variable
                    actor.insns.push(Insn::cell_new);
                    actor.insns.push(Insn::set_local { idx: local_idx });
                }

                // Initialize the local variable
                gen_var_write(decl.as_ref().unwrap(), fun, &mut actor.insns);
            }

            Stmt::ClassDecl { .. } => {}

            //_ => todo!("{:?}", self.stmt)
        }

        Ok(())
    }
}

impl ExprBox
{
    fn gen_code(
        &self,
        fun: &Function,
        actor: &mut Actor,
    ) -> Result<(), ParseError>
    {
        match self.expr.as_ref() {
            Expr::Nil => actor.insns.push(Insn::push { val: Value::NIL }),
            Expr::True => actor.insns.push(Insn::push { val: Value::TRUE }),
            Expr::False => actor.insns.push(Insn::push { val: Value::FALSE }),

            // FIXME: for host calls, emit call_host directly, don't push a host function id
            Expr::HostFn(f) => actor.insns.push(Insn::push { val: Value::host_fn(*f) }),

            // Constants that don't fit in an immediate are boxed in the
            // heap the code is compiled for, and traced from there
            Expr::Int64(v) => {
                let val = match Value::try_fixnum(*v) {
                    Some(val) => val,
                    None => {
                        actor.gc_check(HEADER_SIZE + size_of::<i64>(), &mut []);
                        actor.alloc.heap_int64(*v)
                    }
                };
                actor.insns.push(Insn::push { val });
            }

            Expr::Float64(v) => {
                let val = match Value::try_flonum(*v) {
                    Some(val) => val,
                    None => {
                        actor.gc_check(HEADER_SIZE + size_of::<f64>(), &mut []);
                        actor.alloc.heap_float64(*v)
                    }
                };
                actor.insns.push(Insn::push { val });
            }

            Expr::String(s) => {
                actor.gc_check(Str::alloc_size(s.len()), &mut []);
                let val = Str::new(&s, &mut actor.alloc);
                actor.insns.push(Insn::push { val });
            }

            Expr::ByteArray(bytes) => {
                use crate::bytearray::ByteArray;
                actor.gc_check(ByteArray::alloc_size(bytes.len()), &mut []);
                let ba = ByteArray::with_size(bytes.len(), &mut actor.alloc);
                unsafe { ba.as_ba().get_slice_mut(0, bytes.len()).copy_from_slice(&bytes) };
                actor.insns.push(Insn::push { val: ba });
                actor.insns.push(Insn::ba_clone);
            }

            Expr::Array { exprs } => {
                return gen_arr_expr(
                    exprs,
                    fun,
                    actor,
                );
            }

            Expr::Dict { pairs } => {
                return gen_dict_expr(
                    pairs,
                    fun,
                    actor,
                );
            }

            Expr::Ref {decl, .. } => {
                gen_var_read(decl, fun, &mut actor.insns);
            }

            Expr::Index { base, index } => {
                base.gen_code(fun, actor)?;
                index.gen_code(fun, actor)?;
                actor.insns.push(Insn::get_index);
            }

            Expr::Member { base, field } => {
                base.gen_code(fun, actor)?;
                actor.gc_check(Str::alloc_size(field.len()), &mut []);
                let field = Str::new(&field, &mut actor.alloc);
                actor.insns.push(Insn::get_field {
                    field,
                    class_id: Default::default(),
                    slot_idx: Default::default(),
                });
            }

            Expr::InstanceOf { val, class_id, .. } => {
                val.gen_code(fun, actor)?;
                actor.insns.push(Insn::instanceof { class_id: *class_id });
            }

            Expr::Unary { op, child } => {
                child.gen_code(fun, actor)?;

                match op {
                    UnOp::Minus => {
                        actor.insns.push(Insn::push { val: Value::fixnum(-1) });
                        actor.insns.push(Insn::mul);
                    }

                    // Logical negation
                    UnOp::Not => {
                        actor.insns.push(Insn::not);
                    }

                    //_ => todo!()
                }
            },

            Expr::Binary { op, lhs, rhs } => {
                gen_bin_op(op, lhs, rhs, fun, actor)?;
            }

            Expr::Ternary { test_expr, then_expr, else_expr } => {
                // Evaluate the test expression
                test_expr.gen_code(fun, actor)?;
                let if_idx = actor.insns.len();
                actor.insns.push(Insn::if_false { target_ofs: 0 });

                // Evaluate the then expression
                then_expr.gen_code(fun, actor)?;
                let jump_idx = actor.insns.len();
                actor.insns.push(Insn::jump { target_ofs: 0 });

                // Patch the if_false to jump to the else clause
                let dst_idx = actor.insns.len();
                patch_jump(&mut actor.insns, if_idx, dst_idx);

                // Evaluate the else expression
                else_expr.gen_code(fun, actor)?;

                // Patch the jump over the else expression
                let dst_idx = actor.insns.len();
                patch_jump(&mut actor.insns, jump_idx, dst_idx);
            }

            Expr::Call { callee, args } => {
                let argc = args.len().try_into().unwrap();

                match callee.expr.as_ref() {
                    // New class instance
                    Expr::Ref { decl: Decl::Class { id }, .. } => {
                        // Evaluate the arguments
                        for arg in args {
                            arg.gen_code(fun, actor)?;
                        }

                        actor.insns.push(Insn::new { class_id: *id, argc });
                    }

                    // Callee has form a.b
                    Expr::Member { base, field } => {
                        // Evaluate the self argument
                        base.gen_code(fun, actor)?;

                        for arg in args {
                            arg.gen_code(fun, actor)?;
                        }

                        actor.gc_check(Str::alloc_size(field.len()), &mut []);
                        let name = Str::new(&field, &mut actor.alloc);
                        actor.insns.push(Insn::call_method { name, argc });
                    }

                    // Call to a known function
                    Expr::Ref { decl: Decl::Fun { id }, .. } => {
                        for arg in args {
                            arg.gen_code(fun, actor)?;
                        }

                        actor.insns.push(Insn::call_direct { fun_id: *id, argc });
                    }

                    // Plain regular call
                    _ => {
                        for arg in args {
                            arg.gen_code(fun, actor)?;
                        }

                        callee.gen_code(fun, actor)?;
                        actor.insns.push(Insn::call { argc });
                    }
                }
            }

            // Function expression
            Expr::Fun { fun_id, captured } => {
                // If this is not a closure
                if captured.len() == 0 {
                    actor.insns.push(Insn::push { val: Value::fun(*fun_id) });
                    return Ok(())
                }

                actor.insns.push(Insn::clos_new {
                    fun_id: *fun_id,
                    num_slots: captured.len() as u32,
                });

                // For each variable captured by the closure
                for (idx, decl) in captured.iter().enumerate() {
                    actor.insns.push(Insn::dup);

                    // Copy variables and cells captured by the closure
                    match decl {
                        Decl::Local { idx, mutable: true, .. } => {
                            actor.insns.push(Insn::get_local { idx: *idx });
                        }
                        _ => gen_var_read(decl, fun, &mut actor.insns)
                    }
                    actor.insns.push(Insn::clos_set { idx: idx as u32 });
                }
            }

            _ => todo!("{:?}", self)
        }

        Ok(())
    }
}

// Generate code for an array literal expression
fn gen_arr_expr(
    exprs: &Vec<ExprBox>,
    fun: &Function,
    actor: &mut Actor,
) -> Result<(), ParseError>
{
    actor.insns.push(Insn::arr_new { capacity: exprs.len() as u32 });

    for expr in exprs {
        actor.insns.push(Insn::dup);
        expr.gen_code(fun, actor)?;
        actor.insns.push(Insn::arr_push);
    }

    Ok(())
}

// Generate code for a dictionary literal expression
fn gen_dict_expr(
    pairs: &Vec<(String, ExprBox)>,
    fun: &Function,
    actor: &mut Actor,
) -> Result<(), ParseError>
{
    actor.insns.push(Insn::dict_new);

    // For each field
    for (name, expr) in pairs {
        actor.insns.push(Insn::dup);

        expr.gen_code(fun, actor)?;

        actor.gc_check(Str::alloc_size(name.len()), &mut []);
        let field_name = Str::new(&name, &mut actor.alloc);

        actor.insns.push(Insn::set_field {
            field: field_name,
            class_id: Default::default(),
            slot_idx: Default::default(),
        });
    }

    Ok(())
}

fn gen_bin_op(
    op: &BinOp,
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    actor: &mut Actor,
) -> Result<(), ParseError>
{
    use BinOp::*;

    // Assignments are different from other kinds of expressions
    // because we don't evaluate the lhs the same way
    if *op == Assign {
        gen_assign(lhs, rhs, fun, actor, true)?;
        return Ok(());
    }

    // Logical AND (a && b)
    if *op == And {
        // If a is false, the result is false
        lhs.gen_code(fun, actor)?;
        let if0_idx = actor.insns.len();
        actor.insns.push(Insn::if_false { target_ofs: 0 });

        // If b is false, the result is false
        rhs.gen_code(fun, actor)?;
        let if1_idx = actor.insns.len();
        actor.insns.push(Insn::if_false { target_ofs: 0 });

        // Both subexpressions are true
        actor.insns.push(Insn::push { val: Value::TRUE });
        let jmp_idx = actor.insns.len();
        actor.insns.push(Insn::jump { target_ofs: 0 });

        // If false, short-circuit here
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, if0_idx, dst_idx);
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, if1_idx, dst_idx);
        actor.insns.push(Insn::push { val: Value::FALSE });

        // Done label
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, jmp_idx, dst_idx);

        return Ok(());
    }

    // Logical OR (a || b)
    if *op == Or {

        // If a is true, the result is true
        lhs.gen_code(fun, actor)?;
        let if0_idx = actor.insns.len();
        actor.insns.push(Insn::if_true { target_ofs: 0 });

        // If b is true, the result is true
        rhs.gen_code(fun, actor)?;
        let if1_idx = actor.insns.len();
        actor.insns.push(Insn::if_true { target_ofs: 0 });

        // Both subexpressions are false
        actor.insns.push(Insn::push { val: Value::FALSE });
        let jmp_idx = actor.insns.len();
        actor.insns.push(Insn::jump { target_ofs: 0 });

        // If true, short-circuit here
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, if0_idx, dst_idx);
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, if1_idx, dst_idx);
        actor.insns.push(Insn::push { val: Value::TRUE });

        // Done label
        let dst_idx = actor.insns.len();
        patch_jump(&mut actor.insns, jmp_idx, dst_idx);

        return Ok(());
    }

    // If the rhs is a constant integer value that fits in an immediate
    if let Expr::Int64(int_val) = rhs.expr.as_ref() {
        // The negation below has to fit as well, so keep one bit of room
        let fits = Value::fits_fixnum(*int_val) && Value::fits_fixnum(-*int_val);

        match op {
            Add if fits => {
                lhs.gen_code(fun, actor)?;
                actor.insns.push(Insn::add_i64 { val: *int_val });
                return Ok(())
            }

            Sub if fits => {
                lhs.gen_code(fun, actor)?;
                actor.insns.push(Insn::add_i64 { val: -int_val });
                return Ok(())
            }

            _ => {}
        }
    }

    lhs.gen_code(fun, actor)?;
    rhs.gen_code(fun, actor)?;

    match op {
        BitAnd => actor.insns.push(Insn::bit_and),
        BitOr => actor.insns.push(Insn::bit_or),
        BitXor => actor.insns.push(Insn::bit_xor),
        LShift => actor.insns.push(Insn::lshift),
        RShift => actor.insns.push(Insn::rshift),

        Add => actor.insns.push(Insn::add),
        Sub => actor.insns.push(Insn::sub),
        Mul => actor.insns.push(Insn::mul),
        Div => actor.insns.push(Insn::div),
        IntDiv => actor.insns.push(Insn::div_int),
        Mod => actor.insns.push(Insn::modulo),

        Eq => actor.insns.push(Insn::eq),
        Ne => actor.insns.push(Insn::ne),
        Lt => actor.insns.push(Insn::lt),
        Le => actor.insns.push(Insn::le),
        Gt => actor.insns.push(Insn::gt),
        Ge => actor.insns.push(Insn::ge),

        _ => todo!("{:?}", op),
    }

    Ok(())
}

/// Generate a write to a variable
/// Assumes the value to be written is on top of the stack
fn gen_var_write(
    decl: &Decl,
    fun: &Function,
    code: &mut Vec<Insn>,
)
{
    match *decl {
        Decl::Global { idx, .. } => {
            code.push(Insn::set_global { idx });
        }

        Decl::Local { idx, .. } => {
            // If this is an escaping mutable variable
            if fun.escaping.contains(decl) {
                code.push(Insn::get_local { idx });
                code.push(Insn::cell_set);

            } else {
                code.push(Insn::set_local { idx });
            }
        }

        Decl::Captured { idx, mutable } => {
            assert!(mutable);
            code.push(Insn::clos_get { idx });
            code.push(Insn::cell_set);
        }

        _ => todo!()
    }
}

/// Generate a write to a variable
/// Pushes the value read on the stack
fn gen_var_read(
    decl: &Decl,
    fun: &Function,
    code: &mut Vec<Insn>,
)
{
    match *decl {
        Decl::Fun { id } => {
            code.push(Insn::push { val: Value::fun(id) });
        }

        Decl::Class { id } => {
            code.push(Insn::push { val: Value::class(id) });
        }

        Decl::Global { idx, .. } => {
            code.push(Insn::get_global { idx });
        }

        Decl::Arg { idx, .. } => {
            code.push(Insn::get_arg { idx });
        }

        Decl::Local { idx, .. } => {
            // If this is an escaping mutable variable
            if fun.escaping.contains(decl) {
                code.push(Insn::get_local { idx });
                code.push(Insn::cell_get);

            } else {
                code.push(Insn::get_local { idx });
            }
        }

        Decl::Captured { idx, mutable } => {
            if mutable {
                code.push(Insn::clos_get { idx });
                code.push(Insn::cell_get);
            } else {
                code.push(Insn::clos_get { idx });
            }
        }
    }
}

fn gen_assign(
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    actor: &mut Actor,
    need_value: bool,
) -> Result<(), ParseError>
{
    //dbg!(lhs);
    //dbg!(rhs);

    match lhs.expr.as_ref() {
        Expr::Ref { decl, .. } => {
            rhs.gen_code(fun, actor)?;

            // If the output value is needed
            if need_value {
                actor.insns.push(Insn::dup);
            }

            gen_var_write(decl, fun, &mut actor.insns);
        }

        Expr::Member { base, field } => {
            if need_value {
                rhs.gen_code(fun, actor)?;
                base.gen_code(fun, actor)?;
                actor.insns.push(Insn::getn { idx: 1 });
            } else {
                base.gen_code(fun, actor)?;
                rhs.gen_code(fun, actor)?;
            }

            // Allocated after the operands: generating them can collect,
            // and a name held across that would be left dangling
            actor.gc_check(Str::alloc_size(field.len()), &mut []);
            let field = Str::new(&field, &mut actor.alloc);

            actor.insns.push(Insn::set_field {
                field,
                class_id: Default::default(),
                slot_idx: Default::default(),
            });
        }

        Expr::Index { base, index } => {
            if need_value {
                rhs.gen_code(fun, actor)?;
                base.gen_code(fun, actor)?;
                index.gen_code(fun, actor)?;
                actor.insns.push(Insn::getn { idx: 2 });
                actor.insns.push(Insn::set_index);
            } else {
                base.gen_code(fun, actor)?;
                index.gen_code(fun, actor)?;
                rhs.gen_code(fun, actor)?;
                actor.insns.push(Insn::set_index);
            }
        }

        _ => todo!()
    }

    Ok(())
}
