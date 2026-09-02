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
    // Arities are only listed once a host function needs them, which is
    // why this jumps from five to seven
    Fn7(fn(actor: &mut Actor, a0: Value, a1: Value, a2: Value, a3: Value, a4: Value, a5: Value, a6: Value) -> Result<Value, String>),
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
            Fn7(_) => 7,
        }
    }
}

/// Pick the `FnPtr` variant for an arity. The table states the arity as
/// a number, and the compiler checks it against the signature of the
/// function it is paired with
macro_rules! host_fn_ptr {
    (0, $f:path) => { FnPtr::Fn0($f) };
    (1, $f:path) => { FnPtr::Fn1($f) };
    (2, $f:path) => { FnPtr::Fn2($f) };
    (3, $f:path) => { FnPtr::Fn3($f) };
    (4, $f:path) => { FnPtr::Fn4($f) };
    (5, $f:path) => { FnPtr::Fn5($f) };
    (7, $f:path) => { FnPtr::Fn7($f) };
}

/// Declare the table of host functions, in two groups. An entry names
/// the Rust function that implements it, along with its arity.
///
/// Plush code knows a global by that same name. A method's name carries
/// a type prefix, and several of them answer to the same bare name, so
/// each method gives the name it is called by.
///
/// Generating the id enum, the table and the name lookup from one list
/// is what keeps them in step: an id is always a valid index, and only
/// the globals are reachable by name.
macro_rules! def_host_fns {
    (
        globals { $($g_id:ident($g_argc:tt),)* }
        methods { $($m_name:ident: $m_id:ident($m_argc:tt),)* }
    ) => {
        pub const NUM_HOST_FNS: usize =
            0 $(+ { let _ = stringify!($g_id); 1 })*
              $(+ { let _ = stringify!($m_name); 1 })*;

        // Ids are stored in u16 instruction operands
        const _: () = assert!(NUM_HOST_FNS <= u16::MAX as usize);

        /// Identifies a host function by its position in `HOST_FNS`.
        /// Narrow enough to sit in an instruction operand.
        #[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
        #[allow(non_camel_case_types)]
        #[repr(u16)]
        pub enum HostFnId
        {
            // The first entry stands in for "not resolved yet", which is
            // what a call cache holds until its site has run
            #[default]
            $($g_id,)*
            $($m_id,)*
        }

        /// Every host function the VM provides, in `HostFnId` order.
        /// The table is immutable and shared by every actor, so looking
        /// one up costs an indexed load and no locking.
        pub static HOST_FNS: [HostFn; NUM_HOST_FNS] = [
            $(HostFn { name: stringify!($g_id), f: host_fn_ptr!($g_argc, $g_id) },)*
            $(HostFn { name: stringify!($m_name), f: host_fn_ptr!($m_argc, $m_id) },)*
        ];

        impl HostFnId
        {
            /// Look up a host function that Plush code can name directly.
            /// Methods are deliberately not reachable this way: they are
            /// found by the type they are defined on instead
            pub fn from_name(name: &str) -> Option<HostFnId>
            {
                match name {
                    $(stringify!($g_id) => Some(HostFnId::$g_id),)*
                    _ => None
                }
            }
        }
    };
}

impl HostFnId
{
    /// Recover an id from its index, as an instruction operand holds it
    pub fn from_index(idx: u16) -> HostFnId
    {
        assert!((idx as usize) < NUM_HOST_FNS);
        unsafe { std::mem::transmute(idx) }
    }

    /// Get the host function this id refers to
    pub fn get(self) -> &'static HostFn
    {
        &HOST_FNS[self as usize]
    }
}

use crate::array::*;
use crate::audio::*;
use crate::bytearray::*;
use crate::runtime::*;
use crate::window::*;

