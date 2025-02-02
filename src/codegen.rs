use std::cmp::max;
use crate::ast::*;
use crate::parsing::{ParseError};
use crate::symbols::{Decl};
use crate::vm::{Insn, Value};

/// Compiled function object
#[derive(Copy, Clone)]
pub struct CompiledFun
{
    pub entry_pc: usize,

    pub num_params: usize,

    pub num_locals: usize,
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

    pub fn gen_code(&self, code: &mut Vec<Insn>) -> Result<CompiledFun, ParseError>
    {
        // Entry address of the compiled function
        let entry_pc = code.len();

        // Allocate stack slots for the local variables
        for i in 0..self.num_locals {
            code.push(Insn::push { val: Value::Nil });
        }

        // Compile the function body
        self.body.gen_code(self, &mut vec![], &mut vec![], code)?;

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
        break_idxs: &Vec<usize>,
        cont_idxs: &Vec<usize>,
        code: &mut Vec<Insn>,
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_ref() {
            /*
            Stmt::Expr(expr) => {
                match expr.expr.as_ref() {
                    // For assignment expressions as statements,
                    // avoid generating output that we would then need to pop
                    Expr::Binary { op: BinOp::Assign, lhs, rhs } => {
                        gen_assign(lhs, rhs, fun, sym, code, out, false)?;
                    }

                    /*
                    // For asm expressions with void output type, don't pop
                    // the output because no output is produced
                    Expr::Asm { out_type: Type::Void, .. } => {
                        expr.gen_code(fun, sym, code, out)?;
                    }
                    */

                    _ => {
                        expr.gen_code(fun, sym, code, out)?;
                        code.insn("pop");
                    }
                }
            }

            Stmt::Break => {
                match break_label {
                    Some(label) => code.insn_s("jump", label),
                    None => return ParseError::msg_only("break outside of loop context")
                }
            }

            Stmt::Continue => {
                match cont_label {
                    Some(label) => code.insn_s("jump", label),
                    None => return ParseError::msg_only("continue outside of loop context")
                }
            }
            */

            Stmt::Return(expr) => {
                expr.gen_code(fun, code)?;
                code.push(Insn::ret);
            }

            Stmt::Block(stmts) => {
                for stmt in stmts {
                    stmt.gen_code(fun, break_idxs, cont_idxs, code)?;
                }
            }




