use std::env;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::alloc::Alloc;
use crate::vm::{Value, VM, Actor};
use crate::ast::Expr;
use crate::audio::{audio_open_output, audio_write_samples, audio_open_input, audio_read_samples};

/// Host function signature
/// Note: the in/out arg count should be fixed so
///       that we can JIT host calls efficiently
#[derive(Copy, Clone, Debug)]
pub enum FnPtr
{
    Fn0_0(fn(actor: &mut Actor)),
    Fn0_1(fn(actor: &mut Actor) -> Value),

    Fn1_0(fn(actor: &mut Actor, a0: Value)),
    Fn1_1(fn(actor: &mut Actor, a0: Value) -> Value),

    Fn2_0(fn(actor: &mut Actor, a0: Value, a1: Value)),
    Fn2_1(fn(actor: &mut Actor, a0: Value, a1: Value) -> Value),

    Fn3_0(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value)),
    Fn3_1(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value) -> Value),

    Fn4_0(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value)),
    Fn4_1(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value) -> Value),

    Fn5_0(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value)),
    Fn5_1(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value) -> Value),

    Fn8_0(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value, a5: Value, a6: Value, a7: Value)),
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
            Fn0_0(_) => 0,
            Fn0_1(_) => 0,
            Fn1_0(_) => 1,
            Fn1_1(_) => 1,
            Fn2_0(_) => 2,
            Fn2_1(_) => 2,
            Fn3_0(_) => 3,
            Fn3_1(_) => 3,
            Fn4_0(_) => 4,
            Fn4_1(_) => 4,
            Fn5_0(_) => 5,
            Fn5_1(_) => 5,
            Fn8_0(_) => 8,
        }
    }

    pub fn has_ret(&self) -> bool
    {
        use FnPtr::*;
        match self.f {
            Fn0_0(_) => false,
            Fn0_1(_) => true,
            Fn1_0(_) => false,
            Fn1_1(_) => true,
            Fn2_0(_) => false,
            Fn2_1(_) => true,
            Fn3_0(_) => false,
            Fn3_1(_) => true,
            Fn4_0(_) => false,
            Fn4_1(_) => true,
            Fn5_0(_) => false,
            Fn5_1(_) => true,
            Fn8_0(_) => false,
        }
    }
}

