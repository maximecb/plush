use std::collections::HashMap;
use crate::ast::*;
use crate::parsing::{ParseError};

/// Global/variable/function declaration
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Decl
{
    Global { name: String, fun_id: FunId },
    Arg { idx: usize, fun_id: FunId },
    Local { idx: usize, fun_id: FunId },
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
    fn define_local(&mut self, name: &str, fun: &mut Function) -> Decl
    {
        let num_scopes = self.scopes.len();
        let top_scope = &mut self.scopes[num_scopes - 1];
        assert!(top_scope.decls.get(name).is_none());

        let decl = Decl::Local {
            idx: top_scope.next_idx,
            fun_id: fun.id,
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
        let mut unit_fn = std::mem::take(self.funs.get_mut(&self.main_fn).unwrap());
        //unit_fn.resolve_syms(self, &mut env)?;
        *self.funs.get_mut(&self.main_fn).unwrap() = unit_fn;

        Ok(())
    }
}





impl Unit
{
    pub fn resolve_syms(&mut self) -> Result<(), ParseError>
    {
        let mut env = Env::default();
        env.push_scope();


        // FIXME:
        //self.unit_fn.resolve_syms(&mut env)?;



        Ok(())
    }
}

impl Function
{
    fn resolve_syms(&mut self, env: &mut Env) -> Result<(), ParseError>
    {
        env.push_scope();

        // Declare the function arguments
        for (idx, param_name) in self.params.iter().enumerate() {
            let decl = Decl::Arg { idx, fun_id: self.id };
            env.define(param_name, decl);
        }

        let mut body = std::mem::take(&mut self.body);
        body.resolve_syms(self, env)?;
        self.body = body;

        env.pop_scope();

        Ok(())
    }
}

impl StmtBox
{
    fn resolve_syms(
        &mut self,
        fun: &mut Function,
        env: &mut Env
    ) -> Result<(), ParseError>
    {
        match self.stmt.as_mut() {
            Stmt::Break | Stmt::Continue => {}

            Stmt::Return(expr) => {
                expr.resolve_syms(fun, env)?;
            }

            Stmt::Expr(expr) => {
                expr.resolve_syms(fun, env)?;
            }

            Stmt::Block(stmts) => {
                env.push_scope();

                for stmt in stmts {
                    stmt.resolve_syms(fun, env)?;
                }

                env.pop_scope();
            }

            Stmt::If { test_expr, then_stmt, else_stmt } => {
                test_expr.resolve_syms(fun, env)?;
                then_stmt.resolve_syms(fun, env)?;

                if else_stmt.is_some() {
                    else_stmt.as_mut().unwrap().resolve_syms(fun, env)?;
                }
            }

            Stmt::While { test_expr, body_stmt } => {
                test_expr.resolve_syms(fun, env)?;
                body_stmt.resolve_syms(fun, env)?;
            }

            Stmt::Assert { test_expr } => {
                test_expr.resolve_syms(fun, env)?;
            }

            // Variable declaration
            Stmt::Let { mutable, var_name, init_expr, decl } => {
                // If this is not a function declaration
                // Resolve symbols in the initialization expression
                // before the new definition is added
                match init_expr.expr.as_ref() {
                    Expr::Fun { .. } => {}
                    _ => {
                        init_expr.resolve_syms(fun, env)?;
                    }
                }

                // If we're in a unit-level function
                let new_decl = if fun.is_unit {
                    env.define(
                        var_name,
                        Decl::Global{
                            name: var_name.to_string(),
                            fun_id: fun.id
                        }
                    )
                } else {
                    env.define_local(var_name, fun)
                };

                // If this is a function declaration
                // Resolve symbols in the initialization expression
                // after the new definition is added to allow recursion
                match init_expr.expr.as_ref() {
                    Expr::Fun { .. } => {
                        init_expr.resolve_syms(fun, env)?;
                    }
                    _ => {}
                }

                // Store the declaration for this let statement
                *decl = Some(new_decl)
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
        fun: &mut Function,
        env: &mut Env
    ) -> Result<(), ParseError>
    {
        match self.expr.as_mut() {
            Expr::Nil { .. } => {}
            Expr::True { .. } => {}
            Expr::False { .. } => {}
            Expr::Int { .. } => {}
            Expr::Float64 { .. } => {}
            Expr::String { .. } => {}

            Expr::Array { exprs, .. } => {
                for expr in exprs {
                    expr.resolve_syms(fun, env)?;
                }
            }

            Expr::Object { fields, .. } => {
                for (_, _, expr) in fields {
                    expr.resolve_syms(fun, env)?;
                }
            }

            Expr::Ident(name) => {
                //dbg!(&name);

                if let Some(decl) = env.lookup(name) {
                    /*
                    // If this variable comes from another function,
                    // then it must be captured as a closure variable
                    match decl {
                        Decl::Local { fun_id, .. } | Decl::Arg { fun_id, .. } if fun_id != fun.id => {
                            fun.reg_captured(&decl);
                        }
                        _ => {}
                    };
                    */

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
                base.resolve_syms(fun, env)?;
                index.resolve_syms(fun, env)?;
            }

            Expr::Member { base, field } => {
                base.resolve_syms(fun, env)?;
            }

            Expr::Unary { op, child, .. } => {
                child.resolve_syms(fun, env)?;
            }

            Expr::Binary { op, lhs, rhs, .. } => {
                lhs.resolve_syms(fun, env)?;
                rhs.resolve_syms(fun, env)?;
            }

            Expr::Call { callee, args, .. } => {
                callee.resolve_syms(fun, env)?;
                for arg in args {
                    arg.resolve_syms(fun, env)?;
                }
            }

            Expr::HostCall { fun_name, args, .. } => {
                for arg in args {
                    arg.resolve_syms(fun, env)?;
                }
            }

            Expr::Fun(child_fun) => {

                todo!();

                //child_fun.resolve_syms(env)?;

                /*
                // For each variable captured by the nested function
                for (decl, idx) in &child_fun.captured {
                    // If this variable comes from another function,
                    // then it must be captured as a closure variable
                    match decl {
                        Decl::Local { fun_id, .. } | Decl::Arg { fun_id, .. } if *fun_id != fun.id => {
                            fun.reg_captured(&decl);
                        }
                        _ => {}
                    };
                }
                */
            }

            //_ => todo!("{:?}", self)
        }

        Ok(())
    }
}