def_host_fns! {
    // Functions Plush code names directly
    globals {
        time_current_ms(0),
        cmd_num_args(0),
        cmd_get_arg(1),
        cmd_get_arg_or(2),
        print(1),
        println(1),
        readln(0),
        read_file(1),
        read_file_utf8(1),
        write_file(2),
        make_dir(1),
        vm_shrink_heap(1),
        vm_gc_collect(0),
        actor_id(0),
        actor_parent(0),
        actor_sleep(1),
        actor_spawn(1),
        actor_join(1),
        actor_send(2),
        actor_recv(0),
        actor_poll(0),
        window_create(4),
        window_draw_frame(2),
        audio_open_output(2),
        audio_write_samples(2),
        audio_open_input(2),
        audio_read_samples(4),
        exit(1),
    }

    // Methods on primitive types, found through `runtime::get_method`
    methods {
        to_s: true_to_s(1),
        to_s: false_to_s(1),
        to_s: nil_to_s(1),

        abs: int64_abs(1),
        min: int64_min(2),
        max: int64_max(2),
        clip: int64_clip(3),
        idiv: int64_idiv(2),
        to_f: int64_to_f(1),
        to_s: int64_to_s(1),
        comma_sep: int64_comma_sep(1),
        to_hex: int64_to_hex(2),

        abs: float64_abs(1),
        ceil: float64_ceil(1),
        floor: float64_floor(1),
        trunc: float64_trunc(1),
        sin: float64_sin(1),
        cos: float64_cos(1),
        tan: float64_tan(1),
        atan: float64_atan(1),
        sqrt: float64_sqrt(1),
        min: float64_min(2),
        max: float64_max(2),
        clip: float64_clip(3),
        pow: float64_pow(2),
        exp: float64_exp(1),
        ln: float64_ln(1),
        to_f: float64_to_f(1),
        to_s: float64_to_s(1),
        format_decimals: float64_format_decimals(2),

        from_codepoint: string_from_codepoint(2),
        byte_at: string_byte_at(2),
        char_at: string_char_at(2),
        parse_int: string_parse_int(2),
        parse_float: string_parse_float(1),
        trim: string_trim(1),
        upper: string_upper(1),
        lower: string_lower(1),
        split: string_split(2),
        to_s: string_to_s(1),

        with_size: array_with_size(3),
        push: array_push(2),
        pop: array_pop(1),
        remove: array_remove(2),
        insert: array_insert(3),
        append: array_append(2),
        resize: array_resize(3),

        with_size: ba_with_size(2),
        load_u32: ba_load_u32(2),
        store_u32: ba_store_u32(3),
        load_u16: ba_load_u16(2),
        store_u16: ba_store_u16(3),
        load_f32: ba_load_f32(2),
        store_f32: ba_store_f32(3),
        get_u32: ba_get_u32(2),
        set_u32: ba_set_u32(3),
        get_f32: ba_get_f32(2),
        set_f32: ba_set_f32(3),
        dot_f32: ba_dot_f32(7),
        num_u32: ba_num_u32(1),
        memcpy: ba_memcpy(5),
        resize: ba_resize(2),
        zero_fill: ba_zero_fill(1),
        fill_u32: ba_fill_u32(4),

        has: dict_has(2),

        dump_bytecode: fun_dump_bytecode(1),
    }
}

/// Get a host constant by name
/// Returns an AST expression node for the constant,
/// because we want host constants to be resolved early
pub fn get_host_const(name: &str, fun: &Function, prog: &Program) -> Expr
{
    // This constant is only true inside the main unit
    if name == "MAIN_UNIT" {
        if fun.id == prog.main_fn {
            return Expr::True;
        } else {
            return Expr::False;
        }
    }

    match HostFnId::from_name(name) {
        Some(host_fn) => Expr::HostFn(host_fn),
        None => panic!("unknown host constant `{name}`")
    }
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

    #[test]
    fn host_fn_ids()
    {
        use crate::host::HostFnId;

        // The id enum and the table are generated from one list, so an
        // id has to land on the entry it was declared with
        assert_eq!(HostFnId::print.get().name, "print");
        assert_eq!(HostFnId::ba_fill_u32.get().name, "fill_u32");
        assert_eq!(HostFnId::float64_clip.get().num_params(), 3);

        assert_eq!(HostFnId::from_name("println"), Some(HostFnId::println));
        assert_eq!(HostFnId::from_name("nope"), None);

        // Methods are not reachable as globals, even though they sit in
        // the same table
        assert_eq!(HostFnId::from_name("to_s"), None);
        assert_eq!(HostFnId::from_name("int64_abs"), None);
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
