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
use std::thread::sleep;
use std::time::Duration;
use std::process::exit;
use std::sync::{Arc, Mutex};
use crate::vm::{VM, Value};
use crate::utils::{thousands_sep};
use crate::parser::{parse_file};

/// Command-line options
#[derive(Debug, Clone)]
struct Options
{
    // Parse/validate/compile the input, but don't execute it
    no_exec: bool,

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

        // Try to match this argument as an option
        match arg.as_str() {
            "--no-exec" => {
                opts.no_exec = true;
            }

            _ => panic!("unknown option {}", arg)
        }
    }

    opts
}

fn main()
{
    let opts = parse_args(env::args().collect());
    //println!("{:?}", opts);

    if opts.rest.len() != 1 {
        panic!("must specify exactly one input file to run");
    }

    let file_name = &opts.rest[0];

    let mut prog = match parse_file(file_name) {
        Ok(prog) => prog,
        Err(err) => {
            println!("Error while parsing source file:\n{}", err);
            exit(-1);
        }
    };

    prog.resolve_syms().unwrap();
    let main_fn = prog.main_fn;
    let mut vm = VM::new(prog);

    if !opts.no_exec {
        let ret = VM::call(&mut vm, main_fn, vec![]);

        match ret {
            Value::Nil => exit(0),

            Value::Int64(v) => {
                exit(v as i32);
            }

            _ => panic!("main unit should return an integer value")
        }
    }
}
