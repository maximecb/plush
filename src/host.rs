use std::env;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::alloc::Alloc;
use crate::vm::{Value, VM, Actor};
use crate::ast::{Expr, Function, Program};
use crate::{error, unwrap_usize, unwrap_str};

/// Host function signature
/// Note: the in/out arg count should be fixed so
///       that we can JIT host calls efficiently
#[derive(Copy, Clone, Debug)]
pub enum FnPtr
{
    Fn0(fn(actor: &mut Actor) -> Result<Value, String>),
    Fn1(fn(actor: &mut Actor, a0: Value) -> Result<Value, String>),
    Fn2(fn(actor: &mut Actor, a0: Value, a1: Value) -> Result<Value, String>),
    Fn3(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value) -> Result<Value, String>),
    Fn4(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value) -> Result<Value, String>),
    Fn5(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value) -> Result<Value, String>),
    Fn8(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value, a5: Value, a6: Value, a7: Value) -> Result<Value, String>),
}

// This struct is needed in part because Rust doesn't allow direct
// function pointer equality comparison. It also allows us to store
// the name of the function for easier debugging
#[derive(Debug)]
pub struct HostFn
{
    pub name: &'static str,
    pub f: FnPtr,
}

impl HostFn
{
    pub fn num_params(&self) -> usize
    {
        use FnPtr::*;
        match self.f {
            Fn0(_) => 0,
            Fn1(_) => 1,
            Fn2(_) => 2,
            Fn3(_) => 3,
            Fn4(_) => 4,
            Fn5(_) => 5,
            Fn8(_) => 8,
        }
    }
}

/// Get a host constant by name
/// Returns an AST expression node for the constant,
/// because we want host constants to be resolved early
pub fn get_host_const(name: &str, fun: &Function, prog: &Program) -> Expr
{
    use FnPtr::*;
    use crate::window::*;
    use crate::audio::*;

    // This constant is only true inside the main unit
    if name == "MAIN_UNIT" {
        if fun.id == prog.main_fn {
            return Expr::True;
        } else {
            return Expr::False;
        }
    }

    static TIME_CURRENT_MS: HostFn = HostFn { name: "time_current_ms", f: Fn0(time_current_ms) };
    static CMD_NUM_ARGS: HostFn = HostFn { name: "cmd_num_args", f: Fn0(cmd_num_args) };
    static CMD_GET_ARG: HostFn = HostFn { name: "cmd_get_arg", f: Fn1(cmd_get_arg) };
    static PRINT: HostFn = HostFn { name: "print", f: Fn1(print) };
    static PRINTLN: HostFn = HostFn { name: "println", f: Fn1(println) };
    static READLN: HostFn = HostFn { name: "readln", f: Fn0(readln) };
    static READ_FILE: HostFn = HostFn { name: "read_file", f: Fn1(read_file) };
    static READ_FILE_UTF8: HostFn = HostFn { name: "read_file", f: Fn1(read_file_utf8) };
    static WRITE_FILE: HostFn = HostFn { name: "write_file", f: Fn2(write_file) };
    static ACTOR_ID: HostFn = HostFn { name: "actor_id", f: Fn0(actor_id) };
    static ACTOR_PARENT: HostFn = HostFn { name: "actor_parent", f: Fn0(actor_parent) };
    static ACTOR_SLEEP: HostFn = HostFn { name: "actor_sleep", f: Fn1(actor_sleep) };
    static ACTOR_SPAWN: HostFn = HostFn { name: "actor_spawn", f: Fn1(actor_spawn) };
    static ACTOR_JOIN: HostFn = HostFn { name: "actor_join", f: Fn1(actor_join) };
    static ACTOR_SEND: HostFn = HostFn { name: "actor_send", f: Fn2(actor_send) };
    static ACTOR_RECV: HostFn = HostFn { name: "actor_recv", f: Fn0(actor_recv) };
    static ACTOR_POLL: HostFn = HostFn { name: "actor_poll", f: Fn0(actor_poll) };
    static WINDOW_CREATE: HostFn = HostFn { name: "window_create", f: Fn4(window_create) };
    static WINDOW_DRAW_FRAME: HostFn = HostFn { name: "window_draw_frame", f: Fn2(window_draw_frame) };
    static AUDIO_OPEN_OUTPUT: HostFn = HostFn { name: "audio_open_output", f: Fn2(audio_open_output) };
    static AUDIO_WRITE_SAMPLES: HostFn = HostFn { name: "audio_write_samples", f: Fn2(audio_write_samples) };
    static AUDIO_OPEN_INPUT: HostFn = HostFn { name: "audio_open_input", f: Fn2(audio_open_input) };
    static AUDIO_READ_SAMPLES: HostFn = HostFn { name: "audio_read_samples", f: Fn4(audio_read_samples) };
    static EXIT: HostFn = HostFn { name: "exit", f: Fn1(exit) };

    let fn_ref = match name
    {
        "time_current_ms" => &TIME_CURRENT_MS,

        "cmd_num_args" => &CMD_NUM_ARGS,
        "cmd_get_arg" => &CMD_GET_ARG,

        "print" => &PRINT,
        "println" => &PRINTLN,
        "readln" => &READLN,
        "read_file" => &READ_FILE,
        "read_file_utf8" => &READ_FILE_UTF8,
        "write_file" => &WRITE_FILE,

        "actor_id" => &ACTOR_ID,
        "actor_parent" => &ACTOR_PARENT,
        "actor_sleep" => &ACTOR_SLEEP,
        "actor_spawn" => &ACTOR_SPAWN,
        "actor_join" => &ACTOR_JOIN,
        "actor_send" => &ACTOR_SEND,
        "actor_recv" => &ACTOR_RECV,
        "actor_poll" => &ACTOR_POLL,

        "window_create" => &WINDOW_CREATE,
        "window_draw_frame" => &WINDOW_DRAW_FRAME,

        "audio_open_output" => &AUDIO_OPEN_OUTPUT,
        "audio_write_samples" => &AUDIO_WRITE_SAMPLES,

        "audio_open_input" => &AUDIO_OPEN_INPUT,
        "audio_read_samples" => &AUDIO_READ_SAMPLES,

        "exit" => &EXIT,

        _ => panic!("unknown host constant `{name}`")
    };

    Expr::HostFn(fn_ref)
}

