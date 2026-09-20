use std::io::Write;
use crate::ast::*;
use crate::lexer::{Lexer, ParseError};
use crate::parser::{parse_unit, unit_key};
use crate::symbols::Env;
use crate::value::Value;
use crate::vm::VM;

/// Check if a parse error occurred at the end of the input,
/// meaning more input could make it parse successfully
fn at_end_of_input(src: &str, err: &ParseError) -> bool
{
    let line_no = 1 + src.matches('\n').count() as u32;
    let last_line = src.rsplit('\n').next().unwrap_or("");
    let col_no = 1 + last_line.chars().count() as u32;
    err.pos.line_no() == line_no && err.pos.col_no() == col_no
}

/// Make the unit function return the value of its last statement,
/// if that statement is an expression
fn return_last_expr(prog: &mut Program, unit_fn: FunId)
{
    let fun = prog.funs.get_mut(&unit_fn).unwrap();

    if let Stmt::Block(stmts) = fun.body.stmt.as_mut() {
        if let Some(last) = stmts.last_mut() {
            if let Stmt::Expr(expr) = last.stmt.as_mut() {
                let expr = std::mem::replace(expr, ExprBox::new(Expr::Nil, last.pos));
                *last.stmt = Stmt::Return(expr);
            }
        }
    }
}

/// Run the read-eval-print loop
pub(crate) fn run()
{
    let mut prog = Program::new();
    prog.repl = true;

    let vm = VM::new(prog);
    let mut actor = VM::new_main_actor(&vm);
    let mut env = Env::new();

    // Source code of the entry being typed
    let mut src = String::new();
    let mut entry_no = 1;

    println!("Plush {} REPL, type exit or press Ctrl+D to quit", env!("CARGO_PKG_VERSION"));

    loop {
        print!("{}", if src.is_empty() { "> " } else { "... " });
        let _ = std::io::stdout().flush();

        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => {
                println!();
                break;
            }
            Ok(_) => {}
        }

        if src.is_empty() && matches!(line.trim(), "exit" | "quit") {
            break;
        }

        // An empty line ends an incomplete entry
        let blank_line = line.trim().is_empty();
        if blank_line && src.is_empty() {
            continue;
        }
        src.push_str(&line);

        let src_name = format!("<repl:{}>", entry_no);
        let mut vm_ref = vm.lock().unwrap();
        let prog = vm_ref.prog_mut();

        let mut input = Lexer::new(&src, &src_name);
        let unit_fn = match parse_unit(&mut input, prog) {
            Ok(unit_fn) => unit_fn,
            Err(err) => {
                // Keep reading lines if the entry is incomplete
                if !blank_line && at_end_of_input(&src, &err) {
                    continue;
                }

                println!("Parse error: {}", err);
                src.clear();
                entry_no += 1;
                continue;
            }
        };

        src.clear();
        entry_no += 1;

        return_last_expr(prog, unit_fn);

        // Makes MAIN_UNIT true in this unit
        prog.main_fn = unit_fn;

        if let Err(err) = prog.resolve_repl_unit(&unit_key(&src_name), &mut env) {
            println!("Error: {}", err);
            continue;
        }

        drop(vm_ref);

        actor.grow_globals();
        let ret = actor.call(Value::fun(unit_fn), &[]);

        if !ret.is_nil() {
            crate::host::print(&mut actor, ret).unwrap();
            println!();
        }
    }
}
