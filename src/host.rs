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
        }
    }
}

/// Get a host constant by name
/// Returns an AST expression node for the constant,
/// because we want host constants to be resolved early
pub fn get_host_const(name: &str) -> Expr
{
    use HostFn::*;

    match name
    {
        "time_current_ms" => Expr::HostFn(Fn0_1(time_current_ms)),

        "print_i64" => Expr::HostFn(Fn1_0(print_i64)),
        "print_str" => Expr::HostFn(Fn1_0(print_str)),
        "print_endl" => Expr::HostFn(Fn0_0(print_endl)),

        "actor_id" => Expr::HostFn(Fn0_1(actor_id)),
        "actor_sleep" => Expr::HostFn(Fn1_0(actor_sleep)),
        "actor_spawn" => Expr::HostFn(Fn1_1(actor_spawn)),
        "actor_join" => Expr::HostFn(Fn1_1(actor_join)),
        "actor_send" => Expr::HostFn(Fn2_1(actor_send)),
        "actor_recv" => Expr::HostFn(Fn0_1(actor_recv)),

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

fn print_i64(actor: &mut Actor, v: Value)
{
    let v = v.unwrap_i64();
    print!("{}", v);
}

/// Print a null-terminated UTF-8 string to stdout
fn print_str(actor: &mut Actor, s: Value)
{
    let rust_str = s.unwrap_rust_str();
    print!("{}", rust_str);
}

/// Print a newline characted to stdout
fn print_endl(actor: &mut Actor)
{
    println!();
}

// Get the id of the current actor
fn actor_id(actor: &mut Actor) -> Value
{
    Value::from(actor.actor_id)
}

// Make the current actor sleep
fn actor_sleep(actor: &mut Actor, msecs: Value)
{
    let msecs = msecs.unwrap_u64();
    thread::sleep(Duration::from_millis(msecs));
}

// Spawn a new actor
// Takes a function to call as argument
// Returns an actor id
fn actor_spawn(actor: &mut Actor, fun: Value) -> Value
{
    let actor_id = VM::new_actor(actor, fun, vec![]);
    Value::from(actor_id)
}

// Wait for a thread to terminate, produce the return value
fn actor_join(actor: &mut Actor, actor_id: Value) -> Value
{
    let id = actor_id.unwrap_u64();
    VM::join_actor(&actor.vm, id)
}

// Send a message to an actor
// This will return false in case of failure
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

// Receive a message from the current actor's queue
// This will block until a message is available
fn actor_recv(actor: &mut Actor) -> Value
{
    actor.recv()
}
