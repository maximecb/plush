#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_imports)]

mod utils;
mod ast;
mod parsing;
mod parser;
mod symbols;
mod codegen;
mod vm;
mod alloc;
mod array;
mod host;
mod deepcopy;

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
    // Only parse/validate the input, but don't run it
    parse_only: bool,

    rest: Vec<String>,
}

// TODO: parse permissions
// --allow <permissions>
// --deny <permissions>
// --allow-all
fn parse_args(args: Vec<String>) -> Options
{
    let mut opts = Options {
        parse_only: false,
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
            "--parse-only" => {
                opts.parse_only = true;
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

    let mut prog = parse_file(file_name).unwrap();
    prog.resolve_syms().unwrap();
    let main_fn = prog.main_fn;
    let mut vm = VM::new(prog);
    let ret = VM::call(&mut vm, main_fn, vec![]);
}
