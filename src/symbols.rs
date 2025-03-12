use std::collections::HashMap;
use crate::ast::*;
use crate::parsing::{ParseError};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Decl
{
    // Global function
    Fun { id: FunId },

    // Class declaration
    //Class { id: ClassId },

    // Global variable
    Global { idx: u32, mutable: bool },

    // Function argument
    Arg { idx: u32, src_fun: FunId },

    // Local variable in a function
    Local { idx: u32, src_fun: FunId, mutable: bool },

    // Variables captured by the current closure
    Captured { idx: u32, mutable: bool },
}

impl Decl
{
    fn is_mutable(&self) -> bool
    {
        match *self {
            Decl::Fun { .. } => false,
            Decl::Global { mutable, .. } => mutable,
            Decl::Arg { .. } => false,
            Decl::Local { mutable, .. } => mutable,
            Decl::Captured { mutable, .. } => mutable,
        }
    }
}

#[derive(Default)]
struct Scope
{
    decls: HashMap<String, Decl>,

    /// Next local variable slot index to assign
    /// this is only used for local variables
    next_idx: usize,
}

/// Represent an environment with multiple levels of scoping
#[derive(Default)]
struct Env
{
    scopes: Vec<Scope>,
}

impl Env
{
    fn push_scope(&mut self)
    {
        let num_scopes = self.scopes.len();
        let mut new_scope = Scope::default();

        if num_scopes > 0 {
            new_scope.next_idx = self.scopes[num_scopes - 1].next_idx;
        }

        self.scopes.push(new_scope);
    }

    fn pop_scope(&mut self)
    {
        self.scopes.pop();
    }

    /// Define a new local variable in the current scope
    fn define_local(&mut self, name: &str, mutable: bool, fun: &mut Function) -> Decl
    {
        let num_scopes = self.scopes.len();
        let top_scope = &mut self.scopes[num_scopes - 1];
        assert!(top_scope.decls.get(name).is_none());

        let decl = if fun.is_unit {
            Decl::Global {
                idx: top_scope.next_idx as u32,
                //src_fun: fun.id,
                mutable,
            }
        } else {
            Decl::Local {
                idx: top_scope.next_idx as u32,
                src_fun: fun.id,
                mutable,
            }
        };

        top_scope.next_idx += 1;
        if top_scope.next_idx > fun.num_locals {
            fun.num_locals = top_scope.next_idx;
        }

        top_scope.decls.insert(name.to_string(), decl.clone());
        decl
    }

    /// Define a new entity in the current scope
    fn define(&mut self, name: &str, decl: Decl) -> Decl
    {
        let num_scopes = self.scopes.len();
        let top_scope = &mut self.scopes[num_scopes - 1];

        assert!(
            top_scope.decls.get(name).is_none(),
            "two declarations with name \"{}\"",
            name
        );

        top_scope.decls.insert(name.to_string(), decl.clone());

        decl
    }

    fn lookup(&self, name: &str) -> Option<Decl>
    {
        let top_idx = self.scopes.len() - 1;

        for idx in (0..=top_idx).rev() {

            let scope = &self.scopes[idx];

            if let Some(decl) = scope.decls.get(name) {
                return Some(decl.clone());
            }
        }

        return None;
    }
}

impl Program
{
    pub fn resolve_syms(&mut self) -> Result<(), ParseError>
    {
        let mut env = Env::default();
        env.push_scope();

        // Process the unit function
        let mut main_unit = std::mem::take(&mut self.main_unit);
        main_unit.resolve_syms(self, &mut env)?;
        self.main_unit = main_unit;

        Ok(())
    }
}

impl Unit
{
    fn resolve_syms(&mut self, prog: &mut Program, env: &mut Env) -> Result<(), ParseError>
    {
        // Process the unit function
        let mut unit_fn = std::mem::take(prog.funs.get_mut(&self.unit_fn).unwrap());
        unit_fn.resolve_syms(prog, env)?;

        // Update the number of globals
        prog.num_globals += unit_fn.num_locals;

        // Move the unit function back on the program
        *prog.funs.get_mut(&self.unit_fn).unwrap() = unit_fn;

        Ok(())
    }
}

