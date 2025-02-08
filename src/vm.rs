use std::collections::{HashSet, HashMap};
use std::{thread, thread::sleep};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use crate::ast::{Program, FunId};
use crate::alloc::Alloc;
use crate::codegen::CompiledFun;
use crate::host::*;

/// Instruction opcodes
/// Note: commonly used upcodes should be in the [0, 127] range (one byte)
///       less frequently used opcodes can take multiple bytes if necessary.
#[allow(non_camel_case_types)]
#[derive(PartialEq, Copy, Clone, Debug)]
pub enum Insn
{
    // Halt execution and produce an error
    panic,

    // No-op
    nop,

    // Push a value to the stack
    push { val: Value },

    // Stack manipulation
    pop,
    dup,
    swap,

    // Push the nth-value (indexed from the stack top) on top of the stack
    // getn 0 is equivalent to dup
    getn { idx: u16 },

    /*
    // Pop the stack top and set the nth stack slot from the top to this value
    // setn 0 is equivalent to removing the value below the current stack top
    // setn <idx:u8>
    setn,

    // Get the argument count for the current stack frame
    get_argc,
    */

    // Get the function argument at a given index
    get_arg { idx: u32 },

    /*
    // Get a variadic argument with a dynamic index variable
    // get_arg (idx)
    get_var_arg,
    */

    // Get the local variable at a given stack slot index
    // The index is relative to the base of the stack frame
    get_local { idx: u32 },

    // Set the local variable at a given stack slot index
    // The index is relative to the base of the stack frame
    set_local { idx: u32 },

    // Arithmetic
    add,
    sub,
    mul,
    div,
    modulo,

    // TODO: bitwise lsft, rsft, bit_and

    // Comparisons
    lt,
    le,
    gt,
    ge,
    eq,
    ne,

    // Logical negation
    not,

    // Type check operations
    is_nil,
    is_int64,
    is_object,
    is_array,

    // Create a closure instance
    new_clos { fun_id: FunId, num_cells: u32 },

    // Objects manipulation
    new_obj { capacity: u16 },
    //obj_copy,
    //obj_def { field: String },
    //obj_set { field: String },
    //obj_get { field: String },
    //obj_seal,

    // Array operations
    new_arr { capacity: u32 },
    arr_push,
    arr_len,
    arr_set,
    arr_get,
    arr_freeze,

    // Bytearray operations
    ba_new { capacity: u32 },
    ba_resize,
    ba_write_u32,

    // Jump if true/false
    if_true { target_ofs: i32 },
    if_false { target_ofs: i32 },

    // Unconditional jump
    jump { target_ofs: i32 },

    // Call a host function
    //call_host { host_fn: HostFn, argc: u16 },

    // Call a function using the call stack
    // call (arg0, arg1, ..., argN)
    call { argc: u16 },

    // Call a known function
    //call_known { argc: u16, callee: *mut Object },
    //call_pc { argc: u16, callee: *mut Object, target_pc: usize },

    // Return
    ret,
}