/// Get the current time stamp in milliseconds
pub fn get_time_ms() -> u64
{
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Get the current time stamp in milliseconds since the unix epoch
pub fn time_current_ms(actor: &mut Actor) -> Result<Value, String>
{
    Ok(Value::from(get_time_ms()))
}

/// Get the number of command-line arguments
pub fn cmd_num_args(actor: &mut Actor) -> Result<Value, String>
{
    let num_args = crate::REST_ARGS.lock().unwrap().len();
    Ok(Value::from(num_args))
}

/// Get a command-line argument string by index
/// Note: if we allocate just one object then we can be
/// guaranteed that object won't be GC'd while this function runs
pub fn cmd_get_arg(actor: &mut Actor, idx: Value) -> Result<Value, String>
{
    let idx = idx.unwrap_usize();

    let args = crate::REST_ARGS.lock().unwrap();

    if idx >= args.len() {
        return Ok(Value::Nil);
    }

    Ok(actor.alloc.str_val(&args[idx]))
}

/// Print a value to stdout
fn print(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    match v {
        Value::String(_) => {
            let rust_str = unwrap_str!(v);
            print!("{}", rust_str);
        }

        Value::Int64(v) => print!("{}", v),
        Value::Float64(v) => print!("{}", v),

        Value::True => print!("true"),
        Value::False => print!("false"),
        Value::Nil => print!("nil"),

        _ => print!("{:?}", v)
    }

    Ok(Value::Nil)
}

/// Print a value to stdout, followed by a newline
fn println(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    print(actor, v)?;
    println!();
    Ok(Value::Nil)
}

/// Read one line of input from stdin
fn readln(actor: &mut Actor) -> Result<Value, String>
{
    let mut line = String::new();

    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            Ok(actor.alloc.str_val(&line))
        }

        Err(_) => Ok(Value::Nil)
    }
}