impl Function
{
    fn resolve_syms(&mut self, prog: &mut Program, env: &mut Env) -> Result<(), ParseError>
    {
        env.push_scope();

        // Declare the function arguments
        for (idx, param_name) in self.params.iter().enumerate() {
            let decl = Decl::Arg {
                idx: idx as u32,
                src_fun: self.id
            };
            env.define(param_name, decl);
        }

        let mut body = std::mem::take(&mut self.body);
        body.resolve_syms(prog, self, env)?;
        self.body = body;

        env.pop_scope();

        Ok(())
    }
}

impl StmtBox
{
    fn resolve_syms(
        &mut self,
        prog: &mut Program,
        fun: &mut Function,
        env: &mut Env
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_mut() {
            Stmt::Break | Stmt::Continue => {}

            Stmt::Return(expr) => {
                expr.resolve_syms(prog, fun, env)?;
            }

            Stmt::Expr(expr) => {
                expr.resolve_syms(prog, fun, env)?;
            }

            Stmt::Block(stmts) => {
                env.push_scope();

                // Pre-declare functions before symbols are resolved
                for stmt in stmts.iter_mut() {
                    if let Stmt::Let { mutable, var_name, init_expr, ref mut decl } = stmt.stmt.as_mut() {
                        if let Expr::Fun { fun_id, .. } = init_expr.expr.as_ref() {
                            let new_decl = if fun.is_unit && !*mutable {
                                env.define(var_name, Decl::Fun { id: *fun_id })
                            } else {
                                env.define_local(var_name, *mutable, fun)
                            };

                            *decl = Some(new_decl)
                        }
                    }
                }

                for stmt in stmts {
                    stmt.resolve_syms(prog, fun, env)?;
                }

                env.pop_scope();
            }

            Stmt::If { test_expr, then_stmt, else_stmt } => {
                test_expr.resolve_syms(prog, fun, env)?;
                then_stmt.resolve_syms(prog, fun, env)?;

                if else_stmt.is_some() {
                    else_stmt.as_mut().unwrap().resolve_syms(prog, fun, env)?;
                }
            }

            Stmt::While { test_expr, body_stmt } => {
                test_expr.resolve_syms(prog, fun, env)?;
                body_stmt.resolve_syms(prog, fun, env)?;
            }

            Stmt::Assert { test_expr } => {
                test_expr.resolve_syms(prog, fun, env)?;
            }

            // Variable declaration
            Stmt::Let { mutable, var_name, init_expr, decl } => {
                init_expr.resolve_syms(prog, fun, env)?;

                // Functions have already been pre-declared
                match init_expr.expr.as_ref() {
                    Expr::Fun { .. } => {}
                    _ => {
                        let new_decl = env.define_local(var_name, *mutable, fun);
                        *decl = Some(new_decl)
                    }
                }
            }

            //_ => todo!()
        }

        Ok(())
    }
}

