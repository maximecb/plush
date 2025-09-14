use std::cmp::max;
use crate::ast::*;
use crate::lexer::ParseError;
use crate::symbols::{Decl};
use crate::vm::{Insn, Value};
use crate::alloc::Alloc;

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
        code: &mut Vec<Insn>,
        alloc: &mut Alloc
    ) -> Result<CompiledFun, ParseError>
    {
        // Entry address of the compiled function
        let entry_pc = code.len();

        //let start_idx = code.len();

        // Compile the function body
        self.body.gen_code(self, &mut vec![], &mut vec![], code, alloc)?;

        /*
        let end_idx = code.len();
        println!("# {}", self.name);
        for i in start_idx..end_idx {
            println!("{:?}", code[i]);
        }
        println!();
        */

        // If the body needs a final return
        if self.needs_final_return() {
            // If this is a constructor, return the self argument
            if self.is_ctor() {
                code.push(Insn::get_arg { idx: 0 });
            } else {
                code.push(Insn::push { val: Value::Nil });
            }

            code.push(Insn::ret);
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
        code: &mut Vec<Insn>,
        alloc: &mut Alloc,
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_ref() {
            Stmt::Expr(expr) => {
                match expr.expr.as_ref() {
                    // For assignment expressions as statements,
                    // avoid generating output that we would then need to pop
                    Expr::Binary { op: BinOp::Assign, lhs, rhs } => {
                        gen_assign(lhs, rhs, fun, code, alloc, false)?;
                    }

                    /*
                    // For asm expressions with void output type, don't pop
                    // the output because no output is produced
                    Expr::Asm { out_type: Type::Void, .. } => {
                        expr.gen_code(fun, sym, code, out)?;
                    }
                    */

                    _ => {
                        expr.gen_code(fun, code, alloc)?;
                        code.push(Insn::pop);
                    }
                }
            }

            Stmt::Break => {
                break_idxs.push(code.len());
                code.push(Insn::jump { target_ofs: 0});
            }

            Stmt::Continue => {
                cont_idxs.push(code.len());
                code.push(Insn::jump { target_ofs: 0});
            }

            Stmt::Return(expr) => {
                expr.gen_code(fun, code, alloc)?;
                code.push(Insn::ret);
            }

            Stmt::Block(stmts) => {
                // For each closure declaration
                if !fun.is_unit {
                    for stmt in stmts {
                        if let Stmt::Let { init_expr, decl, .. } = stmt.stmt.as_ref() {
                            if let Expr::Fun { fun_id, captured } = init_expr.expr.as_ref() {
                                // Create the closure
                                code.push(Insn::clos_new {
                                    fun_id: *fun_id,
                                    num_slots: captured.len() as u32,
                                });

                                // Initialize the local variable
                                gen_var_write(decl.as_ref().unwrap(), code);
                            }
                        }
                    }
                }

                for stmt in stmts {
                    stmt.gen_code(fun, break_idxs, cont_idxs, code, alloc)?;
                }
            }

            Stmt::If { test_expr, then_stmt, else_stmt } => {
                // Compile the test expression
                test_expr.gen_code(fun, code, alloc)?;

                // If false, jump to else stmt
                let if_idx = code.len();
                code.push(Insn::if_false { target_ofs: 0 });

                if else_stmt.is_some() {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, code, alloc)?;
                    let jump_idx = code.len();
                    code.push(Insn::jump { target_ofs: 0 });

                    // Patch the if_false to jump to the else clause
                    patch_jump(code, if_idx, code.len());

                    else_stmt.as_ref().unwrap().gen_code(fun, break_idxs, cont_idxs, code, alloc)?;

                    // Patch the jump instruction to jump after the else clause
                    patch_jump(code, jump_idx, code.len());
                }
                else
                {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, code, alloc)?;

                    // Patch the if_false to jump to the else clause
                    let jump_ofs = (code.len() as i32) - (if_idx as i32) - 1;
                    if let Insn::if_false { target_ofs } = &mut code[if_idx] {
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
                    code,
                    alloc,
                )?;

                let mut break_idxs = Vec::new();
                let mut cont_idxs = Vec::new();

                // Evaluate the test expression
                let test_idx = code.len();
                test_expr.gen_code(fun, code, alloc)?;

                // If the test fails, jump after the loop
                break_idxs.push(code.len());
                code.push(Insn::if_false { target_ofs: 0 });

                body_stmt.gen_code(
                    fun,
                    &mut break_idxs,
                    &mut cont_idxs,
                    code,
                    alloc,
                )?;

                // Continue will jump here
                let cont_idx = code.len();

                // Evaluate the increment expression
                incr_expr.gen_code(fun, code, alloc)?;

                // Jump back to the loop test
                code.push(Insn::jump { target_ofs: 0 });
                patch_jump(code, code.len() - 1, test_idx);

                // Break will jump here
                let break_idx = code.len();

                // Patch continue jumps
                for branch_idx in cont_idxs.iter() {
                    patch_jump(code, *branch_idx, cont_idx);
                }

                // Patch break jumps
                for branch_idx in break_idxs.iter() {
                    patch_jump(code, *branch_idx, break_idx);
                }
            }

            Stmt::Assert { test_expr } => {
                test_expr.gen_code(fun, code, alloc)?;

                let if_idx = code.len();
                code.push(Insn::if_true { target_ofs: 0 });

                // TODO: need to report the source position etc.
                // Might be nice to report the assert statement
                // contents as a string?
                /*
                code.insn_s("push", &format!("assertion failed at {}\n", self.pos));
                code.add_insn(vec![
                    "'call_host'".to_string(),
                    "'print_str'".to_string(),
                    "1".to_string(),
                ]);
                */

                code.push(Insn::panic { pos: self.pos });
                patch_jump(code, if_idx, code.len());
            }

            // Variable declaration
            Stmt::Let { mutable, var_name, init_expr, decl } => {
                // Nothing to do for top-level functions
                if let Some(Decl::Fun { .. }) = decl {
                    return Ok(())
                }

                match init_expr.expr.as_ref() {
                    Expr::Fun { fun_id, captured } => {
                        // Read the closure decl
                        let decl = decl.as_ref().unwrap();
                        gen_var_read(decl, code);

                        // For each variable captured by the closure
                        for (idx, decl) in captured.iter().enumerate() {
                            code.push(Insn::dup);

                            // Read the variable and write its value on the closure
                            gen_var_read(decl, code);
                            code.push(Insn::clos_set { idx: idx as u32 });
                        }
                    }

                    _ => init_expr.gen_code(fun, code, alloc)?
                }

                // Initialize the local variable
                gen_var_write(decl.as_ref().unwrap(), code);
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
        code: &mut Vec<Insn>,
        alloc: &mut Alloc,
    ) -> Result<(), ParseError>
    {
        match self.expr.as_ref() {
            Expr::Nil => code.push(Insn::push { val: Value::Nil }),
            Expr::True => code.push(Insn::push { val: Value::True }),
            Expr::False => code.push(Insn::push { val: Value::False }),
            Expr::Int64(v) => code.push(Insn::push { val: Value::Int64(*v) }),
            Expr::Float64(v) => code.push(Insn::push { val: Value::Float64(*v) }),
            Expr::HostFn(f) => code.push(Insn::push { val: Value::HostFn(*f) }),

            Expr::String(s) => {
                let p_str = alloc.str_const(s.clone());
                code.push(Insn::push { val: Value::String(p_str) });
            }

            Expr::ByteArray(bytes) => {
                let ba = crate::bytearray::ByteArray::new(bytes);
                let p_ba = alloc.alloc(ba);
                code.push(Insn::push { val: Value::ByteArray(p_ba) });
                code.push(Insn::ba_clone);
            }

            Expr::Array { exprs } => {
                return gen_arr_expr(
                    exprs,
                    fun,
                    code,
                    alloc,
                );
            }

            Expr::Dict { pairs } => {
                return gen_dict_expr(
                    pairs,
                    fun,
                    code,
                    alloc,
                );
            }

            Expr::Ref(decl) => {
                gen_var_read(decl, code);
            }

            Expr::Index { base, index } => {
                base.gen_code(fun, code, alloc)?;
                index.gen_code(fun, code, alloc)?;
                code.push(Insn::get_index);
            }

            Expr::Member { base, field } => {
                base.gen_code(fun, code, alloc)?;
                let field = alloc.str_const(field.clone());
                code.push(Insn::get_field {
                    field,
                    class_id: Default::default(),
                    slot_idx: Default::default(),
                });
            }

            Expr::InstanceOf { val, class_id, .. } => {
                val.gen_code(fun, code, alloc)?;
                code.push(Insn::instanceof { class_id: *class_id });
            }

            Expr::Unary { op, child } => {
                child.gen_code(fun, code, alloc)?;

                match op {
                    UnOp::Minus => {
                        code.push(Insn::push { val: Value::Int64(-1) });
                        code.push(Insn::mul);
                    }

                    // Logical negation
                    UnOp::Not => {
                        code.push(Insn::not);
                    }

                    //_ => todo!()
                }
            },

            Expr::Binary { op, lhs, rhs } => {
                gen_bin_op(op, lhs, rhs, fun, code, alloc)?;
            }

            Expr::Ternary { test_expr, then_expr, else_expr } => {
                // Evaluate the test expression
                test_expr.gen_code(fun, code, alloc)?;
                let if_idx = code.len();
                code.push(Insn::if_false { target_ofs: 0 });

                // Evaluate the then expression
                then_expr.gen_code(fun, code, alloc)?;
                let jump_idx = code.len();
                code.push(Insn::jump { target_ofs: 0 });

                // Patch the if_false to jump to the else clause
                patch_jump(code, if_idx, code.len());

                // Evaluate the else expression
                else_expr.gen_code(fun, code, alloc)?;

                // Patch the jump over the else expression
                patch_jump(code, jump_idx, code.len());
            }

            Expr::Call { callee, args } => {
                let argc = args.len().try_into().unwrap();

                match callee.expr.as_ref() {
                    // New class instance
                    Expr::Ref(Decl::Class { id }) => {
                        // Evaluate the arguments
                        for arg in args {
                            arg.gen_code(fun, code, alloc)?;
                        }

                        code.push(Insn::new { class_id: *id, argc });
                    }

                    // Callee has form a.b
                    Expr::Member { base, field } => {
                        // Evaluate the self argument
                        base.gen_code(fun, code, alloc)?;

                        for arg in args {
                            arg.gen_code(fun, code, alloc)?;
                        }

                        let name = alloc.str_const(field.clone());
                        code.push(Insn::call_method { name, argc });
                    }

                    // Call to a known function
                    Expr::Ref(Decl::Fun { id }) => {
                        for arg in args {
                            arg.gen_code(fun, code, alloc)?;
                        }

                        code.push(Insn::call_direct { fun_id: *id, argc });
                    }

                    // Plain regular call
                    _ => {
                        for arg in args {
                            arg.gen_code(fun, code, alloc)?;
                        }

                        callee.gen_code(fun, code, alloc)?;
                        code.push(Insn::call { argc });
                    }
                }
            }

            // Function expression
            Expr::Fun { fun_id, captured } => {
                // If this is not a closure
                if captured.len() == 0 {
                    code.push(Insn::push { val: Value::Fun(*fun_id) });
                    return Ok(())
                }

                code.push(Insn::clos_new {
                    fun_id: *fun_id,
                    num_slots: captured.len() as u32,
                });

                // For each variable captured by the closure
                for (idx, decl) in captured.iter().enumerate() {
                    code.push(Insn::dup);
                    gen_var_read(decl, code);
                    code.push(Insn::clos_set { idx: idx as u32 });
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
    code: &mut Vec<Insn>,
    alloc: &mut Alloc,
) -> Result<(), ParseError>
{
    code.push(Insn::arr_new { capacity: exprs.len() as u32 });

    for expr in exprs {
        code.push(Insn::dup);
        expr.gen_code(fun, code, alloc)?;
        code.push(Insn::arr_push);
    }

    Ok(())
}

// Generate code for a dictionary literal expression
fn gen_dict_expr(
    pairs: &Vec<(String, ExprBox)>,
    fun: &Function,
    code: &mut Vec<Insn>,
    alloc: &mut Alloc,
) -> Result<(), ParseError>
{
    code.push(Insn::dict_new);

    // For each field
    for (name, expr) in pairs {
        code.push(Insn::dup);

        expr.gen_code(fun, code, alloc)?;

        let field_name = alloc.str_const(name.clone());

        code.push(Insn::set_field {
            field: field_name,
            class_id: Default::default(),
            slot_idx: Default::default(),
        });
    }

    code.push(Insn::dup);

    Ok(())
}

fn gen_bin_op(
    op: &BinOp,
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    code: &mut Vec<Insn>,
    alloc: &mut Alloc,
) -> Result<(), ParseError>
{
    use BinOp::*;

    // Assignments are different from other kinds of expressions
    // because we don't evaluate the lhs the same way
    if *op == Assign {
        gen_assign(lhs, rhs, fun, code, alloc, true)?;
        return Ok(());
    }

    // Logical AND (a && b)
    if *op == And {
        // If a is false, the result is false
        lhs.gen_code(fun, code, alloc)?;
        let if0_idx = code.len();
        code.push(Insn::if_false { target_ofs: 0 });

        // If b is false, the result is false
        rhs.gen_code(fun, code, alloc)?;
        let if1_idx = code.len();
        code.push(Insn::if_false { target_ofs: 0 });

        // Both subexpressions are true
        code.push(Insn::push { val: Value::True });
        let jmp_idx = code.len();
        code.push(Insn::jump { target_ofs: 0 });

        // If false, short-circuit here
        patch_jump(code, if0_idx, code.len());
        patch_jump(code, if1_idx, code.len());
        code.push(Insn::push { val: Value::False });

        // Done label
        patch_jump(code, jmp_idx, code.len());

        return Ok(());
    }

    // Logical OR (a || b)
    if *op == Or {

        // If a is true, the result is true
        lhs.gen_code(fun, code, alloc)?;
        let if0_idx = code.len();
        code.push(Insn::if_true { target_ofs: 0 });

        // If b is true, the result is true
        rhs.gen_code(fun, code, alloc)?;
        let if1_idx = code.len();
        code.push(Insn::if_true { target_ofs: 0 });

        // Both subexpressions are false
        code.push(Insn::push { val: Value::False });
        let jmp_idx = code.len();
        code.push(Insn::jump { target_ofs: 0 });

        // If true, short-circuit here
        patch_jump(code, if0_idx, code.len());
        patch_jump(code, if1_idx, code.len());
        code.push(Insn::push { val: Value::True });

        // Done label
        patch_jump(code, jmp_idx, code.len());

        return Ok(());
    }

    // If the rhs is a constant integer value
    if let Expr::Int64(int_val) = rhs.expr.as_ref() {
        match op {
            Add => {
                lhs.gen_code(fun, code, alloc)?;
                code.push(Insn::add_i64 { val: *int_val });
                return Ok(())
            }

            Sub => {
                lhs.gen_code(fun, code, alloc)?;
                code.push(Insn::add_i64 { val: -int_val });
                return Ok(())
            }

            _ => {}
        }
    }

    lhs.gen_code(fun, code, alloc)?;
    rhs.gen_code(fun, code, alloc)?;

    match op {
        BitAnd => code.push(Insn::bit_and),
        BitOr => code.push(Insn::bit_or),
        BitXor => code.push(Insn::bit_xor),
        LShift => code.push(Insn::lshift),
        RShift => code.push(Insn::rshift),

        Add => code.push(Insn::add),
        Sub => code.push(Insn::sub),
        Mul => code.push(Insn::mul),
        Div => code.push(Insn::div),
        IntDiv => code.push(Insn::div_int),
        Mod => code.push(Insn::modulo),

        Eq => code.push(Insn::eq),
        Ne => code.push(Insn::ne),
        Lt => code.push(Insn::lt),
        Le => code.push(Insn::le),
        Gt => code.push(Insn::gt),
        Ge => code.push(Insn::ge),

        _ => todo!("{:?}", op),
    }

    Ok(())
}

/// Generate a write to a variable
/// Assumes the value to be written is on top of the stack
fn gen_var_write(
    decl: &Decl,
    code: &mut Vec<Insn>,
)
{
    match *decl {
        Decl::Global { idx, .. } => {
            code.push(Insn::set_global { idx });
        }

        Decl::Local { idx, .. } => {
            code.push(Insn::set_local { idx });
        }

        Decl::Captured { idx, mutable } => {
            assert!(mutable == false);

            todo!();
        }

        _ => todo!()
    }
}

/// Generate a write to a variable
/// Pushes the value read on the stack
fn gen_var_read(
    decl: &Decl,
    code: &mut Vec<Insn>,
)
{
    match *decl {
        Decl::Fun { id } => {
            code.push(Insn::push { val: Value::Fun(id) });
        }

        Decl::Class { id } => {
            code.push(Insn::push { val: Value::Class(id) });
        }

        Decl::Global { idx, .. } => {
            code.push(Insn::get_global { idx });
        }

        Decl::Arg { idx, .. } => {
            code.push(Insn::get_arg { idx });
        }

        Decl::Local { idx, .. } => {
            code.push(Insn::get_local { idx });
        }

        Decl::Captured { idx, mutable } => {
            if mutable {
                todo!()
            }

            code.push(Insn::clos_get { idx });
        }
    }
}

fn gen_assign(
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    code: &mut Vec<Insn>,
    alloc: &mut Alloc,
    need_value: bool,
) -> Result<(), ParseError>
{
    //dbg!(lhs);
    //dbg!(rhs);

    match lhs.expr.as_ref() {
        Expr::Ref(decl) => {
            rhs.gen_code(fun, code, alloc)?;

            // If the output value is needed
            if need_value {
                code.push(Insn::dup);
            }

            gen_var_write(decl, code);
        }

        Expr::Member { base, field } => {
            let field = alloc.str_const(field.to_string());

            if need_value {
                rhs.gen_code(fun, code, alloc)?;
                base.gen_code(fun, code, alloc)?;
                code.push(Insn::getn { idx: 1 });
            } else {
                base.gen_code(fun, code, alloc)?;
                rhs.gen_code(fun, code, alloc)?;
            }

            code.push(Insn::set_field {
                field,
                class_id: Default::default(),
                slot_idx: Default::default(),
            });
        }

        Expr::Index { base, index } => {
            if need_value {
                rhs.gen_code(fun, code, alloc)?;
                base.gen_code(fun, code, alloc)?;
                index.gen_code(fun, code, alloc)?;
                code.push(Insn::getn { idx: 2 });
                code.push(Insn::set_index);
            } else {
                base.gen_code(fun, code, alloc)?;
                index.gen_code(fun, code, alloc)?;
                rhs.gen_code(fun, code, alloc)?;
                code.push(Insn::set_index);
            }
        }

        _ => todo!()
    }

    Ok(())
}
