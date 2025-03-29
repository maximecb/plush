#![cfg(test)]

use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::collections::HashSet;

fn test_file(file_path: &str, no_exec: bool)
{
    io::stdout().write(format!("running: {}\n", file_path).as_bytes()).unwrap();
    io::stdout().flush().unwrap();

    // Compile the source file
    let mut command = Command::new("cargo");
    command.current_dir(".");
    command.arg("run");
    command.arg("--");
    if no_exec {
        command.arg("--no-exec");
    }
    command.arg(file_path);

    println!("{:?}", command);
    let output = command.output().unwrap();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("\ntest failed \"{}\":\n{}", file_path, stderr);
    }
}

#[test]
fn examples()
{
    for file in fs::read_dir("./examples").unwrap() {
        let file_path = file.unwrap().path().display().to_string();

        if !file_path.ends_with(".rs") {
            continue;
        }

        test_file(&file_path, false);
    }
}

#[test]
fn tests()
{
    for file in fs::read_dir("./tests").unwrap() {
        let file_path = file.unwrap().path().display().to_string();
        test_file(&file_path, false);
    }
}

#[test]
fn benchmarks()
{
    for file in fs::read_dir("./benchmarks").unwrap() {
        let file_path = file.unwrap().path().display().to_string();

        if !file_path.ends_with(".rs") {
            continue;
        }

        // The benchmarks get compiled but not executed
        test_file(&file_path, true);
    }
}
