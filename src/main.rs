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
mod audio;
mod exec_tests;
mod str;

extern crate sdl2;
use std::env;
use std::process::exit;
use std::sync::{Arc, Mutex};
use crate::vm::{VM, Value};
use crate::utils::{thousands_sep};
use crate::ast::Program;
use crate::parser::{parse_file, parse_str};

/// Command-line arguments accessible to the program
pub static REST_ARGS: Mutex<Vec<String>> = Mutex::new(vec![]);

/// Command-line options
#[derive(Default, Debug, Clone)]
pub struct Options
{
    // Parse/validate/compile the input, but don't execute it
    no_exec: bool,

    // String of code to be evaluated
    eval_str: Option<String>,

    // Input script file to parse/execute
    input_file: Option<String>,

    // Unnamed rest arguments
    rest: Vec<String>,
}

// Parse the command-line arguments
// TODO: parse permissions
// --allow <permissions>
// --deny <permissions>
// --allow-all
pub fn parse_args(args: Vec<String>) -> Options
{
    let mut opts = Options::default();

    // Start parsing at argument 1 because 0 is the current program name
    let mut idx = 1;

    while idx < args.len()
    {
        let arg = &args[idx];
        //println!("{}", arg);

        // If this is the start of the rest arguments
        if !arg.starts_with("-") {
            opts.input_file = Some(args[idx].clone());
            opts.rest = args[idx+1..].to_vec();
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

    let file_name = match &opts.input_file {
        None => {
            println!("Error: must specify exactly one input file to run");
            exit(-1);
        }
        Some(file_name) => file_name,
    };

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

    // Store the rest arguments in a global variable
    // This is so we can access them from host functions
    let mut args = opts.rest;
    if opts.input_file.is_some() {
        args.insert(0, opts.input_file.unwrap());
    }
    *REST_ARGS.lock().unwrap() = args;

    match prog.resolve_syms() {
        Err(err) => {
            println!("Error while resolving symbols:\n{}", err);
            exit(-1);
        }
        Ok(_) => {}
    }

    // If we're only validating the program without executing it
    if opts.no_exec {
        // Generate code for all the functions to test
        // that this works correctly
        let mut code = vec![];
        let mut alloc = crate::alloc::Alloc::new();
        for (fun_id, fun) in prog.funs {
            fun.gen_code(&mut code, &mut alloc).unwrap();
        }

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
