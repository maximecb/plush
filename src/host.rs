use std::env;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::alloc::Alloc;
use crate::vm::{Value, VM, Actor};
use crate::ast::Expr;

/// Host function signature
/// Note: the in/out arg count should be fixed so
///       that we can JIT host calls efficiently
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum HostFn
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

impl HostFn
{
    pub fn num_params(&self) -> usize
    {
        match self {
            Self::Fn0_0(_) => 0,
            Self::Fn0_1(_) => 0,
            Self::Fn1_0(_) => 1,
            Self::Fn1_1(_) => 1,
            Self::Fn2_0(_) => 2,
            Self::Fn2_1(_) => 2,
            Self::Fn3_0(_) => 3,
            Self::Fn3_1(_) => 3,
            Self::Fn4_0(_) => 4,
            Self::Fn4_1(_) => 4,
            Self::Fn5_0(_) => 5,
            Self::Fn5_1(_) => 5,
            Self::Fn8_0(_) => 8,
        }
    }

    pub fn has_ret(&self) -> bool
    {
        match self {
            Self::Fn0_0(_) => false,
            Self::Fn0_1(_) => true,
            Self::Fn1_0(_) => false,
            Self::Fn1_1(_) => true,
            Self::Fn2_0(_) => false,
            Self::Fn2_1(_) => true,
            Self::Fn3_0(_) => false,
            Self::Fn3_1(_) => true,
            Self::Fn4_0(_) => false,
            Self::Fn4_1(_) => true,
            Self::Fn5_0(_) => false,
            Self::Fn5_1(_) => true,
            Self::Fn8_0(_) => false,
        }
    }
}

/// Get a host constant by name
/// Returns an AST expression node for the constant,
/// because we want host constants to be resolved early
pub fn get_host_const(name: &str) -> Expr
{
    use HostFn::*;
    use crate::window::*;

    match name
    {
        "time_current_ms" => Expr::HostFn(Fn0_1(time_current_ms)),

        "cmd_num_args" => Expr::HostFn(Fn0_1(cmd_num_args)),
        "cmd_get_arg" => Expr::HostFn(Fn1_1(cmd_get_arg)),

        "print" => Expr::HostFn(Fn1_0(print)),
        "println" => Expr::HostFn(Fn1_0(println)),
        "readln" => Expr::HostFn(Fn0_1(readln)),

        "actor_id" => Expr::HostFn(Fn0_1(actor_id)),
        "actor_parent" => Expr::HostFn(Fn0_1(actor_parent)),
        "actor_sleep" => Expr::HostFn(Fn1_0(actor_sleep)),
        "actor_spawn" => Expr::HostFn(Fn1_1(actor_spawn)),
        "actor_join" => Expr::HostFn(Fn1_1(actor_join)),
        "actor_send" => Expr::HostFn(Fn2_1(actor_send)),
        "actor_recv" => Expr::HostFn(Fn0_1(actor_recv)),
        "actor_poll" => Expr::HostFn(Fn0_1(actor_poll)),

        "window_create" => Expr::HostFn(Fn4_1(window_create)),
        "window_draw_frame" => Expr::HostFn(Fn2_0(window_draw_frame)),

        "exit" => Expr::HostFn(Fn1_0(exit)),

        _ => panic!("unknown host constant \"{name}\"")
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
