#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_imports)]
#![allow(unused_parens)]

mod utils;
mod ast;
mod lexer;
mod parser;
mod symbols;
mod codegen;
mod vm;
mod alloc;
mod array;
mod bytearray;
mod runtime;
mod host;
mod deepcopy;
mod window;
mod exec_tests;

extern crate sdl2;
use std::env;
use std::process::exit;
use crate::vm::{VM, Value};
use crate::utils::{thousands_sep};
use crate::ast::Program;
use crate::parser::{parse_file, parse_str};

/// Command-line options
#[derive(Debug, Clone)]
struct Options
{
    // Parse/validate/compile the input, but don't execute it
    no_exec: bool,

    // String of code to be evaluated
    eval_str: Option<String>,

    // Unnamed rest arguments
    rest: Vec<String>,
}

// TODO: parse permissions
// --allow <permissions>
// --deny <permissions>
// --allow-all
fn parse_args(args: Vec<String>) -> Options
{
    let mut opts = Options {
        no_exec: false,
        eval_str: None,
        rest: Vec::default(),
    };

    // Start parsing at argument 1 because 0 is the current program name
    let mut idx = 1;

    while idx < args.len()
    {
        let arg = &args[idx];
        //println!("{}", arg);

        // If this is the start of the rest arguments
        if !arg.starts_with("-") {
            opts.rest = args[idx..].to_vec();
            break;
        }

        // Move to the next argument
        idx += 1;

        macro_rules! read_arg {
            ($name: expr) => {{
                if idx >= args.len() {
                    println!("Missing argument for {} command-line option", $name);
                    exit(-1);
                }

                let arg = args[idx].clone();
                idx += 1;
                arg
            }}
        }

        // Try to match this argument as an option
        match arg.as_str() {
            "--no-exec" => {
                opts.no_exec = true;
            }

            "--eval" | "-e" => {
                opts.eval_str = Some(read_arg!(arg));
            }

            _ => panic!("unknown option {}", arg)
        }
    }

    opts
}

fn parse_input(opts: &Options) -> Program
{
    if let Some(eval_str) = &opts.eval_str {
        match parse_str(&eval_str) {
            Err(err) => {
                println!("Error while parsing eval string:\n{}", err);
                exit(-1);
            }
            Ok(prog) => return prog,
        };
    }

    if opts.rest.len() != 1 {
        println!("Error: must specify exactly one input file to run");
        exit(-1);
    }

    let file_name = &opts.rest[0];

    match parse_file(file_name) {
        Err(err) => {
            println!("Error while parsing source file:\n{}", err);
            exit(-1);
        }
        Ok(prog) => return prog,
    };
}

fn main()
{
    let opts = parse_args(env::args().collect());
    //println!("{:?}", opts);

    let mut prog = parse_input(&opts);

    match prog.resolve_syms() {
        Err(err) => {
            println!("Error while resolving symbols:\n{}", err);
            exit(-1);
        }
        Ok(_) => {}
    }

    if opts.no_exec {
        return;
    }

    let main_fn = prog.main_fn;
    let mut vm = VM::new(prog);
    let ret = VM::call(&mut vm, main_fn, vec![]);

    // This is the value returned by the main unit
    match ret {
        Value::Nil => exit(0),

        Value::Int64(v) => {
            exit(v as i32);
        }

        _ => panic!("main unit should return an integer value")
    }
}
