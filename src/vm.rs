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

    // Bitwise operations
    bit_and,
    bit_or,
    bit_xor,
    lshift,
    rshift,

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

    // Closure operations
    clos_new { fun_id: FunId, num_slots: u32 },
    clos_set { idx: u32 },
    clos_get { idx: u32 },

    // Objects manipulation
    obj_new,
    //obj_extend,
    obj_def { field: *const String },
    obj_set { field: *const String },
    obj_get { field: *const String },
    obj_seal,

    // Array operations
    arr_new { capacity: u32 },
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

    // Captured variable slots
    slots: Vec<Value>,
}

#[derive(Clone, Default)]
pub struct Object
{
    fields: HashMap<String, (bool, Value)>,
    sealed: bool,
}

impl Object
{
    fn seal(&mut self)
    {
        self.sealed = true;
    }

    // Define an immutable field field
    fn def_const(&mut self, field_name: &str, val: Value)
    {
        if let Some(_) = self.fields.get(field_name) {
            panic!();
        }

        self.fields.insert(field_name.to_string(), (false, val));
    }

    // Set the value associated with a given field
    fn set(&mut self, field_name: &str, new_val: Value)
    {
        if let Some((mutable, val)) = self.fields.get_mut(field_name) {
            if *mutable == false {
                panic!("write to immutable field");
            }

            *val = new_val;
        } else {
            self.fields.insert(field_name.to_string(), (true, new_val));
        }
    }