/// Do some basic safety checking (sandboxing) to minimize
/// security risks for file accesses
fn is_safe_path(file_path: &str) -> bool
{
    use std::path::Path;
    use std::path::PathBuf;
    use std::fs::canonicalize;

    let file_path = file_path.trim();
    let mut file_path = PathBuf::from(file_path);

    // Reject extensions associated with executable files
    if let Some(ext) = file_path.extension() {
        if ext == "exe" || ext == "bat" || ext == "cmd" || ext == "com" || ext == "sh" {
            return false;
        }
    }

    // If this is a file that does not exist yet,
    // Pop the file name from the path
    if !file_path.exists() {
        file_path.pop();
        if file_path.as_os_str().is_empty() {
            file_path = PathBuf::from(".");
        }
    }

    // Get the absolute path for the file, resolving symlinks
    let file_path = canonicalize(&file_path).unwrap();
    //println!("Canonical path: {:?}", file_path);

    // Don't allow access to the current executable
    let current_exe = std::env::current_exe().unwrap();
    let current_exe = canonicalize(&current_exe).unwrap();
    if file_path == current_exe {
        println!("file path is current exe");
        return false;
    }

    // On Unix/Linux platforms, deny access to files marked as executable
    #[cfg(unix)]
    if !file_path.is_dir() {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();
        let mode = permissions.mode();
        if (mode & 0o111) != 0 {
            println!("mode is executable");
            return false;
        }
    }

    // Get the current working directory
    let cwd = std::env::current_dir().unwrap();
    let cwd = canonicalize(&cwd).unwrap();
    //println!("Canonical cwd: {:?}", cwd);

    // If the file path is inside the current working directory, allow access
    if file_path.starts_with(cwd) {
        return true;
    }

    // Parse the rest arguments
    let rest_args = crate::parse_args(std::env::args().collect()).rest;

    // For each rest argument supplied on the command-line
    for arg in rest_args {

        let arg_path = PathBuf::from(arg);

        // If this is not a valid path, ignore it
        if !arg_path.exists() {
            continue;
        }

        let arg_path = canonicalize(&arg_path).unwrap();

        // We can allow access to files in directories
        // explicitly specified on the command-line
        if arg_path.is_dir() {
            if file_path.starts_with(&arg_path) {
                return true;
            }
        }

        // We can allow access to files explicitly
        // specified on the command-line
        if arg_path.is_file() && file_path == arg_path {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests
{
    use crate::host::is_safe_path;

    #[test]
    fn safe_path()
    {
        assert!(!is_safe_path("/"));
        assert!(!is_safe_path("/root"));
        assert!(!is_safe_path("/usr/bin"));
        assert!(!is_safe_path("/home/user"));
        assert!(!is_safe_path(".."));
        assert!(!is_safe_path("run_me.sh"));
        assert!(!is_safe_path("run_me.exe"));

        if let Some(home_path) = std::env::home_dir() {
            let home_path = home_path.to_str().unwrap();
            assert!(!is_safe_path(home_path));
        }

        assert!(is_safe_path("foo.txt"));
        assert!(is_safe_path("docs/language.md"));
    }
}

/// Read the contents of an entire file into a ByteArray object
fn read_file(actor: &mut Actor, file_path: Value) -> Result<Value, String>
{
    let file_path = unwrap_str!(file_path);

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    let bytes: Vec<u8> = match std::fs::read(file_path) {
        Err(_) => return Ok(Value::Nil),
        Ok(bytes) => bytes
    };

    let ba = crate::bytearray::ByteArray::new(bytes);
    let ba_obj = actor.alloc.alloc(ba);
    Ok(Value::ByteArray(ba_obj))
}

/// Read the contents of an entire file encoded as valid UTF-8
fn read_file_utf8(actor: &mut Actor, file_path: Value) -> Result<Value, String>
{
    let file_path = unwrap_str!(file_path);

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    let s: String = match std::fs::read_to_string(file_path) {
        Err(_) => return Ok(Value::Nil),
        Ok(s) => s
    };

    Ok(actor.alloc.str_val(&s))
}

/// Writes the contents of a ByteArray to a file
fn write_file(actor: &mut Actor, file_path: Value, mut bytes: Value) -> Result<Value, String>
{
    let file_path = unwrap_str!(file_path);
    let bytes = bytes.unwrap_ba();
    let bytes = unsafe { bytes.get_slice(0, bytes.num_bytes()) };

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    match std::fs::write(file_path, &bytes) {
        Err(_) => Ok(Value::False),
        Ok(_) => Ok(Value::True)
    }
}

/// Get the id of the current actor
fn actor_id(actor: &mut Actor) -> Result<Value, String>
{
    Ok(Value::from(actor.actor_id))
}

/// Get the id of the parent actor
fn actor_parent(actor: &mut Actor) -> Result<Value, String>
{
    Ok(match actor.parent_id {
        Some(actor_id) => Value::from(actor_id),
        None => Value::Nil,
    })
}

/// Make the current actor sleep
fn actor_sleep(actor: &mut Actor, msecs: Value) -> Result<Value, String>
{
    let msecs = msecs.unwrap_u64();
    thread::sleep(Duration::from_millis(msecs));
    Ok(Value::Nil)
}

/// Spawn a new actor
/// Takes a function to call as argument
/// Returns an actor id
fn actor_spawn(actor: &mut Actor, fun: Value) -> Result<Value, String>
{
    let fun_id = match fun {
        Value::Closure(clos) => unsafe { (*clos).fun_id },
        Value::Fun(fun_id) => fun_id,
        _ => return Err("actor_spawn received non-function value".into())
    };

    // TODO: check the function argument count and report a helpful
    // error message here

    let actor_id = VM::new_actor(actor, fun, vec![]);
    Ok(Value::from(actor_id))
}

/// Wait for a thread to terminate, produce the return value
fn actor_join(actor: &mut Actor, actor_id: Value) -> Result<Value, String>
{
    let id = actor_id.unwrap_u64();
    Ok(VM::join_actor(&actor.vm, id))
}

/// Send a message to an actor
/// This will return false in case of failure
fn actor_send(actor: &mut Actor, actor_id: Value, msg: Value) -> Result<Value, String>
{
    let actor_id = actor_id.unwrap_u64();

    let res = actor.send(actor_id, msg);

    if res.is_ok() {
        Ok(Value::True)
    } else {
        Ok(Value::False)
    }
}

/// Receive a message from the current actor's queue
/// This will block until a message is available
fn actor_recv(actor: &mut Actor) -> Result<Value, String>
{
    Ok(actor.recv())
}

/// Receive a message from the current actor's queue
/// This will block until a message is available
fn actor_poll(actor: &mut Actor) -> Result<Value, String>
{
    Ok(match actor.try_recv() {
        Some(msg_val) => msg_val,
        None => Value::Nil,
    })
}

/// End program execution
fn exit(thread: &mut Actor, val: Value) -> Result<Value, String>
{
    let val = (val.unwrap_i64() & 0xFF) as i32;
    std::process::exit(val);
}