use std::thread;
use std::time::Duration;
use crate::vm::{VM, Actor};
use crate::value::*;
use crate::ast::{Expr, Function, Program};
use crate::str::Str;
use crate::*;

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
    static CMD_GET_ARG_OR: HostFn = HostFn { name: "cmd_get_arg_or", f: Fn2(cmd_get_arg_or) };
    static PRINT: HostFn = HostFn { name: "print", f: Fn1(print) };
    static PRINTLN: HostFn = HostFn { name: "println", f: Fn1(println) };
    static READLN: HostFn = HostFn { name: "readln", f: Fn0(readln) };
    static READ_FILE: HostFn = HostFn { name: "read_file", f: Fn1(read_file) };
    static READ_FILE_UTF8: HostFn = HostFn { name: "read_file_utf8", f: Fn1(read_file_utf8) };
    static WRITE_FILE: HostFn = HostFn { name: "write_file", f: Fn2(write_file) };
    static MAKE_DIR: HostFn = HostFn { name: "make_dir", f: Fn1(make_dir) };
    static VM_SHRINK_HEAP: HostFn = HostFn { name: "vm_shrink_heap", f: Fn1(vm_shrink_heap) };
    static VM_GC_COLLECT: HostFn = HostFn { name: "vm_gc_collect", f: Fn0(vm_gc_collect) };
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
        "cmd_get_arg_or" => &CMD_GET_ARG_OR,

        "print" => &PRINT,
        "println" => &PRINTLN,
        "readln" => &READLN,
        "read_file" => &READ_FILE,
        "read_file_utf8" => &READ_FILE_UTF8,
        "write_file" => &WRITE_FILE,
        "make_dir" => &MAKE_DIR,

        "vm_shrink_heap" => &VM_SHRINK_HEAP,
        "vm_gc_collect" => &VM_GC_COLLECT,
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
    Ok(actor.int64(get_time_ms() as i64))
}

/// Get the number of command-line arguments
pub fn cmd_num_args(_actor: &mut Actor) -> Result<Value, String>
{
    let num_args = crate::REST_ARGS.lock().unwrap().len();
    Ok(Value::fixnum(num_args as i64))
}

/// Get a command-line argument string by index
pub fn cmd_get_arg_or(actor: &mut Actor, idx: Value, default: Value) -> Result<Value, String>
{
    let idx = unwrap_usize!(idx);

    let args = crate::REST_ARGS.lock().unwrap();

    if idx >= args.len() {
        return Ok(default);
    }

    let arg_str = &args[idx];

    actor.gc_check(
        Str::alloc_size(arg_str.len()),
        &mut [],
    );

    Ok(Str::new(arg_str, &mut actor.alloc))
}

/// Get a command-line argument string by index
pub fn cmd_get_arg(actor: &mut Actor, idx: Value) -> Result<Value, String>
{
    cmd_get_arg_or(actor, idx, Value::NIL)
}

/// Print a value to stdout
fn print(_actor: &mut Actor, v: Value) -> Result<Value, String>
{
    match v.type_of() {
        Type::String => print!("{}", v.as_str()),
        Type::Int64 => print!("{}", v.to_i64().unwrap()),
        Type::Float64 => print!("{}", v.to_f64().unwrap()),
        Type::Bool => print!("{}", v.as_bool()),
        Type::Nil => print!("nil"),
        _ => print!("{:?}", v)
    }

    // Rust line-buffers stdout, so without this a program that prints
    // incrementally without newlines shows nothing until its buffer fills
    use std::io::Write;
    let _ = std::io::stdout().flush();

    Ok(Value::NIL)
}

/// Print a value to stdout, followed by a newline
fn println(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    print(actor, v)?;
    println!();
    Ok(Value::NIL)
}

/// Read one line of input from stdin
fn readln(actor: &mut Actor) -> Result<Value, String>
{
    let mut line = String::new();

    match std::io::stdin().read_line(&mut line) {
        Ok(_) => {
            actor.gc_check(
                Str::alloc_size(line.len()),
                &mut [],
            );

            Ok(Str::new(&line, &mut actor.alloc))
        }

        Err(_) => Ok(Value::NIL)
    }
}