    // Get the value associated with a given field
    fn get(&mut self, field_name: &str) -> Value
    {
        if let Some((_, val)) = self.fields.get(field_name) {
            *val
        } else {
            panic!();
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Value
{
    // Uninitialized. This should never be observed.
    Undef,
    Nil,
    False,
    True,
    Int64(i64),
    Float64(f64),

    // String constant
    String(*const String),

    Fun(FunId),
    Closure(*mut Closure),
    HostFn(HostFn),

    Object(*mut Object),
    //Array(*mut Array),
}
use Value::{Undef, Nil, False, True, Int64, Float64};

// Allow sending Value between threads
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

impl Value
{
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
    pub fn unwrap_rust_str(&self) -> &str
    {
        match self {
            Value::String(p) => unsafe { &**p },
            _ => panic!("expected string value but got {:?}", self)
        }
    }

    pub fn unwrap_obj(&mut self) -> &mut Object
    {
        match self {
            Value::Object(p) => unsafe { &mut **p },
            _ => panic!("expected object value but got {:?}", self)
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

    /// Receive a message from the message queue
    /// This will block until a message is available
    pub fn recv(&mut self) -> Value
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

    /// Send a message to another actor
    pub fn send(&mut self, actor_id: u64, msg: Value) -> Result<(), ()>
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

    /// Get a compiled function entry for a given function id
    fn get_compiled_fun(&mut self, fun_id: FunId) -> CompiledFun
    {
        if let Some(entry) = self.funs.get(&fun_id) {
            return *entry;
        }

        // Borrow the function from the VM and compile it
        let vm = self.vm.lock().unwrap();
        let fun = &vm.prog.funs[&fun_id];
        let entry = fun.gen_code(&mut self.insns, &mut self.alloc).unwrap();
        self.funs.insert(fun_id, entry);

        // Return the compiled function entry
        entry
    }

    /// Call a host function
    pub fn call_host(&mut self, host_fn: HostFn, argc: usize)
    {
        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        if host_fn.num_params() != argc {
            panic!(
                "incorrect argument count for host functions, got {}, expected {}",
                argc,
                host_fn.num_params()
            );
        }

        match host_fn
        {
            HostFn::Fn0_0(fun) => {
                fun(self);
                push!(Value::Nil);
            }

            HostFn::Fn0_1(fun) => {
                let v = fun(self);
                push!(v);
            }

            HostFn::Fn1_0(fun) => {
                let a0 = pop!();
                fun(self, a0);
                push!(Value::Nil);
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
                push!(Value::Nil);
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
                push!(Value::Nil);
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
                push!(Value::Nil);
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

    /// Call and execute a function in this actor
    pub fn call(&mut self, fun: Value, args: &[Value]) -> Value
    {
        assert!(self.stack.len() == 0);
        assert!(self.frames.len() == 0);

        let fun_id = match fun {
            Value::Closure(clos) => unsafe { (*clos).fun_id },
            Value::Fun(fun_id) => fun_id,
            _ => panic!()
        };

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
            fun,
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
                        panic!(
                            "invalid index in get_arg, idx={}, argc={}, stack depth: {}",
                            idx,
                            argc,
                            self.frames.len()
                        );
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
                        // For now we intentionally don't provide string reference equality
                        // as we may eventually choose to intern *some* strings
                        (Value::String(_), Value::String(_)) => panic!(),
                        (Nil, Nil) => true,
                        (Nil, _) => false,
                        (_, Nil) => false,
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
                        (Nil, Nil) => false,
                        (Nil, _) => true,
                        (_, Nil) => true,
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
                Insn::clos_new { fun_id, num_slots } => {
                    let clos = Closure { fun_id, slots: vec![Undef; num_slots as usize] };
                    let clos_val = self.alloc.alloc(clos);
                    push!(Value::Closure(clos_val));
                }

                // Set a closure slot
                Insn::clos_set { idx } => {
                    let val = pop!();
                    let clos = pop!();

                    match clos {
                        Value::Closure(clos) => {
                            let clos = unsafe { &mut *clos };
                            clos.slots[idx as usize] = val;
                        }
                        _ => panic!()
                    }
                }

                // Get a closure slot for the function currently executing
                Insn::clos_get { idx } => {
                    let fun = &self.frames[self.frames.len() - 1].fun;

                    let val = match fun {
                        Value::Closure(clos) => {
                            let clos = unsafe { &**clos };
                            clos.slots[idx as usize]
                        }
                        _ => panic!()
                    };

                    if val == Value::Undef {
                        panic!("executing uninitialized closure");
                    }

                    push!(val);
                }

                // Create new empty object
                Insn::obj_new => {
                    let new_obj = self.alloc.alloc(Object::default());
                    push!(Value::Object(new_obj))
                }

                /*
                // Copy object
                Insn::obj_copy => {
                    let obj = pop!().unwrap_obj();
                    let new_obj = Object::copy(obj, &mut self.alloc);
                    push!(Value::Object(new_obj))
                }
                */

                // Define constant field
                Insn::obj_def { field } => {
                    let val = pop!();
                    let mut obj = pop!();
                    let field_name = unsafe { &*field };
                    obj.unwrap_obj().def_const(field_name, val);
                }

                // Set object field
                Insn::obj_set { field } => {
                    let val = pop!();
                    let mut obj = pop!();
                    let field_name = unsafe { &*field };
                    obj.unwrap_obj().set(field_name, val);
                }

                // Get object field
                Insn::obj_get { field } => {
                    let mut obj = pop!();
                    let field_name = unsafe { &*field };
                    let val = obj.unwrap_obj().get(field_name);
                    push!(val);
                }

                // Seal object
                Insn::obj_seal => {
                    let mut obj = pop!();
                    obj.unwrap_obj().seal();
                }

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

                // call (arg0, arg1, ..., argN, fun)
                Insn::call { argc } => {
                    // Function to call
                    let fun = pop!();

                    // Argument count
                    if argc as usize > self.stack.len() - bp {
                        panic!();
                    }

                    let fun_id = match fun {
                        Value::Fun(id) => id,
                        Value::Closure(clos) => unsafe { (*clos).fun_id },
                        Value::HostFn(f) => {
                            self.call_host(f, argc.into());
                            continue;
                        }
                        _ => panic!("call with non-function {:?}", fun)
                    };

                    // Get a compiled address for this function
                    let fun_entry = self.get_compiled_fun(fun_id);

                    if argc as usize != fun_entry.num_params {
                        panic!("incorrect argument count");
                    }

                    self.frames.push(StackFrame {
                        argc,
                        fun,
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                    pc = fun_entry.entry_pc;
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
    pub fn new_actor(vm: &Arc<Mutex<VM>>, fun: Value, args: Vec<Value>) -> u64
    {
        let vm_mutex = vm.clone();

        // Assign an actor id
        let mut vm_ref = vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        vm_ref.next_actor_id += 1;
        drop(vm_ref);

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::channel::<Message>();


        // FIXME:
        // We need to recursively copy the function/closure



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
    pub fn call(vm: &mut Arc<Mutex<VM>>, fun_id: FunId, args: Vec<Value>) -> Value
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
        actor.call(Value::Fun(fun_id), &args)
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::parser::parse_str;

    fn eval(s: &str) -> Value
    {
        dbg!(s);
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
    }

    #[test]
    #[should_panic]
    fn assign_const()
    {
        eval("let x = 1; x = 2;");
    }

    #[test]
    fn assert()
    {
        eval("assert(1 != nil);");
        eval("assert(nil == nil);");
        eval("let x = 1; assert(x == 1);");
        eval("let x = 1; assert(x < 2);");
        eval("let var x = 1; x = x + 1; assert(x < 10);");
    }

    #[test]
    fn while_loop()
    {
        eval_eq("let var x = 1; while (x < 10) { x = x + 1; } return x;", Value::Int64(10));
    }

    #[test]
    fn fun_call()
    {
        eval_eq("fun f() { return 7; } return f();", Value::Int64(7));
        eval_eq("fun f(x) { return x + 1; } return f(7);", Value::Int64(8));
        eval_eq("fun f(a, b) { return a - b; } return f(7, 2);", Value::Int64(5));

        // Captured global variable
        eval_eq("let g = 3; fun f() { return g+1; } return f();", Value::Int64(4));

        // Function calling another function
        eval_eq("fun a() { return 8; } fun b() { return a(); } return b();", Value::Int64(8));
    }

    #[test]
    fn fact()
    {
        // Recursive factorial function
        eval_eq("fun f(n) { if (n < 2) return 1; return n * f(n-1); } return f(6);", Value::Int64(720));
    }

    #[test]
    fn fib()
    {
        // Recursive fibonacci function
        eval_eq("fun f(n) { if (n < 2) return n; return f(n-1) + f(n-2); } return f(10);", Value::Int64(55));
    }

    #[test]
    fn ret_clos()
    {
        eval_eq("fun a() { fun b() { return 33; } return b; } let f = a(); return f();", Value::Int64(33));
    }

    #[test]
    fn call_ahead()
    {
        // Call a function before its definition
        eval_eq("fun a() { return b(); } fun b() { return 7; } return a();", Value::Int64(7));
    }

    #[test]
    fn mutual_rec()
    {
        // Mutual recursion
        eval_eq("fun a(n) { return b(n-1); } fun b(n) { if (n<1) return 0; return a(n-1); } return a(8);", Value::Int64(0));
    }

    #[test]
    fn host_call()
    {
        eval_eq("return $actor_id();", Value::Int64(0));
        eval("return $print_str('hi');");
        eval("return $time_current_ms();");
    }

    #[test]
    fn actor_spawn()
    {
        eval_eq(
            concat!(
                "fun f() { return 77; }",
                "let id = $actor_spawn(f);",
                "let ret = $actor_join(id);",
                "return ret;",
            ),
            Value::Int64(77)
        );
    }

    #[test]
    fn actor_send()
    {
        eval_eq(
            concat!(
                "fun f() { return $actor_recv() + 1; }",
                "let id = $actor_spawn(f);",
                "$actor_send(id, 1336);",
                "return $actor_join(id);",
            ),
            Value::Int64(1337)
        );
    }

    #[test]
    fn objects()
    {
        eval("let o = {};");
        eval("let o = { x: 1, y: 2 };");
        eval("let o = { x: 1, f(s) {} };");
        eval_eq("let o = { x: 1, y: 2 }; return o.x;", Value::Int64(1));
        eval_eq("let o = { x: 1, y: 2 }; return o.x + o.y;", Value::Int64(3));

        // Getter method
        eval_eq("let o = { n: 1337, get(s) { return s.n; } }; return o.get();", Value::Int64(1337));

        // Increment method
        eval_eq("let o = { var n: 1, inc(s) { s.n = s.n + 1; } }; o.inc(); return o.n;", Value::Int64(2));
    }
}
