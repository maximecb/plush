#![cfg(test)]

use rustc_hash::FxHashSet as HashSet;
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::sync::Once;

/// Where the interpreter the tests run scripts through is built
const BIN_PATH: &str = "target/verify_gc/debug/plush";

/// Build an interpreter with heap verification turned on, so that every
/// collection a test triggers checks the heap it produced.
///
/// The build gets its own target directory: cargo holds a lock on the one
/// the test run itself is using. This costs one extra build the first
/// time, and is cached from then on.
fn verify_gc_binary() -> &'static str
{
    static BUILD: Once = Once::new();

    // The test functions run in parallel, so only the first one through
    // builds and the others wait for it
    BUILD.call_once(|| {
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--features", "verify_gc",
                "--target-dir", "target/verify_gc",
            ])
            .status()
            .unwrap();

        assert!(status.success(), "could not build plush with verify_gc");
    });

    BIN_PATH
}

fn test_file(file_path: &str, no_exec: bool)
{
    if no_exec {
        io::stdout().write(format!("parsing: {}\n", file_path).as_bytes()).unwrap();
    } else {
        io::stdout().write(format!("running: {}\n", file_path).as_bytes()).unwrap();
    }
    io::stdout().flush().unwrap();

    // Compile the source file
    let mut command = Command::new(verify_gc_binary());
    command.current_dir(".");
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

        if !file_path.ends_with(".psh") {
            continue;
        }

        // Examples get parsed but not executed
        test_file(&file_path, true);
    }
}

#[test]
fn tests()
{
    for file in fs::read_dir("./tests").unwrap() {
        let file_path = file.unwrap().path().display().to_string();

        if !file_path.ends_with(".psh") {
            continue;
        }

        test_file(&file_path, false);
    }
}

#[test]
fn benchmarks()
{
    for file in fs::read_dir("./benchmarks").unwrap() {
        let file_path = file.unwrap().path().display().to_string();

        if !file_path.ends_with(".psh") {
            continue;
        }

        // The benchmarks get compiled but not executed
        test_file(&file_path, true);
    }
}