/// Get a host constant by name
/// Returns an AST expression node for the constant,
/// because we want host constants to be resolved early
pub fn get_host_const(name: &str) -> Expr
{
    use FnPtr::*;
    use crate::window::*;

    static TIME_CURRENT_MS: HostFn = HostFn { name: "time_current_ms", f: Fn0_1(time_current_ms) };
    static CMD_NUM_ARGS: HostFn = HostFn { name: "cmd_num_args", f: Fn0_1(cmd_num_args) };
    static CMD_GET_ARG: HostFn = HostFn { name: "cmd_get_arg", f: Fn1_1(cmd_get_arg) };
    static PRINT: HostFn = HostFn { name: "print", f: Fn1_0(print) };
    static PRINTLN: HostFn = HostFn { name: "println", f: Fn1_0(println) };
    static READLN: HostFn = HostFn { name: "readln", f: Fn0_1(readln) };
    static READ_FILE: HostFn = HostFn { name: "read_file", f: Fn1_1(read_file) };
    static READ_FILE_UTF8: HostFn = HostFn { name: "read_file", f: Fn1_1(read_file_utf8) };
    static WRITE_FILE: HostFn = HostFn { name: "write_file", f: Fn2_1(write_file) };
    static ACTOR_ID: HostFn = HostFn { name: "actor_id", f: Fn0_1(actor_id) };
    static ACTOR_PARENT: HostFn = HostFn { name: "actor_parent", f: Fn0_1(actor_parent) };
    static ACTOR_SLEEP: HostFn = HostFn { name: "actor_sleep", f: Fn1_0(actor_sleep) };
    static ACTOR_SPAWN: HostFn = HostFn { name: "actor_spawn", f: Fn1_1(actor_spawn) };
    static ACTOR_JOIN: HostFn = HostFn { name: "actor_join", f: Fn1_1(actor_join) };
    static ACTOR_SEND: HostFn = HostFn { name: "actor_send", f: Fn2_1(actor_send) };
    static ACTOR_RECV: HostFn = HostFn { name: "actor_recv", f: Fn0_1(actor_recv) };
    static ACTOR_POLL: HostFn = HostFn { name: "actor_poll", f: Fn0_1(actor_poll) };
    static WINDOW_CREATE: HostFn = HostFn { name: "window_create", f: Fn4_1(window_create) };
    static WINDOW_DRAW_FRAME: HostFn = HostFn { name: "window_draw_frame", f: Fn2_0(window_draw_frame) };
    static AUDIO_OPEN_OUTPUT: HostFn = HostFn { name: "audio_open_output", f: Fn2_1(audio_open_output) };
    static AUDIO_WRITE_SAMPLES: HostFn = HostFn { name: "audio_write_samples", f: Fn2_0(audio_write_samples) };
    static AUDIO_OPEN_INPUT: HostFn = HostFn { name: "audio_open_input", f: Fn2_1(audio_open_input) };
    static AUDIO_READ_SAMPLES: HostFn = HostFn { name: "audio_read_samples", f: Fn4_0(audio_read_samples) };
    static EXIT: HostFn = HostFn { name: "exit", f: Fn1_0(exit) };

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
pub fn time_current_ms(actor: &mut Actor) -> Value
{
    Value::from(get_time_ms())
}

/// Get the number of command-line arguments
pub fn cmd_num_args(actor: &mut Actor) -> Value
{
    let num_args = crate::REST_ARGS.lock().unwrap().len();
    Value::from(num_args)
}

/// Get a command-line argument string by index
/// Note: if we allocate just one object then we can be
/// guaranteed that object won't be GC'd while this function runs
pub fn cmd_get_arg(actor: &mut Actor, idx: Value) -> Value
{
    let idx = idx.unwrap_usize();

    let args = crate::REST_ARGS.lock().unwrap();

    if idx >= args.len() {
        return Value::Nil;
    }

    let str_obj = actor.alloc.str_const(args[idx].clone());
    Value::String(str_obj)
}

/// Print a value to stdout
fn print(actor: &mut Actor, v: Value)
{
    match v {
        Value::String(_) => {
            let rust_str = v.unwrap_rust_str();
            print!("{}", rust_str);
        }

        Value::Int64(v) => print!("{}", v),
        Value::Float64(v) => print!("{}", v),

        Value::True => print!("true"),
        Value::False => print!("false"),
        Value::Nil => print!("nil"),

        _ => todo!()
    }
}

/// Print a value to stdout, followed by a newline
fn println(actor: &mut Actor, v: Value)
{
    print(actor, v);
    println!();
}

/// Read one line of input from stdin
fn readln(actor: &mut Actor) -> Value
{
    let mut line = String::new();

    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            let str_obj = actor.alloc.str_const(line);
            Value::String(str_obj)
        }

        Err(_) => Value::Nil
    }
}