impl ExprBox
{
    fn resolve_syms(
        &mut self,
        prog: &mut Program,
        fun: &mut Function,
        env: &mut Env
    ) -> Result<(), ParseError>
    {
        match self.expr.as_mut() {
            Expr::Nil { .. } => {}
            Expr::True { .. } => {}
            Expr::False { .. } => {}
            Expr::Int64 { .. } => {}
            Expr::Float64 { .. } => {}
            Expr::String { .. } => {}
            Expr::HostFn { .. } => {}

            Expr::Array { exprs, .. } => {
                for expr in exprs {
                    expr.resolve_syms(prog, fun, env)?;
                }
            }

            Expr::Object { pairs, .. } => {
                for (_, expr) in pairs {
                    expr.resolve_syms(prog, fun, env)?;
                }
            }

            Expr::Ident(name) => {
                //dbg!(&name);

                if let Some(mut decl) = env.lookup(name) {
                    // If this variable comes from another function,
                    // then it must be captured as a closure variable
                    let decl = match decl {
                        Decl::Arg { src_fun, .. } |
                        Decl::Local { src_fun, .. }
                        if src_fun != fun.id => {
                            // Register and get an index for the captured variable
                            let cell_idx = fun.reg_captured(&decl);

                            // Identify this as a captured closure variable
                            Decl::Captured {
                                idx: cell_idx,
                                mutable: decl.is_mutable()
                            }
                        },
                        _ => decl
                    };

                    *(self.expr) = Expr::Ref(decl);
                }
                else
                {
                    return ParseError::with_pos(
                        &format!("reference to undeclared identifier \"{}\"", name),
                        &self.pos
                    );
                }
            }

            Expr::Ref { .. } => panic!("unresolved ref"),

            Expr::Index { base, index } => {
                base.resolve_syms(prog, fun, env)?;
                index.resolve_syms(prog, fun, env)?;
            }

            Expr::Member { base, field } => {
                base.resolve_syms(prog, fun, env)?;
            }

            Expr::Unary { op, child, .. } => {
                child.resolve_syms(prog, fun, env)?;
            }

            Expr::Binary { op, lhs, rhs, .. } => {
                lhs.resolve_syms(prog, fun, env)?;
                rhs.resolve_syms(prog, fun, env)?;

                // If this is an assignment to a constant
                if *op == BinOp::Assign {
                    if let Expr::Ref(decl) = lhs.expr.as_ref() {
                        if !decl.is_mutable() {
                            return ParseError::with_pos(
                                &format!("assignment to immutable variable"),
                                &self.pos
                            );
                        }
                    }
                }
            }

            Expr::Ternary { test_expr, then_expr, else_expr, .. } => {
                test_expr.resolve_syms(prog, fun, env)?;
                then_expr.resolve_syms(prog, fun, env)?;
                else_expr.resolve_syms(prog, fun, env)?;
            }

            Expr::Call { callee, args, .. } => {
                callee.resolve_syms(prog, fun, env)?;
                for arg in args {
                    arg.resolve_syms(prog, fun, env)?;
                }
            }

            Expr::HostCall { fun_name, args, .. } => {
                for arg in args {
                    arg.resolve_syms(prog, fun, env)?;
                }
            }

            Expr::Fun { fun_id, captured } => {
                // Resolve symbols in the nested function
                let mut child_fun = std::mem::take(prog.funs.get_mut(fun_id).unwrap());
                child_fun.resolve_syms(prog, env)?;

                // For each variable captured by the nested function
                for (decl, idx) in &child_fun.captured {
                    // If this variable doesn't comes from this function,
                    // then it must be captured as a closure variable
                    match *decl {
                        Decl::Arg { src_fun, .. } |
                        Decl::Local { src_fun, .. }
                        if src_fun != fun.id => {
                            fun.reg_captured(&decl);
                        },
                        _ =>{}
                    };

                    captured.push(decl.clone());
                }

                // Put the child function back in place
                *prog.funs.get_mut(fun_id).unwrap() = child_fun;
            }

            //_ => todo!("{:?}", self)
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::parsing::Input;
    use crate::parser::parse_program;

    fn succeeds(src: &str)
    {
        dbg!(src);
        let mut input = Input::new(&src, "src");
        let mut prog = parse_program(&mut input).unwrap();
        prog.resolve_syms().unwrap();
    }

    fn fails(src: &str)
    {
        dbg!(src);
        let mut input = Input::new(&src, "src");
        let mut prog = parse_program(&mut input).unwrap();
        assert!(prog.resolve_syms().is_err());
    }

    fn parse_file(file_name: &str)
    {
        dbg!(file_name);
        let mut prog = crate::parser::parse_file(file_name).unwrap();
        prog.resolve_syms().unwrap();
    }

    #[test]
    fn basics()
    {
        succeeds("");
        succeeds("let foo = fun() {};");
        succeeds("fun foo(a) { return a; }");

        // Local variables
        succeeds("fun main() { let a = 0; }");
        succeeds("fun foo(a) { let a = 0; }");

        // Infix expressions
        succeeds("fun foo(a, b) { return a + b; }");

        // Two functions with the same parameter name
        succeeds("fun foo(a) {} fun bar(a) {}");

        // Reference to global
        succeeds("let g = 1; fun foo() { return g; }");

        // Undefined local
        fails("fun foo() { return g; }")
    }

    #[test]
    fn globals()
    {
        succeeds("let g = 5; fun main() { return g; }");
        succeeds("let g = 5; fun main() { return g + 1; }");
        succeeds("let global_str = \"foo\"; fun main() {}");

        // Undefined global
        fails("g;");
    }

    #[test]
    fn immutable()
    {
        succeeds("let var g = 5; g = 6;");
        succeeds("let var f = fun() {}; f = 6;");

        fails("let g = 5; g = 6;");
        fails("fun f() {} f = 6;");
    }

    #[test]
    fn keywords()
    {
        fails("letx = 1;");
    }

    #[test]
    fn calls()
    {
        succeeds("fun foo() {} fun main() { foo(); }");
    }

    /*
    #[test]
    fn test_files()
    {
        //parse_file("tests/call_ident.pls");
    }
    */
}
