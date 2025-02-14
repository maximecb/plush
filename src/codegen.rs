use std::cmp::max;
use crate::ast::*;
use crate::parsing::{ParseError};
use crate::symbols::{Decl, DeclKind};
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

        // Allocate stack slots for the local variables
        for i in 0..self.num_locals {
            code.push(Insn::push { val: Value::Nil });
        }

        // Compile the function body
        self.body.gen_code(self, &mut vec![], &mut vec![], code, alloc)?;

        // If the body needs a final return
        if self.needs_final_return() {
            code.push(Insn::push { val: Value::Nil });
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

            /*
            Stmt::Break => {
                match break_label {
                    Some(label) => code.insn_s("jump", label),
                    None => return ParseError::msg_only("break outside of loop context")
                }
            }
            */

            /*
            Stmt::Continue => {
                match cont_label {
                    Some(label) => code.insn_s("jump", label),
                    None => return ParseError::msg_only("continue outside of loop context")
                }
            }
            */

            Stmt::Return(expr) => {
                expr.gen_code(fun, code, alloc)?;
                code.push(Insn::ret);
            }

            Stmt::Block(stmts) => {
                // For each function declaration
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

            Stmt::While { test_expr, body_stmt } => {
                let mut break_idxs = Vec::new();
                let mut cont_idxs = Vec::new();

                // Continue will jump here
                let cont_idx = code.len();

                // Evaluate the test expression
                test_expr.gen_code(fun, code, alloc)?;

                break_idxs.push(code.len());
                code.push(Insn::if_false { target_ofs: 0 });

                body_stmt.gen_code(
                    fun,
                    &mut break_idxs,
                    &mut cont_idxs,
                    code,
                    alloc,
                )?;

                // Jump back to the loop test
                cont_idxs.push(code.len());
                code.push(Insn::jump { target_ofs: 0 });

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

                /*
                code.insn_s("push", &format!("assertion failed at {}\n", self.pos));
                code.add_insn(vec![
                    "'call_host'".to_string(),
                    "'print_str'".to_string(),
                    "1".to_string(),
                ]);
                */

                code.push(Insn::panic);
                patch_jump(code, if_idx, code.len());
            }

            // Variable declaration
            Stmt::Let { mutable, var_name, init_expr, decl } => {
                match init_expr.expr.as_ref() {
                    Expr::Fun { fun_id, captured } => {

                        // TODO: we need a way to eval the closure decl
                        let decl = decl.as_ref().unwrap();
                        assert!(decl.kind == DeclKind::Local);
                        code.push(Insn::get_local { idx: decl.idx.try_into().unwrap() });

                        // For each variable captured by the closure
                        for (idx, decl) in captured.iter().enumerate() {

                            code.push(Insn::dup);

                            // TODO: here we need to be able to eval the ref
                            // we need a gen_ref()
                            if decl.fun_id != fun.id {
                                panic!();
                            }
                            code.push(Insn::get_local { idx: decl.idx });

                            code.push(Insn::clos_set { idx: idx as u32 });
                        }
                    }

                    _ => init_expr.gen_code(fun, code, alloc)?
                }

                // Initialize the local variable
                gen_var_write(decl.as_ref().unwrap(), code);
            }

            _ => todo!("{:?}", self.stmt)
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

            /*
            Expr::Array { frozen, exprs } => {
                return gen_arr_expr(
                    *frozen,
                    exprs,
                    fun,
                    sym,
                    code,
                    out,
                );
            }
            */

            Expr::Object { fields } => {
                return gen_obj_expr(
                    fields,
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
                code.push(Insn::arr_get);
            }

            /*
            Expr::Member { base, field } if field == "len" => {
                // Evaluate the base
                base.gen_code(fun, sym, code, out)?;

                let not_obj = sym.gen_sym("not_obj");
                let len_done = sym.gen_sym("len_done");

                // Is this an object?
                code.insn("dup");
                code.insn("typeof");
                code.insn_s("push", "object");
                code.insn("eq");

                code.insn_s("if_false", &not_obj);

                // Object case, get field
                code.insn_s("obj_get", &field);
                code.jump(&len_done);

                // Not object case
                code.label(&not_obj);

                // Get array/string length
                code.insn("arr_len");

                code.label(&len_done);
            }
            */

            Expr::Member { base, field } => {
                base.gen_code(fun, code, alloc)?;
                let field = alloc.str_const(field.clone());
                code.push(Insn::obj_get { field });
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

                    /*
                    UnOp::TypeOf => {
                        code.insn("typeof");
                    }
                    */

                    _ => todo!()
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

                // If the callee has the form a.b
                if let Expr::Member { base, field } = callee.expr.as_ref() {
                    // Evaluate the self argument
                    base.gen_code(fun, code, alloc)?;

                    for arg in args {
                        arg.gen_code(fun, code, alloc)?;
                    }

                    // Read the method from the object
                    code.push(Insn::getn { idx: argc });
                    let field = alloc.str_const(field.clone());
                    code.push(Insn::obj_get { field });

                    // Pass one extra argument (self)
                    code.push(Insn::call { argc: argc + 1 });
                } else {
                    for arg in args {
                        arg.gen_code(fun, code, alloc)?;
                    }

                    callee.gen_code(fun, code, alloc)?;
                    code.push(Insn::call { argc });
                }
            }

            // Closure expression
            Expr::Fun { fun_id, captured } => {
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

/*
// Generate code for an array literal expression
fn gen_arr_expr(
    frozen: bool,
    exprs: &Vec<ExprBox>,
    fun: &Function,
    sym: &mut SymGen,
    code: &mut ByteCode,
    out: &mut String,
) -> Result<(), ParseError>
{
    code.insn_i("arr_new", exprs.len() as i64);

    for expr in exprs {
        expr.gen_code(fun, sym, code, out)?;
        code.insn_i("getn", 1);
        code.insn("arr_push");
    }

    if frozen {
        code.insn("dup");
        code.insn("arr_freeze");
    }

    Ok(())
}
*/

// Generate code for an object literal expression
fn gen_obj_expr(
    fields: &Vec<(bool, String, ExprBox)>,
    fun: &Function,
    code: &mut Vec<Insn>,
    alloc: &mut Alloc,
) -> Result<(), ParseError>
{
    code.push(Insn::obj_new);

    // For each field
    for (mutable, name, expr) in fields {
        code.push(Insn::dup);

        expr.gen_code(fun, code, alloc)?;

        let field_name = alloc.str_const(name.clone());
        if *mutable {
            code.push(Insn::obj_set { field: field_name });
        } else {
            code.push(Insn::obj_def { field: field_name });
        }
    }

    code.push(Insn::dup);
    code.push(Insn::obj_seal);

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

    /*
    // Logical AND (a && b)
    if *op == And {
        let false_label = sym.gen_sym("and_false");
        let done_label = sym.gen_sym("and_done");

        // If a is false, the expression evaluates to false
        lhs.gen_code(fun, sym, code, out)?;
        code.insn_s("if_false", &false_label);

        // Evaluate the rhs
        rhs.gen_code(fun, sym, code, out)?;
        code.insn_s("if_false", &false_label);

        // Both subexpressions are true
        code.push("true");
        code.jump(&done_label);

        code.label(&false_label);
        code.push("false");

        code.label(&done_label);

        return Ok(());
    }
    */

    /*
    // Logical OR (a || b)
    if *op == Or {
        let true_label = sym.gen_sym("or_true");
        let done_label = sym.gen_sym("or_done");

        // If a is true, the expression evaluates to true
        lhs.gen_code(fun, sym, code, out)?;
        code.insn_s("if_true", &true_label);

        // Evaluate the rhs
        rhs.gen_code(fun, sym, code, out)?;
        code.insn_s("if_true", &true_label);

        // Both subexpressions are false
        code.push("false");
        code.jump(&done_label);

        code.label(&true_label);
        code.push("true");

        code.label(&done_label);

        return Ok(());
    }
    */

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
    match decl.kind {
        DeclKind::Local => {
            code.push(Insn::set_local { idx: decl.idx });
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
    match decl.kind {
        DeclKind::Arg => {
            code.push(Insn::get_arg { idx: decl.idx });
        }

        DeclKind::Local => {
            code.push(Insn::get_local { idx: decl.idx });
        }

        DeclKind::Captured => {
            if decl.mutable {
                todo!()
            }

            code.push(Insn::clos_get { idx: decl.idx });
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
                code.push(Insn::obj_set { field });
            } else {
                base.gen_code(fun, code, alloc)?;
                rhs.gen_code(fun, code, alloc)?;
                code.push(Insn::obj_set { field });
            }
        }

        Expr::Index { base, index } => {
            if need_value {
                rhs.gen_code(fun, code, alloc)?;
                base.gen_code(fun, code, alloc)?;
                index.gen_code(fun, code, alloc)?;
                code.push(Insn::getn { idx: 2 });
                code.push(Insn::arr_set);
            } else {
                base.gen_code(fun, code, alloc)?;
                index.gen_code(fun, code, alloc)?;
                rhs.gen_code(fun, code, alloc)?;
                code.push(Insn::arr_set);
            }
        }

        _ => todo!()
    }

    Ok(())
}

/*
#[cfg(test)]
mod tests
{
    use super::*;

    fn gen_ok(src: &str) -> String
    {
        use crate::parsing::Input;
        use crate::parser::parse_unit;

        dbg!(src);
        let mut input = Input::new(&src, "src");
        let mut unit = parse_unit(&mut input).unwrap();
        unit.resolve_syms().unwrap();
        //dbg!(&unit.fun_decls[0]);
        unit.gen_code().unwrap()
    }

    fn compile_file(file_name: &str)
    {
        use crate::parsing::Input;
        use crate::parser::parse_unit;

        dbg!(file_name);
        let mut input = Input::from_file(file_name).unwrap();
        //println!("{}", output);

        let mut unit = parse_unit(&mut input).unwrap();
        unit.resolve_syms().unwrap();
        unit.gen_code().unwrap();
    }

    #[test]
    fn basics()
    {
        gen_ok("");
        gen_ok("{}");
        gen_ok("{} {}");
        gen_ok("1;");
        gen_ok("1.5;");
        gen_ok("-77;");
        gen_ok("true;");
        gen_ok("false;");
        gen_ok("none;");
    }

    fn unary()
    {
        gen_ok("typeof 'str';");
    }

    fn arith()
    {
        gen_ok("1 + 2;");
        gen_ok("1 + 2 * 3;");
    }

    fn call_host()
    {
        gen_ok("$print_endl();");
        gen_ok("$print_i64(1 + 2);");
    }

    #[test]
    fn globals()
    {
        gen_ok("let x = 3;");
        gen_ok("let x = 3; x;");
        gen_ok("let var x = 3;");
        gen_ok("let var x = 3; x = 5;");
        gen_ok("let var x = 3; x = x + 1; x;");
    }

    #[test]
    fn functions()
    {
        gen_ok("let f = fun() {};");
        gen_ok("let f = fun(x, y) {};");
        gen_ok("let f = fun(x, y) { return x + y; };");
        gen_ok("let f = fun(x, y) { return x + y; };");
        gen_ok("fun f() {}");
    }

    /*
    #[test]
    fn call_ret()
    {
        gen_ok("void foo() {} void bar() {}");
        gen_ok("void foo() {} void bar() { return foo(); } ");
        gen_ok("void print_i64(i64 v) {} void bar(u64 v) { print_i64(v); }");
    }
    */

    /*
    #[test]
    fn var_arg()
    {
        gen_ok("void foo(int x, ...) {} void bar() { foo(1); }");
        gen_ok("void foo(int x, ...) {} void bar() { foo(1, 2); }");
    }
    */

    #[test]
    fn strings()
    {
        gen_ok("let str = 'foo';");
        gen_ok("let str = \"foo\\nbar\";");
    }

    #[test]
    fn arrays()
    {
        gen_ok("let a = [];");
        gen_ok("let a = *[];");
        gen_ok("let a = *[1, 2, 3];");

        //gen_ok("let a = *[1, 2, 3]; a[0];");
        //gen_ok("let a = *[1, 2, 3]; a[0] = 7;");
    }

    #[test]
    fn if_else()
    {
        gen_ok("let a = 0; { if (a) {} }");
        gen_ok("let a = 0; { if (a) {} else {} }");
        gen_ok("let a = 0; let b = 1; { if (a || b) {} }");
        gen_ok("let a = 0; let b = 1; { if (a && b) {} }");
        gen_ok("let a = 0; let b = 1; { if (a && !b) {} }");
    }

    #[test]
    fn while_stmt()
    {
        gen_ok("let i = 0; let n = 10; while (i < n) i = i + 1;");
    }

    #[test]
    fn assert_stmt()
    {
        gen_ok("assert(true);");
        gen_ok("let a = true; let b = false; assert(a && b);");
    }

    #[test]
    fn compile_files()
    {
        // Make sure that we can compile all the examples
        for file in std::fs::read_dir("./examples").unwrap() {
            let file_path = file.unwrap().path().display().to_string();
            if file_path.ends_with(".pls") {
                println!("{}", file_path);
                compile_file(&file_path);
            }
        }
    }
}
*/