/// Do some basic safety checking (sandboxing) to minimize
/// security risks for file accesses
fn is_safe_path(file_path: &str) -> bool
{
    use std::path::Path;
    use std::path::PathBuf;
    use std::fs::canonicalize;

    // Extra paranoid check
    if file_path.contains("..") {
        println!("file path contains parent operator");
        return false;
    }

    let file_path = file_path.trim();
    let mut file_path = PathBuf::from(file_path);

    // Reject absolute paths, accept relative paths only
    if !file_path.is_relative() {
        println!("file path is not relative");
        return false;
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

    // Get the current working directory
    let cwd = std::env::current_dir().unwrap();
    let cwd = canonicalize(&cwd).unwrap();
    //println!("Canonical cwd: {:?}", cwd);

    // For now, only allow access to paths inside the CWD
    if !file_path.starts_with(cwd) {
        println!("path not in CWD");
        return false;
    }

    // Don't allow access to the current executable
    let current_exe = std::env::current_exe().unwrap();
    let current_exe = canonicalize(&current_exe).unwrap();
    if file_path == current_exe {
        println!("file path is current exe");
        return false;
    }

    // Reject extensions associated with executable files
    let ext = match file_path.extension() {
        Some(ext) => ext.to_str().unwrap(),
        None => ""
    };
    if ext == "exe" || ext == "bat" || ext == "cmd" || ext == "com" || ext == "sh" {
        return false;
    }

    // On Unix/Linux platforms, deny access to files marked as executable
    #[cfg(unix)]
    if !file_path.is_dir() {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(file_path).unwrap();
        let permissions = metadata.permissions();
        let mode = permissions.mode();
        if (mode & 0o111) != 0 {
            println!("mode is executable");
            return false;
        }
    }

    true
}

/// Read the contents of an entire file into a ByteArray object
fn read_file(actor: &mut Actor, file_path: Value) -> Value
{
    let file_path = file_path.unwrap_rust_str();

    if !is_safe_path(&file_path) {
        panic!("requested file path breaks sandboxing rules");
    }

    let bytes: Vec<u8> = match std::fs::read(file_path) {
        Err(_) => return Value::Nil,
        Ok(bytes) => bytes
    };

    let ba = crate::bytearray::ByteArray::new(bytes);
    let ba_obj = actor.alloc.alloc(ba);
    Value::ByteArray(ba_obj)
}

/// Read the contents of an entire file encoded as valid UTF-8
fn read_file_utf8(actor: &mut Actor, file_path: Value) -> Value
{
    let file_path = file_path.unwrap_rust_str();

    if !is_safe_path(&file_path) {
        panic!("requested file path breaks sandboxing rules");
    }

    let s: String = match std::fs::read_to_string(file_path) {
        Err(_) => return Value::Nil,
        Ok(s) => s
    };

    let s_obj = actor.alloc.str_const(s);
    Value::String(s_obj)
}

/// Writes the contents of a ByteArray to a file
fn write_file(actor: &mut Actor, file_path: Value, mut bytes: Value) -> Value
{
    let file_path = file_path.unwrap_rust_str();
    let bytes = bytes.unwrap_ba();
    let bytes = unsafe { bytes.get_slice(0, bytes.num_bytes()) };

    if !is_safe_path(&file_path) {
        panic!("requested file path breaks sandboxing rules");
    }

    match std::fs::write(file_path, &bytes) {
        Err(_) => Value::False,
        Ok(_) => Value::True
    }
}

/// Get the id of the current actor
fn actor_id(actor: &mut Actor) -> Value
{
    Value::from(actor.actor_id)
}

/// Get the id of the parent actor
fn actor_parent(actor: &mut Actor) -> Value
{
    match actor.parent_id {
        Some(actor_id) => Value::from(actor_id),
        None => Value::Nil,
    }
}

/// Make the current actor sleep
fn actor_sleep(actor: &mut Actor, msecs: Value)
{
    let msecs = msecs.unwrap_u64();
    thread::sleep(Duration::from_millis(msecs));
}

/// Spawn a new actor
/// Takes a function to call as argument
/// Returns an actor id
fn actor_spawn(actor: &mut Actor, fun: Value) -> Value
{
    let actor_id = VM::new_actor(actor, fun, vec![]);
    Value::from(actor_id)
}

/// Wait for a thread to terminate, produce the return value
fn actor_join(actor: &mut Actor, actor_id: Value) -> Value
{
    let id = actor_id.unwrap_u64();
    VM::join_actor(&actor.vm, id)
}

/// Send a message to an actor
/// This will return false in case of failure
fn actor_send(actor: &mut Actor, actor_id: Value, msg: Value) -> Value
{
    let actor_id = actor_id.unwrap_u64();

    let res = actor.send(actor_id, msg);

    if res.is_ok() {
        Value::True
    } else {
        Value::False
    }
}

/// Receive a message from the current actor's queue
/// This will block until a message is available
fn actor_recv(actor: &mut Actor) -> Value
{
    actor.recv()
}

/// Receive a message from the current actor's queue
/// This will block until a message is available
fn actor_poll(actor: &mut Actor) -> Value
{
    match actor.try_recv() {
        Some(msg_val) => msg_val,
        None => Value::Nil,
    }
}

/// End program execution
fn exit(thread: &mut Actor, val: Value)
{
    let val = (val.unwrap_i64() & 0xFF) as i32;
    std::process::exit(val);
}