pub struct Closure
{
    fun_id: FunId,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Value
{
    Nil,
    False,
    True,
    Int64(i64),
    Float64(f64),
    Fun(FunId),
    Closure(*mut Closure),

    // TODO: HostFun?
    // need some kind of id for this to work,
    // can't just use a string here?
    HostFun(&'static u32)
}
use Value::{False, True, Int64, Float64};

// Allow sending Value between threads
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

impl Value
{
    /*
    pub fn is_object(&self) -> bool
    {
        match self {
            Value::Object(_) => true,
            _ => false,
        }
    }
    */

    /*
    pub fn is_array(&self) -> bool
    {
        match self {
            Value::Array(_) => true,
            _ => false,
        }
    }
    */

    pub fn unwrap_usize(&self) -> usize
    {
        match self {
            Int64(v) => (*v).try_into().unwrap(),
            _ => panic!("expected int64 value but got {:?}", self)
        }
    }

    pub fn unwrap_u64(&self) -> u64
    {
        match self {
            Int64(v) => (*v).try_into().unwrap(),
            _ => panic!("expected int64 value but got {:?}", self)
        }
    }

    pub fn unwrap_u32(&self) -> u32
    {
        match self {
            Int64(v) => (*v).try_into().unwrap(),
            _ => panic!("expected int64 value but got {:?}", self)
        }
    }

    pub fn unwrap_u8(&self) -> u8
    {
        match self {
            Int64(v) => (*v).try_into().unwrap(),
            _ => panic!("expected int64 value but got {:?}", self)
        }
    }

    pub fn unwrap_i64(&self) -> i64
    {
        match self {
            Int64(v) => *v,
            _ => panic!("expected int64 value but got {:?}", self)
        }
    }

    pub fn unwrap_f64(&self) -> f64
    {
        match self {
            Float64(v) => *v,
            _ => panic!("expected float64 value but got {:?}", self)
        }
    }
}

impl From<usize> for Value {
    fn from(val: usize) -> Self {
        Value::Int64(val.try_into().unwrap())
    }
}

impl From<u64> for Value {
    fn from(val: u64) -> Self {
        Value::Int64(val.try_into().unwrap())
    }
}

impl From<u8> for Value {
    fn from(val: u8) -> Self {
        Value::Int64(val.try_into().unwrap())
    }
}

/// Mesage to be sent to an actor
pub struct Message
{
    // Sender actor id
    // Can be none when the message is a callback
    sender: u64,

    // Message to be sent
    msg: Value,
}

#[derive(Copy, Clone, Debug)]
struct StackFrame
{
    // Function currently executing
    fun: Value,

    // Argument count (number of args supplied)
    argc: u16,

    // Previous base pointer at the time of call
    prev_bp: usize,

    // Return address
    ret_addr: usize,
}

pub struct Actor
{
    // Actor id
    pub actor_id: u64,

    // Parent VM
    pub vm: Arc<Mutex<VM>>,

    // Private allocator
    pub alloc: Alloc,

    // Message queue receiver endpoint
    queue_rx: mpsc::Receiver<Message>,

    // Cache of actor ids to message queue endpoints
    actor_map: HashMap<u64, mpsc::Sender<Message>>,

    // Value stack
    stack: Vec<Value>,

    // List of stack frames (activation records)
    frames: Vec<StackFrame>,

    // Map of compiled functions
    funs: HashMap<FunId, CompiledFun>,

    // Array of compiled instructions
    insns: Vec<Insn>,
}

impl Actor
{
    pub fn new(actor_id: u64, vm: Arc<Mutex<VM>>, queue_rx: mpsc::Receiver<Message>) -> Self
    {
        Self {
            actor_id,
            vm,
            alloc: Alloc::new(),
            queue_rx,
            actor_map: HashMap::default(),
            stack: Vec::default(),
            frames: Vec::default(),
            insns: Vec::default(),
            funs: HashMap::default(),
        }
    }

    // Receive a message from the message queue
    // This will block until a message is available
    fn recv(&mut self) -> Value
    {
        //use crate::window::poll_ui_msg;

        if self.actor_id != 0 {
            let msg = self.queue_rx.recv().unwrap();
            return msg.msg;
        }

        // Actor 0 (the main actor) may need to poll for UI events
        loop {
            // Poll for UI messages
            //let ui_msg = poll_ui_msg(self);
            //if let Some(msg) = ui_msg {
            //    return msg;
            //}

            // Block on the message queue for up to 10ms
            let msg = self.queue_rx.recv_timeout(Duration::from_millis(10));

            if let Ok(msg) = msg {
                return msg.msg;
            }
        }
    }

    // Send a message to another actor
    fn send(&mut self, actor_id: u64, msg: Value) -> Result<(), ()>
    {
        //
        // TODO: logic to copy objects
        //

        // Lookup the queue endpoint in our local cache
        let mut actor_tx = self.actor_map.get(&actor_id);

        if actor_tx.is_none() {
            let vm = self.vm.lock().unwrap();

            let tx = vm.actor_txs.get(&actor_id);

            if tx.is_none() {
                return Err(());
            }

            self.actor_map.insert(actor_id, tx.unwrap().clone());

            actor_tx = self.actor_map.get(&actor_id);
        }

        let actor_tx = actor_tx.unwrap();

        match actor_tx.send(Message { sender: self.actor_id, msg }) {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    // Get a compiled function entry for a given function id
    fn get_compiled_fun(&mut self, fun_id: FunId) -> CompiledFun
    {
        if let Some(entry) = self.funs.get(&fun_id) {
            return *entry;
        }

        // Borrow the function from the VM and compile it
        let vm = self.vm.lock().unwrap();
        let fun = &vm.prog.funs[&fun_id];
        let entry = fun.gen_code(&mut self.insns).unwrap();
        self.funs.insert(fun_id, entry);

        // Return the compiled function entry
        entry
    }

    // Call and execute a function in this actor
    pub fn call(&mut self, fun_id: FunId, args: &[Value]) -> Value
    {
        assert!(self.stack.len() == 0);
        assert!(self.frames.len() == 0);

        // Get a compiled address for this function
        let fun_entry = self.get_compiled_fun(fun_id);
        let mut pc = fun_entry.entry_pc;

        if args.len() != fun_entry.num_params {
            panic!();
        }

        // Push the arguments on the stack
        for arg in args {
            self.stack.push(*arg);
        }

        // Push a new stack frame
        self.frames.push(StackFrame {
            fun: Value::Fun(fun_id),
            argc: args.len().try_into().unwrap(),
            prev_bp: usize::MAX,
            ret_addr: usize::MAX,
        });

        // The base pointer will point at the first local
        let mut bp = self.stack.len();

        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        macro_rules! push_bool {
            ($b: expr) => { push!(if $b { True } else { False }) }
        }

        loop
        {
            if pc >= self.insns.len() {
                panic!("pc out of bounds");
            }

            let insn = self.insns[pc];
            pc += 1;
            //println!("executing {:?}", insn);

            match insn {
                Insn::nop => {},

                Insn::panic => {
                    panic!("encountered panic opcode");
                }

                Insn::push { val } => {
                   self.stack.push(val);
                }

                Insn::dup => {
                    let val = pop!();
                    push!(val);
                    push!(val);
                }

                Insn::pop => {
                    pop!();
                }

                Insn::swap => {
                    let a = pop!();
                    let b = pop!();
                    push!(a);
                    push!(b);
                }

                Insn::getn { idx } => {
                    let idx = idx as usize;
                    let val = self.stack[self.stack.len() - (1 + idx)];
                    push!(val);
                }

                Insn::get_arg { idx } => {
                    let argc = self.frames[self.frames.len() - 1].argc as usize;
                    let idx = idx as usize;

                    if idx >= argc {
                        panic!("invalid index in get_arg, idx={}, argc={}", idx, argc);
                    }

                    // Last argument is at bp - 1 (if there are arguments)
                    let stack_idx = (bp - argc) + idx;
                    let arg_val = self.stack[stack_idx];
                    push!(arg_val);
                    //println!("arg_val={:?}", arg_val);
                }

                Insn::get_local { idx } => {
                    let idx = idx as usize;

                    if bp + idx >= self.stack.len() {
                        panic!("invalid index {} in get_local", idx);
                    }

                    push!(self.stack[bp + idx]);
                }

                Insn::set_local { idx } => {
                    let idx = idx as usize;
                    let val = pop!();

                    if bp + idx >= self.stack.len() {
                        panic!("invalid index in set_local");
                    }

                    self.stack[bp + idx] = val;
                }

                Insn::add => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 + v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                Insn::sub => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 - v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                Insn::mul => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 * v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::div => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 / v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::modulo => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 % v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Less than
                Insn::lt => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 < v1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                // Less than or equal
                Insn::le => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 <= v1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                // Greater than or equal
                Insn::ge => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 >= v1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                Insn::eq => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 == v1,
                        //(Value::String(p0), Value::String(p1)) => p0 == p1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                Insn::ne => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 != v1,
                        //(Value::String(p0), Value::String(p1)) => p0 != p1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                // Logical negation
                Insn::not => {
                    let v0 = pop!();

                    let b = match v0 {
                        Value::True => Value::False,
                        Value::False => Value::True,
                        _ => panic!()
                    };

                    push!(b);
                }

                // Create a new closure
                Insn::new_clos { fun_id, num_cells } => {
                    let clos = Closure { fun_id };
                    //lew new_clos = self.alloc.alloc();
                    //push!(Value::Object(new_obj))

                    todo!();
                }

                /*
                // Create new empty object
                Insn::obj_new => {
                    let new_obj = Object::new(&mut self.alloc);
                    push!(Value::Object(new_obj))
                }

                // Copy object
                Insn::obj_copy => {
                    let obj = pop!().unwrap_obj();
                    let new_obj = Object::copy(obj, &mut self.alloc);
                    push!(Value::Object(new_obj))
                }

                // Define constant field
                Insn::obj_def { field_name } => {
                    let obj = pop!().unwrap_obj();
                    let val = pop!();
                    Object::def_const(obj, Value::String(field_name), val);
                }

                // Set object field
                Insn::obj_set { field_name } => {
                    let obj = pop!().unwrap_obj();
                    let val = pop!();
                    Object::set(obj, Value::String(field_name), val);
                }

                // Get object field
                Insn::obj_get { field_name } => {
                    let obj = pop!().unwrap_obj();
                    let val = Object::get(obj, Value::String(field_name));
                    push!(val);
                }

                // Seal object
                Insn::obj_seal => {
                    let obj = pop!().unwrap_obj();
                    Object::seal(obj);
                }
                */

                /*
                // Create new empty array
                Insn::arr_new { capacity } => {
                    let new_arr = Array::new(
                        &mut self.alloc,
                        capacity as usize
                    );
                    push!(Value::Array(new_arr))
                }

                Insn::arr_push => {
                    let arr = pop!().unwrap_arr();
                    let val = pop!();
                    Array::push(arr, val, &mut self.alloc);
                }

                Insn::arr_get => {
                    let idx = pop!().unwrap_u64();
                    let arr = pop!();

                    let val = match arr {
                        Value::Array(p) => Array::get(p, idx),
                        Value::ByteArray(p) => Value::from(ByteArray::get(p, idx)),
                        _ => panic!("expected array type")
                    };

                    push!(val);
                }

                Insn::arr_set => {
                    let idx = pop!().unwrap_u64();
                    let arr = pop!();
                    let val = pop!();

                    match arr {
                        Value::Array(p) => Array::set(p, idx, val),
                        Value::ByteArray(p) => {
                            let b = val.unwrap_u8();
                            ByteArray::set(p, idx, b);
                        }
                        _ => panic!("expected array type")
                    };
                }

                Insn::arr_len => {
                    let len = match pop!() {
                        Value::Array(p) => Array::len(p),
                        Value::ByteArray(p) => ByteArray::len(p),
                        _ => panic!(),
                    };

                    push!(Value::from(len));
                }

                // Freeze array
                Insn::arr_freeze => {
                    let arr = pop!().unwrap_arr();
                    Array::freeze(arr);
                }
                */

                /*
                // Create new empty bytearray
                Insn::ba_new { capacity } => {
                    let new_arr = ByteArray::new(
                        &mut self.alloc,
                        capacity as usize
                    );
                    push!(Value::ByteArray(new_arr))
                }

                // Resize byte array
                Insn::ba_resize => {
                    let fill_val = pop!().unwrap_u8();
                    let new_len = pop!().unwrap_u64();
                    let arr = pop!().unwrap_ba();
                    ByteArray::resize(arr, new_len, fill_val, &mut self.alloc);
                }

                // Write u32 value
                Insn::ba_write_u32 => {
                    let val = pop!().unwrap_u32();
                    let idx = pop!().unwrap_u64();
                    let arr = pop!().unwrap_ba();
                    ByteArray::write_u32(arr, idx, val);
                }
                */

                // Jump if true
                Insn::if_true { target_ofs } => {
                    let v = pop!();

                    match v {
                        Value::True => { pc = ((pc as i64) + (target_ofs as i64)) as usize }
                        Value::False => {}
                        _ => panic!()
                    }
                }

                // Jump if false
                Insn::if_false { target_ofs } => {
                    let v = pop!();

                    match v {
                        Value::False => { pc = ((pc as i64) + (target_ofs as i64)) as usize }
                        Value::True => {}
                        _ => panic!("{:?}", v)
                    }
                }

                // Unconditional jump
                Insn::jump { target_ofs } => {
                    pc = ((pc as i64) + (target_ofs as i64)) as usize
                }

                /*
                Insn::call_host { host_fn, argc } => {
                    if host_fn.num_params() != (argc as usize) {
                        panic!();
                    }

                    match host_fn
                    {
                        HostFn::Fn0_0(fun) => {
                            fun(self);
                            push!(Value::None);
                        }

                        HostFn::Fn0_1(fun) => {
                            let v = fun(self);
                            push!(v);
                        }

                        HostFn::Fn1_0(fun) => {
                            let a0 = pop!();
                            fun(self, a0);
                            push!(Value::None);
                        }

                        HostFn::Fn1_1(fun) => {
                            let a0 = pop!();
                            let v = fun(self, a0);
                            push!(v);
                        }

                        HostFn::Fn2_0(fun) => {
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(self, a0, a1);
                            push!(Value::None);
                        }

                        HostFn::Fn2_1(fun) => {
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(self, a0, a1);
                            push!(v);
                        }

                        HostFn::Fn3_0(fun) => {
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(self, a0, a1, a2);
                            push!(Value::None);
                        }

                        HostFn::Fn3_1(fun) => {
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(self, a0, a1, a2);
                            push!(v);
                        }

                        HostFn::Fn4_0(fun) => {
                            let a3 = pop!();
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(self, a0, a1, a2, a3);
                            push!(Value::None);
                        }

                        HostFn::Fn4_1(fun) => {
                            let a3 = pop!();
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(self, a0, a1, a2, a3);
                            push!(v);
                        }
                    }
                }
                */

                // call (arg0, arg1, ..., argN, fun)
                Insn::call { argc } => {
                    // Function to call
                    let fun = pop!();

                    // Argument count
                    assert!(argc as usize <= self.stack.len() - bp);

                    let fun_id = match fun {
                        Value::Fun(id) => id,
                        Value::Closure(clos) => unsafe { (*clos).fun_id },
                        _ => panic!()
                    };

                    // Get a compiled address for this function
                    let fun_entry = self.get_compiled_fun(fun_id);
                    pc = fun_entry.entry_pc;

                    if args.len() != fun_entry.num_params {
                        panic!();
                    }

                    self.frames.push(StackFrame {
                        argc,
                        fun,
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                }

                /*
                // call (arg0, arg1, ..., argN, fun)
                Insn::call_known { argc, callee } => {
                    // Get a compiled address for this function
                    let target_pc = self.get_version(callee, 0);

                    // Patch this instruction with the compiled pc
                    self.insns[pc - 1] = Insn::call_pc { argc, callee, target_pc };

                    // Executed the patched instruction next
                    pc -= 1;
                }
                */

                /*
                // call (arg0, arg1, ..., argN, fun)
                Insn::call_pc { argc, callee, target_pc } => {
                    // Argument count
                    assert!(argc as usize <= self.stack.len() - bp);

                    self.frames.push(StackFrame {
                        argc,
                        fun,
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();

                    // Get a compiled address for this function
                    pc = target_pc;
                }
                */

                Insn::ret => {
                    if self.stack.len() <= bp {
                        panic!("ret with no return value on stack");
                    }

                    let ret_val = pop!();
                    //println!("ret_val={:?}", ret_val);

                    // If this is a top-level return
                    if self.frames.len() == 1 {
                        self.stack.clear();
                        self.frames.clear();
                        return ret_val;
                    }

                    assert!(self.frames.len() > 0);
                    let top_frame = self.frames.pop().unwrap();

                    // Pop all local variables and arguments
                    // We pop arguments in the callee so we can support tail calls
                    let argc = top_frame.argc as usize;
                    assert!(self.stack.len() >= bp - argc);
                    self.stack.truncate(bp - argc);

                    pc = top_frame.ret_addr;
                    bp = top_frame.prev_bp;

                    push!(ret_val);
                }

                _ => panic!("unknown opcode {:?}", insn)
            }
        }
    }
}

pub struct VM
{
    // Program to run
    prog: Program,

    // Next actor id to assign
    next_actor_id: u64,

    // Map from actor ids to thread join handles
    threads: HashMap<u64, thread::JoinHandle<Value>>,

    // Map from actor ids to message queue endpoints
    actor_txs: HashMap<u64, mpsc::Sender<Message>>,

    // Reference to self
    // Needed to instantiate actors
    vm: Option<Arc<Mutex<VM>>>,
}

// Needed to send Arc<Mutex<VM>> to thread
unsafe impl Send for VM {}

// Note: all VM methods operate on an Arc<Mutex<VM>>
// This is because we want to avoid people grabbing
// the lock for the entire duration of a call.
impl VM
{
    pub fn new(prog: Program) -> Arc<Mutex<VM>>
    {
        let vm = Self {
            prog,
            next_actor_id: 0,
            threads: HashMap::default(),
            actor_txs: HashMap::default(),
            vm: None
        };

        let vm = Arc::new(Mutex::new(vm));

        // Store a reference to the mutex on the VM
        // This is so we can pass this reference to threads
        vm.lock().unwrap().vm = Some(vm.clone());

        vm
    }

    // Create a new actor
    pub fn new_actor(vm: &Arc<Mutex<VM>>, fun: FunId, args: Vec<Value>) -> u64
    {
        let vm_mutex = vm.clone();

        // Assign an actor id
        let mut vm_ref = vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        vm_ref.next_actor_id += 1;
        drop(vm_ref);

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::channel::<Message>();

        // Spawn a new thread for the actor
        let handle = thread::spawn(move || {
            let mut actor = Actor::new(actor_id, vm_mutex, queue_rx);
            actor.call(fun, &args)
        });

        // Store the join handles and queue endpoints on the VM
        let mut vm_ref = vm.lock().unwrap();
        vm_ref.threads.insert(actor_id, handle);
        vm_ref.actor_txs.insert(actor_id, queue_tx);
        drop(vm_ref);

        actor_id
    }

    // Wait for an actor to produce a result and return it.
    pub fn join_actor(vm: &Arc<Mutex<VM>>, tid: u64) -> Value
    {
        // Get the join handle, then release the VM lock
        let mut vm = vm.lock().unwrap();
        let mut handle = vm.threads.remove(&tid).unwrap();
        drop(vm);

        // Note: there is no need to copy data when joining,
        // because the actor sending the data is done running
        handle.join().expect(&format!("could not actor thread with id {}", tid))
    }

    // Call a function in the main actor
    pub fn call(vm: &mut Arc<Mutex<VM>>, fun: FunId, args: Vec<Value>) -> Value
    {
        let vm_mutex = vm.clone();

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::channel::<Message>();

        // Assign an actor id
        // Store the queue endpoints on the VM
        let mut vm_ref = vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        assert!(actor_id == 0);
        vm_ref.next_actor_id += 1;
        vm_ref.actor_txs.insert(actor_id, queue_tx);
        drop(vm_ref);

        let mut actor = Actor::new(actor_id, vm_mutex, queue_rx);
        actor.call(fun, &args)
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::parser::parse_str;

    fn eval(s: &str) -> Value
    {
        let mut prog = parse_str(s).unwrap();
        prog.resolve_syms().unwrap();
        let main_fn = prog.main_fn;
        let mut vm = VM::new(prog);
        VM::call(&mut vm, main_fn, vec![])
    }

    fn eval_eq(s: &str, v: Value)
    {
        let val = eval(s);
        assert_eq!(val, v);
    }

    #[test]
    fn vm_new()
    {
        let prog = Program::default();
        let _vm = VM::new(prog);
    }

    #[test]
    fn empty_unit()
    {
        eval_eq("", Value::Nil);
    }

    #[test]
    fn simple_exprs()
    {
        eval_eq("return 77;", Value::Int64(77));
        eval_eq("return -77;", Value::Int64(-77));
        eval_eq("return 1 + 5;", Value::Int64(6));
        eval_eq("return 5 - 3;", Value::Int64(2));
        eval_eq("return 2 * 3 + 4;", Value::Int64(10));
        eval_eq("return 5 + 2 * -2;", Value::Int64(1));
    }

    #[test]
    fn if_else()
    {
        eval_eq("if (true) return 1; return 2;", Value::Int64(1));
        eval_eq("if (false) return 1; return 2;", Value::Int64(2));
        eval_eq("if (true) return 77; else return 88;", Value::Int64(77));
        eval_eq("if (false) return 77; else return 88;", Value::Int64(88));
        eval_eq("if (3 < 5) return 1; return 2;", Value::Int64(1));
    }

    #[test]
    fn let_expr()
    {
        eval_eq("let x = 1; return x;", Value::Int64(1));
        eval_eq("let var x = 1; return x;", Value::Int64(1));
        eval_eq("let x = 1; let y = 2; return x + y;", Value::Int64(3));
    }

    #[test]
    fn assign()
    {
        eval_eq("let var x = 1; x = 2; return x;", Value::Int64(2));

        // FIXME: this should fail
        //eval_eq("let x = 1; x = 2; return x;", Value::Int64(2));
    }

    #[test]
    fn assert()
    {
        eval("let x = 1; assert(x == 1);");
        eval("let x = 1; assert(x < 2);");
        eval("let x = 1; x = x + 1; assert(x < 10);");
    }

    #[test]
    fn while_loop()
    {
        eval_eq("let var x = 1; while (x < 10) { x = x + 1; } return x;", Value::Int64(10));
    }




    /*
    #[test]
    fn sub_ab()
    {
        // This checks that argument ordering is handled correctly
        let ret = run_image("tests/sub_ab.zim");
        assert!(ret == Value::Int64(2));
    }

    #[test]
    fn fact()
    {
        let ret = run_image("tests/fact.zim");
        assert!(ret == Value::Int64(720));
    }

    #[test]
    fn fib()
    {
        let ret = run_image("tests/fib.zim");
        dbg!(ret);
        assert!(ret == Value::Int64(6765));
    }

    #[test]
    fn examples()
    {
        let ret = run_image("examples/hello_world.zim");
        assert!(ret == Value::Int64(0));
    }
    */
}
