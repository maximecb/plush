#![allow(unused_parens)]

mod utils;
mod ast;
mod lexer;
mod parser;
mod symbols;
mod codegen;
mod vm;
mod insns;
mod value;
mod alloc;
mod object;
mod closure;
mod array;
mod bytearray;
mod runtime;
mod host;
mod gc;
mod window;
mod audio;
mod exec_tests;
mod str;
mod dict;

extern crate sdl2;
use std::env;
use std::path::PathBuf;
use std::process::exit;
use std::sync::Mutex;
use crate::vm::VM;
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
            "--version" => {
                println!("plush {}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }

            "--no-exec" => {
                opts.no_exec = true;
            }

            "--eval" | "-e" => {
                opts.eval_str = Some(read_arg!(arg));
            }

            "--run-example" => {
                // With no name given, show what is available instead of failing
                if idx >= args.len() {
                    list_examples(&example_dirs());
                    exit(0);
                }

                opts.input_file = Some(find_example(&read_arg!(arg)));
                opts.rest = args[idx..].to_vec();
                break;
            }

            _ => panic!("unknown option {}", arg)
        }
    }

    opts
}

// Places an example may live, in search order
fn example_dirs() -> Vec<PathBuf>
{
    let mut dirs = Vec::new();

    // Set by the installer, and lets people point at their own collection
    if let Ok(dir) = env::var("PLUSH_EXAMPLES_DIR") {
        dirs.push(PathBuf::from(dir));
    }

    // Installed layout, where <root>/bin/plush sits next to <root>/examples
    if let Ok(exe) = env::current_exe() {
        if let Some(root) = exe.parent().and_then(|p| p.parent()) {
            dirs.push(root.join("examples"));
        }
    }

    // Running out of a source checkout
    if let Ok(cwd) = env::current_dir() {
        dirs.push(cwd.join("examples"));
    }

    dirs
}

// Examples refer to their data files as "examples/data/...", so running one
// means entering the directory that contains the examples directory, not the
// examples directory itself
fn find_example(name: &str) -> String
{
    let dirs = example_dirs();

    for dir in &dirs {
        if !dir.join(format!("{}.psh", name)).exists() {
            continue;
        }

        let (root, sub) = match (dir.parent(), dir.file_name()) {
            (Some(root), Some(sub)) => (root, sub.to_string_lossy().into_owned()),
            _ => continue,
        };

        if let Err(err) = env::set_current_dir(root) {
            println!("Error: could not enter {}: {}", root.display(), err);
            exit(-1);
        }

        return format!("{}/{}.psh", sub, name);
    }

    println!("Error: no example named \"{}\"", name);
    list_examples(&dirs);
    exit(-1);
}

fn list_examples(dirs: &[PathBuf])
{
    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| n.strip_suffix(".psh").map(str::to_string))
            .collect();

        if names.is_empty() {
            continue;
        }

        names.sort();
        println!("\n{} examples available:", names.len());
        for name in names {
            println!("  {}", name);
        }
        return;
    }
}

/// Parse an input file or eval string
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
        VM::compile_all(prog);

        return;
    }

    let main_fn = prog.main_fn;
    let mut vm = VM::new(prog);
    let ret = VM::call(&mut vm, main_fn, vec![]);

    // This is the value returned by the main unit
    if ret.is_nil() {
        exit(0);
    }

    match ret.to_i64() {
        Some(v) => exit(v as i32),
        None => panic!("main unit should return an integer value")
    }
}