/// Do some basic safety checking (sandboxing) to minimize
/// security risks for file accesses
fn is_safe_path(file_path: &str) -> bool
{
    use std::path::{PathBuf, Component};
    use std::fs::canonicalize;

    let file_path = file_path.trim();
    let mut file_path = PathBuf::from(file_path);

    // Guard specifically against touching the source code and tooling
    // directories of the UVM project itself. We reject the path if any of
    // its components names one of these directories (case-insensitive). This
    // is checked on the requested path, before canonicalization, so it also
    // prevents creating new files under such a directory.
    for comp in file_path.components() {
        if let Component::Normal(name) = comp {
            match name.to_string_lossy().to_lowercase().as_str() {
                "src" | ".cargo" | "cargo.toml" | "cargo.lock" |
                ".git" | ".github" => return false,
                _ => {}
            }
        }
    }

    // Reject extensions associated with executable, script or
    // loadable library files. The comparison is case-insensitive
    // because some filesystems (e.g. macOS, Windows) treat "EXE"
    // and "exe" as referring to the same file, so a case-sensitive
    // check would be trivially bypassable.
    if let Some(ext) = file_path.extension() {
        match ext.to_string_lossy().to_lowercase().as_str() {
            // Windows executables and scripts
            "exe" | "com" | "scr" | "msi" | "cpl" | "dll" |
            "bat" | "cmd" | "ps1" | "psm1" | "vbs" | "vbe" |
            "js" | "jse" | "wsf" | "wsh" | "hta" | "jar" |
            // Unix/macOS executables, libraries and shell scripts
            "sh" | "bash" | "zsh" | "csh" | "ksh" | "fish" |
            "command" | "so" | "dylib" | "app" | "out" |
            // Interpreted language sources
            "py" | "pyc" | "pyo" | "rb" | "pl" | "php" | "lua"
                => return false,
            _ => {}
        }
    }

    // If this is a file that does not exist yet, pop the trailing
    // components from the path. This is necessary for the canonicalize
    // function to work
    while !file_path.exists() {
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
    if file_path.exists() && !file_path.is_dir() {
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

        // Other executable/script/library extensions are unsafe
        assert!(!is_safe_path("lib.dylib"));
        assert!(!is_safe_path("script.py"));
        assert!(!is_safe_path("app.jar"));

        // The blocklist must not be bypassable by changing the case
        assert!(!is_safe_path("run_me.SH"));
        assert!(!is_safe_path("MALWARE.Exe"));

        // Reject access to the UVM project's own source and tooling dirs
        assert!(!is_safe_path("src/main.rs"));
        assert!(!is_safe_path("vm/src/host.rs"));
        assert!(!is_safe_path(".cargo/config.toml"));
        assert!(!is_safe_path(".git/config"));
        assert!(!is_safe_path(".github/workflows/test.yml"));
        assert!(!is_safe_path("../.git/config"));

        // The manifest files are guarded as whole-name components
        // (the extension is part of the component, not separate)
        assert!(!is_safe_path("Cargo.toml"));
        assert!(!is_safe_path("Cargo.lock"));
        assert!(!is_safe_path("vm/Cargo.toml"));

        // The project-dir blocklist is also case-insensitive
        assert!(!is_safe_path("SRC/main.rs"));
        assert!(!is_safe_path(".GitHub/workflows/test.yml"));
        assert!(!is_safe_path("CARGO.TOML"));

        // Home directory access is not safe
        if let Some(home_path) = std::env::home_dir() {
            let home_path = home_path.to_str().unwrap();
            assert!(!is_safe_path(home_path));
        }

        // Safe paths inside CWD
        assert!(is_safe_path("."));
        assert!(is_safe_path("foo.txt"));
        assert!(is_safe_path("data.csv"));
        assert!(is_safe_path("docs/language.md"));
    }
}

/// Read the contents of an entire file into a ByteArray object
fn read_file(actor: &mut Actor, file_path: Value) -> Result<Value, String>
{
    use crate::bytearray::ByteArray;

    let file_path = unwrap_str!(file_path);

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    let bytes: Vec<u8> = match std::fs::read(file_path) {
        Err(_) => return Ok(Value::NIL),
        Ok(bytes) => bytes
    };

    actor.gc_check(
        ByteArray::alloc_size(bytes.len()),
        &mut [],
    );

    let ba = ByteArray::with_size(bytes.len(), &mut actor.alloc);
    unsafe { ba.as_ba().get_slice_mut(0, bytes.len()).copy_from_slice(&bytes) };
    Ok(ba)
}

/// Read the contents of an entire file encoded as valid UTF-8
fn read_file_utf8(actor: &mut Actor, file_path: Value) -> Result<Value, String>
{
    let file_path = unwrap_str!(file_path);

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    let s: String = match std::fs::read_to_string(file_path) {
        Err(_) => return Ok(Value::NIL),
        Ok(s) => s
    };

    actor.gc_check(
        Str::alloc_size(s.len()),
        &mut [],
    );

    Ok(Str::new(&s, &mut actor.alloc))
}

/// Writes the contents of a ByteArray to a file
fn write_file(_actor: &mut Actor, file_path: Value, bytes: Value) -> Result<Value, String>
{
    let file_path = unwrap_str!(file_path);
    let bytes = unwrap_ba!(bytes);
    let bytes = unsafe { bytes.get_slice(0, bytes.num_bytes()) };

    if !is_safe_path(&file_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", file_path));
    }

    match std::fs::write(file_path, &bytes) {
        Err(_) => Ok(Value::FALSE),
        Ok(_) => Ok(Value::TRUE)
    }
}

/// Create a directory, along with any missing parent directories.
/// Succeeds if the directory already exists.
fn make_dir(_actor: &mut Actor, dir_path: Value) -> Result<Value, String>
{
    let dir_path = unwrap_str!(dir_path);

    if !is_safe_path(&dir_path) {
        return Err(format!("requested file path breaks sandboxing rules: {}", dir_path));
    }

    match std::fs::create_dir_all(dir_path) {
        Err(_) => Ok(Value::FALSE),
        Ok(_) => Ok(Value::TRUE)
    }
}

/// Shrink the heap to a smaller size
fn vm_shrink_heap(actor: &mut Actor, new_size: Value) -> Result<Value, String>
{
    let new_size = unwrap_usize!(new_size);

    if new_size > actor.alloc.mem_size() {
        return Err("requested heap size is larger than the current heap size".into());
    }

    if actor.alloc.bytes_used() > new_size {
        return Err("requested heap size is smaller than bytes currently allocated".into());
    }

    actor.alloc.shrink_to(new_size);

    Ok(Value::NIL)
}

/// Manually trigger garbage collection in the current actor
fn vm_gc_collect(actor: &mut Actor) -> Result<Value, String>
{
    actor.gc_collect(0, &mut []);
    Ok(Value::NIL)
}

/// Get the id of the current actor
fn actor_id(actor: &mut Actor) -> Result<Value, String>
{
    Ok(actor.int64(actor.actor_id as i64))
}

/// Get the id of the parent actor
fn actor_parent(actor: &mut Actor) -> Result<Value, String>
{
    Ok(match actor.parent_id {
        Some(actor_id) => actor.int64(actor_id as i64),
        None => Value::NIL,
    })
}

/// Make the current actor sleep
fn actor_sleep(_actor: &mut Actor, msecs: Value) -> Result<Value, String>
{
    let msecs = unwrap_u64!(msecs);
    thread::sleep(Duration::from_millis(msecs));
    Ok(Value::NIL)
}

/// Spawn a new actor
/// Takes a function to call as argument
/// Returns an actor id
fn actor_spawn(actor: &mut Actor, fun: Value) -> Result<Value, String>
{
    if fun.to_clos().is_none() && !fun.is_fun() {
        return Err("actor_spawn received non-function value".into());
    }

    // The new actor is started with no arguments. Checking here reports the
    // problem at the spawn site instead of inside the actor being spawned.
    let fun_id = fun.to_fun_id().unwrap();
    let num_params = actor.get_num_params(fun_id);
    if num_params != 0 {
        return Err(format!(
            "function passed to actor_spawn should take no arguments, but takes {}",
            num_params
        ));
    }

    let actor_id = VM::new_actor(actor, fun, vec![]);
    Ok(actor.int64(actor_id as i64))
}

/// Wait for a thread to terminate, produce the return value
fn actor_join(actor: &mut Actor, actor_id: Value) -> Result<Value, String>
{
    let id = unwrap_u64!(actor_id);
    Ok(VM::join_actor(&actor.vm, id))
}

/// Send a message to an actor
/// This will return false in case of failure
fn actor_send(actor: &mut Actor, actor_id: Value, msg: Value) -> Result<Value, String>
{
    let actor_id = unwrap_u64!(actor_id);
    let res = actor.send(actor_id, msg);

    if res.is_ok() {
        Ok(Value::TRUE)
    } else {
        Ok(Value::FALSE)
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
        None => Value::NIL,
    })
}

/// End program execution
fn exit(_actor: &mut Actor, val: Value) -> Result<Value, String>
{
    let val = (unwrap_i64!(val) & 0xFF) as i32;
    std::process::exit(val);
}