            Stmt::If { test_expr, then_stmt, else_stmt } => {
                // Compile the test expression
                test_expr.gen_code(fun, code)?;




                // If false, jump to else stmt
                //code.push(Insn::if_false { target: 0 });



                if else_stmt.is_some() {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, code)?;
                    //code.jump(&join_label);

                    //code.label(&false_label);

                    else_stmt.as_ref().unwrap().gen_code(fun, break_idxs, cont_idxs, code)?;

                    //code.label(&join_label);
                }
                else
                {
                    then_stmt.gen_code(fun, break_idxs, cont_idxs, code)?;


                    // TODO: we need to patch the if_false
                    //code.label(&false_label);
                }



            }






            /*
            Stmt::While { test_expr, body_stmt } => {
                let loop_label = sym.gen_sym("while_loop");
                let break_label = sym.gen_sym("while_break");

                code.label(&loop_label);
                test_expr.gen_code(fun, sym, code, out)?;
                code.insn_s("if_false", &break_label);

                body_stmt.gen_code(
                    fun,
                    &Some(break_label.clone()),
                    &Some(loop_label.clone()),
                    sym,
                    code,
                    out,
                )?;

                code.jump(&loop_label);
                code.label(&break_label);
            }
            */

            /*
            Stmt::Assert { test_expr } => {
                let pass_label = sym.gen_sym("assert_pass");
                test_expr.gen_code(fun, sym, code, out)?;
                code.insn_s("if_true", &pass_label);

                code.insn_s("push", &format!("assertion failed at {}\n", self.pos));

                code.add_insn(vec![
                    "'call_host'".to_string(),
                    "'print_str'".to_string(),
                    "1".to_string(),
                ]);

                code.insn("panic");

                code.label(&pass_label);
            }

            // Variable declaration
            Stmt::Let { mutable, var_name, init_expr, decl } => {
                init_expr.gen_code(fun, sym, code, out)?;

                match decl.as_ref().unwrap() {
                    Decl::Global { name, fun_id } => {
                        let global_obj_name = format!("@global_{}", fun.id);
                        code.push(&global_obj_name);

                        // FIXME: need to distinguish def_const
                        code.insn_s("obj_set", &var_name);
                    }

                    Decl::Arg { .. } => panic!(),

                    Decl::Local { idx, fun_id } => {
                        // TODO: handle captured closure vars

                        code.insn_i("set_local", *idx as i64);
                    }
                }
            }
            */

            _ => todo!()
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
    ) -> Result<(), ParseError>
    {
        match self.expr.as_ref() {
            Expr::Nil => code.push(Insn::push { val: Value::Nil }),
            Expr::True => code.push(Insn::push { val: Value::True }),
            Expr::False => code.push(Insn::push { val: Value::False }),
            Expr::Int64(v) => code.push(Insn::push { val: Value::Int64(*v) }),

            /*
            Expr::Float64(v) => {
                code.push(&format!("{}", v));
            }

            Expr::String(s) => {
                code.insn_s("push", s);
            }

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

            /*
            Expr::Object { extensible, fields } => {
                return gen_obj_expr(
                    *extensible,
                    fields,
                    fun,
                    sym,
                    code,
                    out,
                );
            }
            */

            /*
            Expr::Ref(decl) => {
                match decl {
                    Decl::Arg { idx, .. } => {
                        code.insn_i("get_arg", *idx as i64);
                    }

                    Decl::Local { idx, .. } => {
                        code.insn_i("get_local", *idx as i64);
                    }

                    Decl::Global { name, fun_id } => {
                        let global_obj_name = format!("@global_{}", fun_id);
                        code.push(&global_obj_name);
                        code.insn_s("obj_get", &name);
                    }
                }
            }
            */

            /*
            Expr::Index { base, index } => {
                base.gen_code(fun, sym, code, out)?;
                index.gen_code(fun, sym, code, out)?;
                code.insn("arr_get");
            }
            */

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

            /*
            Expr::Member { base, field } => {
                base.gen_code(fun, sym, code, out)?;
                code.insn_s("obj_get", &field);
            }
            */

            /*
            Expr::Unary { op, child } => {
                child.gen_code(fun, sym, code, out)?;

                match op {
                    UnOp::Minus => {
                        code.push("-1");
                        code.insn("mul");
                    }

                    // Logical negation
                    UnOp::Not => {
                        code.insn("not");
                    }

                    UnOp::TypeOf => {
                        code.insn("typeof");
                    }
                }
            },
            */

            Expr::Binary { op, lhs, rhs } => {
                gen_bin_op(op, lhs, rhs, fun, code)?;
            }

            /*
            Expr::Ternary { test_expr, then_expr, else_expr } => {
                let false_label = sym.gen_sym("and_false");
                let done_label = sym.gen_sym("and_done");

                test_expr.gen_code(fun, sym, code, out)?;
                out.push_str(&format!("jz {};\n", false_label));

                // Evaluate the then expression
                then_expr.gen_code(fun, sym, code, out)?;
                out.push_str(&format!("jmp {};\n", done_label));

                // Evaluate the else expression
                out.push_str(&format!("{}:\n", false_label));
                else_expr.gen_code(fun, sym, code, out)?;

                out.push_str(&format!("{}:\n", done_label));
            }
            */

            /*
            Expr::HostCall { fun_name, args } => {
                for arg in args {
                    arg.gen_code(fun, sym, code, out)?;
                }

                code.add_insn(vec![
                    "'call_host'".to_string(),
                    format!("'{}'", fun_name),
                    format!("{}", args.len())
                ]);
            }

            Expr::Call { callee, args } => {

                // If the callee has the form a.b
                if let Expr::Member { base, field } = callee.expr.as_ref() {
                    // Evaluate the self argument
                    base.gen_code(fun, sym, code, out)?;

                    for arg in args {
                        arg.gen_code(fun, sym, code, out)?;
                    }

                    // Read the method from the object
                    code.insn_i("getn", args.len() as i64);
                    code.insn_s("obj_get", &field);

                    // Pass one extra argument (self)
                    code.insn_i("call", 1 + args.len() as i64);
                } else {
                    for arg in args {
                        arg.gen_code(fun, sym, code, out)?;
                    }

                    callee.gen_code(fun, sym, code, out)?;
                    code.insn_i("call", args.len() as i64);
                }
            }

            Expr::Fun(child_fun) => {
                child_fun.gen_code(sym, out)?;

                let fun_sym = child_fun.fun_sym();
                code.push(&fun_sym);
            }
            */

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

/*
// Generate code for an object literal expression
fn gen_obj_expr(
    extensible: bool,
    fields: &Vec<(bool, String, ExprBox)>,
    fun: &Function,
    sym: &mut SymGen,
    code: &mut ByteCode,
    out: &mut String,
) -> Result<(), ParseError>
{
    code.insn("obj_new");

    // For each field
    for (mutable, name, expr) in fields {
        expr.gen_code(fun, sym, code, out)?;

        code.insn_i("getn", 1);

        if *mutable {
            code.insn_s("obj_set", name);
        } else {
            code.insn_s("obj_def", name);
        }
    }

    if !extensible {
        code.insn("dup");
        code.insn("obj_seal");
    }

    Ok(())
}
*/

fn gen_bin_op(
    op: &BinOp,
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    code: &mut Vec<Insn>,
) -> Result<(), ParseError>
{
    use BinOp::*;

    /*
    // Assignments are different from other kinds of expressions
    // because we don't evaluate the lhs the same way
    if *op == Assign {
        gen_assign(lhs, rhs, fun, sym, code, out, true)?;
        return Ok(());
    }
    */

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

    lhs.gen_code(fun, code)?;
    rhs.gen_code(fun, code)?;

    match op {
        /*
        BitAnd => code.insn("bit_and"),
        BitOr => code.insn("bit_or"),
        BitXor => code.insn("bit_xor"),
        LShift => code.insn("lshift"),
        RShift => code.insn("rshift"),
        */

        Add => code.push(Insn::add),
        Sub => code.push(Insn::sub),
        Mul => code.push(Insn::mul),

        /*
        Eq => code.insn("eq"),
        Ne => code.insn("ne"),
        Lt => code.insn("lt"),
        Le => code.insn("le"),
        Gt => code.insn("gt"),
        Ge => code.insn("ge"),
        */

        _ => todo!("{:?}", op),
    }

    Ok(())
}

/*
fn gen_assign(
    lhs: &ExprBox,
    rhs: &ExprBox,
    fun: &Function,
    sym: &mut SymGen,
    code: &mut ByteCode,
    out: &mut String,
    need_value: bool,
) -> Result<(), ParseError>
{
    //dbg!(lhs);
    //dbg!(rhs);

    rhs.gen_code(fun, sym, code, out)?;

    // If the output value is needed
    if need_value {
        code.insn("dup");
    }

    match lhs.expr.as_ref() {
        Expr::Ref(decl) => {
            match decl {
                Decl::Arg { idx, .. } => {
                    code.insn_i("set_arg", *idx as i64);
                }

                Decl::Local { idx, .. } => {
                    code.insn_i("set_local", *idx as i64);
                }

                Decl::Global { name, .. } => {
                    let global_obj_name = format!("@global_{}", fun.id);
                    code.push(&global_obj_name);
                    code.insn_s("obj_set", &name);
                }
            }
        }

        Expr::Member { base, field } => {
            base.gen_code(fun, sym, code, out)?;
            code.insn_s("obj_set", &field);
        }

        Expr::Index { base, index } => {
            base.gen_code(fun, sym, code, out)?;
            index.gen_code(fun, sym, code, out)?;
            code.insn("arr_set");
        }

        _ => todo!()
    }

    Ok(())
}
*/

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