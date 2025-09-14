use std::collections::{HashSet, HashMap};
use std::{thread, thread::sleep};
use std::sync::{Arc, Weak, Mutex, mpsc};
use std::time::Duration;
use crate::lexer::SrcPos;
use crate::ast::{Program, FunId, ClassId, Class};
use crate::alloc::Alloc;
use crate::array::Array;
use crate::bytearray::ByteArray;
use crate::codegen::CompiledFun;
use crate::deepcopy::{deepcopy, remap};
use crate::host::*;

/// Instruction opcodes
/// Note: commonly used upcodes should be in the [0, 127] range (one byte)
///       less frequently used opcodes can take multiple bytes if necessary.
#[allow(non_camel_case_types)]
#[derive(PartialEq, Copy, Clone, Debug)]
pub enum Insn
{
    // Halt execution and produce an error
    panic { pos: SrcPos },

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

    // Global variable access
    get_global { idx: u32 },
    set_global { idx: u32 },

    // Arithmetic
    add,
    sub,
    mul,
    div,
    div_int,
    modulo,

    // Add an int64 constant
    add_i64 { val: i64 },

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
    //is_nil,
    //is_int64,
    //is_object,
    //is_array,

    // Closure operations
    clos_new { fun_id: FunId, num_slots: u32 },
    clos_set { idx: u32 },
    clos_get { idx: u32 },

    // Create class instance
    new { class_id: ClassId, argc: u16 },

    // Check if instance of class
    instanceof { class_id: ClassId },

    // Get/set field
    get_field { field: *const String, class_id: ClassId, slot_idx: u32 },
    set_field { field: *const String, class_id: ClassId, slot_idx: u32 },

    // Get/set indexed element
    get_index,
    set_index,

    // Create a new dictionary
    dict_new,

    // Array operations
    arr_new { capacity: u32 },
    arr_push,

    // Clone a bytearray
    ba_clone,

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

    // Call a known function using its function id
    call_direct { fun_id: FunId, argc: u16 },

    // Call a known function by directly jumping to its entry point
    call_pc { entry_pc: u32, fun_id: FunId, num_locals: u16, argc: u16 },

    // Call a method on an object
    // call_method (self, arg0, ..., argN)
    call_method { name: *const String, argc: u16 },

    // Return
    ret,
}

#[derive(Clone)]
pub struct Closure
{
    pub fun_id: FunId,

    // Captured variable slots
    pub slots: Vec<Value>,
}

#[derive(Clone)]
pub struct Object
{
    pub class_id: ClassId,
    pub slots: Vec<Value>,
}

impl Object
{
    fn new(class_id: ClassId, num_slots: usize) -> Self
    {
        Object {
            class_id,
            slots: vec![Value::Undef; num_slots]
        }
    }
}

#[derive(Clone, Default)]
pub struct Dict
{
    pub hash: HashMap<String, Value>,
}

impl Dict
{
    // Set the value associated with a given field
    fn set(&mut self, field_name: &str, new_val: Value)
    {
        self.hash.insert(field_name.to_string(), new_val);
    }

    // Get the value associated with a given field
    fn get(&mut self, field_name: &str) -> Value
    {
        if let Some(val) = self.hash.get(field_name) {
            *val
        } else {
            panic!();
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum Value
{
    // Undef means uninitialized.
    // This should never be observed in user code.
    Undef,

    Nil,
    False,
    True,
    Int64(i64),
    Float64(f64),

    // String constant
    String(*const String),

    HostFn(HostFn),
    Fun(FunId),
    Closure(*mut Closure),

    // Mutable cell, captured variable
    Cell(*mut Value),

    Object(*mut Object),
    Array(*mut Array),
    ByteArray(*mut ByteArray),
    Dict(*mut Dict),

    Class(ClassId),
}
use Value::{Undef, Nil, False, True, Int64, Float64};

// Allow sending Value between threads
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

impl Value
{
    pub fn is_heap(&self) -> bool
    {
        use Value::*;
        match self {
            // Non-heap values
            Undef       |
            Nil         |
            False       |
            True        |
            Int64(_)    |
            Float64(_)  |
            HostFn(_)   |
            Fun(_)      |
            Class(_)    => false,

            // Heap-allocated values
            String(_)   |
            Closure(_)  |
            Cell(_)     |
            Object(_)   |
            Array(_)    |
            ByteArray(_)|
            Dict(_)     => true,
        }
    }

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

    pub fn unwrap_i32(&self) -> i32
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

    pub fn unwrap_arr(&mut self) -> &mut Array
    {
        match self {
            Value::Array(p) => unsafe { &mut **p },
            _ => panic!("expected array value but got {:?}", self)
        }
    }

    pub fn unwrap_ba(&mut self) -> &mut ByteArray
    {
        match self {
            Value::ByteArray(p) => unsafe { &mut **p },
            _ => panic!("expected byte array value but got {:?}", self)
        }
    }
}

// Implement PartialEq for Value
impl PartialEq for Value
{
    fn eq(&self, other: &Self) -> bool
    {
        use Value::*;

        match (self, other) {
            (Nil, Nil) => true,
            (True, True) => true,
            (False, False) => true,

            // For strings, we do a structural equality comparison, so
            // that some strings can be interned (deduplicated)
            (String(p1), String(p2))    => unsafe { **p1 == **p2 },

            // For int & float, we may need type conversions
            (Float64(a), Int64(b))      => *a == *b as f64,
            (Int64(a), Float64(b))      => *a as f64 == *b,

            // For all other cases, use structural equality
            (Int64(a), Int64(b))        => a == b,
            (Float64(a), Float64(b))    => a == b,
            (HostFn(a), HostFn(b))      => a == b,
            (Fun(a), Fun(b))            => a == b,
            (Closure(a), Closure(b))    => a == b,
            (Object(a), Object(b))      => a == b,
            (Array(a), Array(b))            => a == b,
            (ByteArray(a), ByteArray(b))    => a == b,

            _ => false,
        }
    }
}

// Implement Eq since our equality relation is reflexive
impl Eq for Value {}

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
        Value::Int64(val as i64)
    }
}

impl From<u32> for Value {
    fn from(val: u32) -> Self {
        Value::Int64(val as i64)
    }
}

impl From<i32> for Value {
    fn from(val: i32) -> Self {
        Value::Int64(val as i64)
    }
}

impl From<i64> for Value {
    fn from(val: i64) -> Self {
        Value::Int64(val)
    }
}

impl From<bool> for Value {
    fn from(val: bool) -> Self {
        match val {
            true => Value::True,
            false => Value::False,
        }
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

    // Parent actor id
    pub parent_id: Option<u64>,

    // Parent VM
    pub vm: Arc<Mutex<VM>>,

    // Private allocator
    pub alloc: Alloc,

    // Allocator for incoming messages
    msg_alloc: Arc<Mutex<Alloc>>,

    // Message queue receiver endpoint
    queue_rx: mpsc::Receiver<Message>,

    // Cache of actor ids to message queue endpoints
    actor_map: HashMap<u64, ActorTx>,

    // Global variable slots
    globals: Vec<Value>,

    // Value stack
    stack: Vec<Value>,

    // List of stack frames (activation records)
    frames: Vec<StackFrame>,

    // Map of classes referenced by this actor
    classes: HashMap<ClassId, Class>,

    // Map of compiled functions
    funs: HashMap<FunId, CompiledFun>,

    // Array of compiled instructions
    insns: Vec<Insn>,
}

impl Actor
{
    pub fn new(
        actor_id: u64,
        parent_id: Option<u64>,
        vm: Arc<Mutex<VM>>,
        msg_alloc: Arc<Mutex<Alloc>>,
        queue_rx: mpsc::Receiver<Message>,
        globals: Vec<Value>,
    ) -> Self
    {
        Self {
            actor_id,
            parent_id,
            vm,
            alloc: Alloc::new(),
            msg_alloc,
            queue_rx,
            globals,
            actor_map: HashMap::default(),
            stack: Vec::default(),
            frames: Vec::default(),
            insns: Vec::default(),
            classes: HashMap::default(),
            funs: HashMap::default(),
        }
    }

    /// Receive a message from the message queue
    /// This will block until a message is available
    pub fn recv(&mut self) -> Value
    {
        use crate::window::poll_ui_msg;

        if self.actor_id != 0 {
            let msg = self.queue_rx.recv().unwrap();
            return msg.msg;
        }

        // Actor 0 (the main actor) may need to poll for UI events
        loop {
            // Poll for UI messages
            let ui_msg = poll_ui_msg(self);
            if let Some(msg) = ui_msg {
                return msg;
            }

            // Block on the message queue for up to 8ms
            let msg = self.queue_rx.recv_timeout(Duration::from_millis(8));

            if let Ok(msg) = msg {
                return msg.msg;
            }
        }
    }

    /// Try to receive a message from the message queue
    /// This function will not block if no message is available
    pub fn try_recv(&mut self) -> Option<Value>
    {
        use crate::window::poll_ui_msg;

        if self.actor_id != 0 {
            return match self.queue_rx.try_recv() {
                Ok(msg) => Some(msg.msg),
                _ => None,
            }
        }

        // Actor 0 (the main actor) needs to poll for UI events
        let ui_msg = poll_ui_msg(self);
        if let Some(msg) = ui_msg {
            return Some(msg);
        }

        // Block on the message queue for up to 8ms
        match self.queue_rx.try_recv() {
            Ok(msg) => Some(msg.msg),
            _ => None,
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

        // Copy the message using the receiver's message allocator
        // Note: locking can fail if the receiving thread panics
        let alloc_rc = match actor_tx.msg_alloc.upgrade() {
            Some(rc) => rc,
            None => return Err(()),
        };
        let mut dst_map = HashMap::new();
        let msg = deepcopy(msg, alloc_rc.lock().as_mut().unwrap(), &mut dst_map);
        remap(dst_map);

        match actor_tx.sender.send(Message { sender: self.actor_id, msg }) {
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

    /// Compute something requiring access to a class, lazily
    /// copying the class from the parent VM as needed
    pub fn with_class<F, T>(&mut self, class_id: ClassId, f: F) -> T
    where F: FnOnce(&Class) -> T
    {
        if let Some(class) = self.classes.get(&class_id) {
            return f(class);
        }

        // Borrow the VM and clone the class
        let vm = self.vm.lock().unwrap();
        let class = vm.prog.classes[&class_id].clone();
        drop(vm);

        let ret = f(&class);

        // Save a cached copy of the class to avoid
        // locking if needed again
        self.classes.insert(class_id, class);

        ret
    }

    /// Get the number of slots for a given class
    pub fn get_num_slots(&mut self, class_id: ClassId) -> usize
    {
        self.with_class(class_id, |c| c.fields.len())
    }

    /// Get the slot index for a given field of a given class
    pub fn get_slot_idx(&mut self, class_id: ClassId, field_name: &str) -> usize
    {
        self.with_class(
            class_id, |c| {
                match c.fields.get(field_name) {
                    Some(slot_idx) => *slot_idx,
                    None => panic!("unknown field '{}' in class '{}' (class_id: {:?}). Available fields: {:?}",
                        field_name,
                        c.name,
                        class_id,
                        c.fields.keys().collect::<Vec<_>>())
                }
        })
    }

    // Get the function id for a given method of a given class
    pub fn get_method(&mut self, class_id: ClassId, method_name: &str) -> Option<FunId>
    {
        self.with_class(class_id, |c| c.methods.get(method_name).copied())
    }

    /// Allocate an object of a given class
    /// Note that this won't call the constructor if present
    pub fn alloc_obj(&mut self, class_id: ClassId) -> Value
    {
        let num_slots = self.get_num_slots(class_id);
        let obj = Object::new(class_id, num_slots);
        Value::Object(self.alloc.alloc(obj))
    }

    /// Set the value of an object field
    pub fn set_field(&mut self, obj: Value, field_name: &str, val: Value)
    {
        match obj {
            Value::Object(p) => {
                let obj = unsafe { &mut *p };
                let slot_idx = self.get_slot_idx(obj.class_id, field_name);
                obj.slots[slot_idx] = val;
            },
            _ => panic!()
        }
    }

    /// Allocate/intern a constant string used by the runtime
    /// or present as a constant in the program
    pub fn intern_str(&mut self, str_const: &str) -> Value
    {
        // Note: for now this doesn't do interning but we
        // may choose to add this optimization later
        Value::String(self.alloc.str_const(str_const.to_string()))
    }

    /// Call a host function
    fn call_host(&mut self, host_fn: HostFn, argc: usize)
    {
        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        if host_fn.num_params() != argc {
            panic!(
                "incorrect argument count for host function, got {}, expected {}",
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

            HostFn::Fn5_0(fun) => {
                let a4 = pop!();
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2, a3, a4);
                push!(Value::Nil);
            }

            HostFn::Fn5_1(fun) => {
                let a4 = pop!();
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                let v = fun(self, a0, a1, a2, a3, a4);
                push!(v);
            }

            HostFn::Fn8_0(fun) => {
                let a7 = pop!();
                let a6 = pop!();
                let a5 = pop!();
                let a4 = pop!();
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2, a3, a4, a5, a6, a7);
                push!(Value::Nil);
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
            _ => panic!("expected function argument")
        };

        // Get a compiled address for this function
        let fun_entry = self.get_compiled_fun(fun_id);
        let mut pc = fun_entry.entry_pc;

        if args.len() != fun_entry.num_params {
            panic!("incorrect argument count");
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

        // Allocate stack slots for the local variables
        self.stack.resize(self.stack.len() + fun_entry.num_locals, Value::Nil);

        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        macro_rules! push_bool {
            ($b: expr) => { push!(if $b { True } else { False }) }
        }

        /// Set up a new frame for a function call
        macro_rules! call_fun {
            ($fun: expr, $argc: expr) => {{
                if $argc as usize > self.stack.len() - bp {
                    panic!();
                }

                let fun_id = match $fun {
                    Value::Fun(id) => id,
                    Value::Closure(clos) => unsafe { (*clos).fun_id },
                    Value::HostFn(f) => {
                        self.call_host(f, $argc.into());
                        continue;
                    }
                    _ => panic!("call to non-function {:?}", $fun)
                };

                // Get a compiled address for this function
                let fun_entry = self.get_compiled_fun(fun_id);

                if $argc as usize != fun_entry.num_params {
                    let vm = self.vm.lock().unwrap();
                    let fun = &vm.prog.funs[&fun_id];
                    panic!(
                        "incorrect argument count in call to function \"{}\", defined at {}, received {} arguments, expected {}",
                        fun.name,
                        fun.pos,
                        $argc,
                        fun_entry.num_params
                    );
                }

                self.frames.push(StackFrame {
                    argc: $argc,
                    fun: $fun,
                    prev_bp: bp,
                    ret_addr: pc,
                });

                // The base pointer will point at the first local
                bp = self.stack.len();
                pc = fun_entry.entry_pc;

                // Allocate stack slots for the local variables
                self.stack.resize(self.stack.len() + fun_entry.num_locals, Value::Nil);

                fun_entry
            }}
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

                Insn::panic { pos } => {
                    panic!("panic at: {}", pos);
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

                Insn::get_global { idx } => {
                    let idx = idx as usize;

                    if idx >= self.globals.len() {
                        panic!("invalid index {} in get_global", idx);
                    }

                    let val = self.globals[idx];

                    if val == Value::Undef {
                        panic!("accessing uninitialized global");
                    }

                    push!(val);
                }

                Insn::set_global { idx } => {
                    let idx = idx as usize;
                    let val = pop!();

                    if idx >= self.globals.len() {
                        panic!("invalid index {} in get_global", idx);
                    }

                    self.globals[idx] = val;
                }

                Insn::add => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 + v1),
                        (Float64(v0), Float64(v1)) => Float64(v0 + v1),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 + v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 + v1 as f64),

                        (Value::String(s1), Value::String(s2)) => {
                            let s1 = unsafe { &*s1 };
                            let s2 = unsafe { &*s2 };
                            Value::String(self.alloc.str_const(s1.to_owned() + s2))
                        }

                        _ => panic!("unsupported types in add")
                    };

                    push!(r);
                }

                Insn::sub => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 - v1),
                        (Float64(v0), Float64(v1)) => Float64(v0 - v1),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 - v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 - v1 as f64),
                        _ => panic!("unsupported types in sub")
                    };

                    push!(r);
                }

                Insn::mul => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 * v1),
                        (Float64(v0), Float64(v1)) => Float64(v0 * v1),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 * v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 * v1 as f64),
                        _ => panic!("unsupported types in mul")
                    };

                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::div => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Float64(v0 as f64 / v1 as f64),
                        (Float64(v0), Float64(v1)) => Float64(v0 / v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 / v1 as f64),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 / v1),
                        _ => panic!("div with unsupported types")
                    };

                    push!(r);
                }

                // Integer division
                // Division by zero will cause a panic (this is intentional)
                Insn::div_int => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 / v1),
                        _ => panic!("modulo with non-integer types")
                    };

                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::modulo => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 % v1),
                        _ => panic!("modulo with non-integer types")
                    };

                    push!(r);
                }

                // Add a constant int64 value
                Insn::add_i64 { val } => {
                    if let Some(top_val) = self.stack.last_mut() {
                        match top_val {
                            Int64(v0) => *v0 += val,
                            Float64(v0) => *v0 += val as f64,
                            _ => panic!("unsupported types in add_i64")
                        }
                    } else {
                        panic!();
                    }
                }

                // Integer bitwise or
                Insn::bit_or => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 | v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Integer bitwise and
                Insn::bit_and => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 & v1),
                        _ => panic!("bitwise AND with non-integer values")
                    };

                    push!(r);
                }

                // Integer bitwise XOR
                Insn::bit_xor => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 ^ v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Integer left shift
                Insn::lshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 << v1),
                        _ => panic!()
                    };

                    push!(r);
                }

                // Integer right shift
                Insn::rshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 >> v1),
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
                        (Float64(v0), Float64(v1)) => v0 < v1,
                        (Float64(v0), Int64(v1)) => v0 < (v1 as f64),
                        (Int64(v0), Float64(v1)) => (v0 as f64) < v1,
                        _ => panic!("unsupported types in lt")
                    };

                    push_bool!(b);
                }

                // Less than or equal
                Insn::le => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 <= v1,
                        (Float64(v0), Float64(v1)) => v0 <= v1,
                        (Float64(v0), Int64(v1)) => v0 <= (v1 as f64),
                        (Int64(v0), Float64(v1)) => (v0 as f64) <= v1,
                        _ => panic!("unsupported types in le")
                    };

                    push_bool!(b);
                }

                // Greater than
                Insn::gt => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 > v1,
                        (Float64(v0), Float64(v1)) => v0 > v1,
                        (Float64(v0), Int64(v1)) => v0 > (v1 as f64),
                        (Int64(v0), Float64(v1)) => (v0 as f64) > v1,
                        _ => panic!("unsupported types in lt")
                    };

                    push_bool!(b);
                }

                // Greater than or equal
                Insn::ge => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 >= v1,
                        (Float64(v0), Float64(v1)) => v0 >= v1,
                        (Float64(v0), Int64(v1)) => v0 >= (v1 as f64),
                        (Int64(v0), Float64(v1)) => (v0 as f64) >= v1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                Insn::eq => {
                    let v1 = pop!();
                    let v0 = pop!();
                    push_bool!(v0 == v1);
                }

                Insn::ne => {
                    let v1 = pop!();
                    let v0 = pop!();
                    push_bool!(v0 != v1);
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

                /*
                // Create new empty dictionary
                Insn::dict_new => {
                    let new_obj = self.alloc.alloc(Dict::default());
                    push!(Value::Dict(new_obj))
                }
                */

                // Set object field
                Insn::set_field { field, class_id, slot_idx } => {
                    let val = pop!();
                    let mut obj = pop!();
                    let field_name = unsafe { &*field };

                    match obj {
                        Value::Object(p) => {
                            let obj = unsafe { &mut *p };

                            if class_id == obj.class_id {
                                obj.slots[slot_idx as usize] = val;
                            } else {
                                let slot_idx = self.get_slot_idx(obj.class_id, field_name);
                                let class_id = obj.class_id;

                                // Update the cache
                                self.insns[pc - 1] = Insn::set_field {
                                    field,
                                    class_id,
                                    slot_idx: slot_idx as u32,
                                };

                                obj.slots[slot_idx] = val;
                            }
                        },
                        _ => panic!()
                    }
                }

                // Allocate a new class instance and call
                // the constructor for the given class
                Insn::new { class_id, argc } => {
                    let num_slots = self.get_num_slots(class_id);
                    let obj = Object::new(class_id, num_slots);
                    let obj_val = Value::Object(self.alloc.alloc(obj));

                    let init_fun = self.get_method(class_id, "init");

                    // If a constructor method is present
                    if let Some(fun_id) = init_fun {
                        // The self value should be first on the stack
                        self.stack.insert(self.stack.len() - argc as usize, obj_val);
                        call_fun!(Value::Fun(fun_id), argc + 1);
                    }

                    push!(obj_val);
                }

                Insn::instanceof { class_id } => {
                    // Check that the class id matches
                    let mut val = pop!();
                    let id = crate::runtime::get_class_id(val);
                    push_bool!(id == class_id);
                }

                // Get object field
                Insn::get_field { field, class_id, slot_idx } => {
                    let mut obj = pop!();
                    let field_name = unsafe { &*field };

                    let val = match obj {
                        Value::Array(p) => {
                            match field_name.as_str() {
                                "len" => obj.unwrap_arr().elems.len().into(),
                                _ => panic!()
                            }
                        }

                        Value::ByteArray(p) => {
                            match field_name.as_str() {
                                "len" => obj.unwrap_ba().num_bytes().into(),
                                _ => panic!()
                            }
                        }

                        Value::String(p) => {
                            match field_name.as_str() {
                                "len" => obj.unwrap_rust_str().len().into(),
                                _ => panic!()
                            }
                        }

                        Value::Object(p) => {
                            let obj = unsafe { &*p };

                            // If the class id doesn't match the cache, update it
                            let val = if class_id == obj.class_id {
                                obj.slots[slot_idx as usize]
                            } else {
                                let slot_idx = self.get_slot_idx(obj.class_id, field_name);
                                let class_id = obj.class_id;

                                // Update the cache
                                self.insns[pc - 1] = Insn::get_field {
                                    field,
                                    class_id,
                                    slot_idx: slot_idx as u32,
                                };

                                obj.slots[slot_idx]
                            };

                            if val == Value::Undef {
                                panic!("object field not initialized");
                            };

                            val
                        },

                        _ => panic!("get_field on non-object value {:?}", obj)
                    };

                    push!(val);
                }

                Insn::get_index => {
                    let idx = pop!().unwrap_usize();
                    let mut arr = pop!();

                    let val = match arr {
                        Value::Array(p) => {
                            let arr = unsafe { &mut *p };
                            arr.get(idx)
                        }

                        Value::ByteArray(p) => {
                            let ba = unsafe { &mut *p };
                            Value::from(ba.get(idx))
                        }

                        _ => panic!("expected array type in get_index")
                    };

                    push!(val);
                }

                Insn::set_index => {
                    let val = pop!();
                    let idx = pop!().unwrap_usize();
                    let arr = pop!();

                    match arr {
                        Value::Array(p) => {
                            let arr = unsafe { &mut *p };
                            arr.set(idx, val);
                        }

                        Value::ByteArray(p) => {
                            let ba = unsafe { &mut *p };
                            let b = val.unwrap_u8();
                            ba.set(idx, b);
                        }

                        _ => panic!("expected array type")
                    };
                }

                // Create new empty array
                Insn::arr_new { capacity } => {
                    let new_arr = self.alloc.alloc(Array::with_capacity(capacity));
                    push!(Value::Array(new_arr))
                }

                // Append an element at the end of an array
                Insn::arr_push => {
                    let val = pop!();
                    let mut arr = pop!();
                    arr.unwrap_arr().push(val);
                }

                // Clone a bytearray
                Insn::ba_clone => {
                    let mut val = pop!();
                    let ba = val.unwrap_ba();
                    let p_clone = self.alloc.alloc(ba.clone());
                    push!(Value::ByteArray(p_clone));
                }

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
                    let fun = pop!();
                    call_fun!(fun, argc);
                }

                // call_direct (arg0, arg1, ..., argN)
                Insn::call_direct { fun_id, argc } => {
                    let this_pc = pc - 1;
                    let fun_entry = call_fun!(Value::Fun(fun_id), argc);

                    // Patch the instruction to jump directly to the entry point next time
                    self.insns[this_pc] = Insn::call_pc {
                        entry_pc: fun_entry.entry_pc.try_into().unwrap(),
                        fun_id,
                        num_locals: fun_entry.num_locals.try_into().unwrap(),
                        argc
                    };
                }

                // call_pc (arg0, arg1, ..., argN)
                Insn::call_pc { entry_pc, fun_id, num_locals, argc } => {
                    self.frames.push(StackFrame {
                        argc,
                        fun: Value::Fun(fun_id),
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                    pc = entry_pc as usize;

                    // Allocate stack slots for the local variables
                    self.stack.resize(self.stack.len() + num_locals as usize, Value::Nil);
                }

                // Call a method with a known name
                // call_method (self, arg0, ..., argN)
                Insn::call_method { name, argc } => {
                    let method_name = unsafe { &*name };
                    let self_val = self.stack[self.stack.len() - (1 + argc as usize)];

                    let fun = match self_val {
                        Value::Object(p) => {
                            let obj = unsafe { &*p };
                            let fun_id = self.get_method(obj.class_id, &method_name).unwrap();
                            Value::Fun(fun_id)
                        }

                        _ => {
                            crate::runtime::get_method(self_val, &method_name)
                        }
                    };

                    call_fun!(fun, argc + 1);
                }

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

#[derive(Clone)]
struct ActorTx
{
    sender: mpsc::Sender<Message>,
    msg_alloc: Weak<Mutex<Alloc>>,
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
    actor_txs: HashMap<u64, ActorTx>,

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
    pub fn new_actor(parent: &mut Actor, fun: Value, args: Vec<Value>) -> u64
    {
        // Assign an actor id
        let mut vm_ref = parent.vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        let parent_id = parent.actor_id;
        vm_ref.next_actor_id += 1;
        drop(vm_ref);

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::channel::<Message>();

        // Create an allocator to send messages to the actor
        let mut msg_alloc = Alloc::new();

        // Hash map for remapping copied values
        let mut dst_map = HashMap::new();

        // We need to recursively copy the function/closure
        // using the actor's message allocator
        let fun = deepcopy(fun, &mut msg_alloc, &mut dst_map);

        // Copy the global variables from the parent actor
        let mut globals = parent.globals.clone();
        for val in &mut globals {
            *val = deepcopy(*val, &mut msg_alloc, &mut dst_map);
        }

        remap(dst_map);

        // Wrap the message allocator in a shared mutex
        let msg_alloc = Arc::new(Mutex::new(msg_alloc));

        // Info needed to send the actor a message
        let actor_tx = ActorTx {
            sender: queue_tx,
            msg_alloc: Arc::downgrade(&msg_alloc),
        };

        // Spawn a new thread for the actor
        let vm_mutex = parent.vm.clone();
        let handle = thread::spawn(move || {
            let mut actor = Actor::new(
                actor_id,
                Some(parent_id),
                vm_mutex,
                msg_alloc,
                queue_rx,
                globals,
            );
            actor.call(fun, &args)
        });

        // Store the join handles and queue endpoints on the VM
        let mut vm_ref = parent.vm.lock().unwrap();
        vm_ref.threads.insert(actor_id, handle);
        vm_ref.actor_txs.insert(actor_id, actor_tx);
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

        // Create an allocator to send messages to the actor
        let msg_alloc = Arc::new(Mutex::new(Alloc::new()));

        // Info needed to send the actor a message
        let actor_tx = ActorTx {
            sender: queue_tx,
            msg_alloc: Arc::downgrade(&msg_alloc),
        };

        // Assign an actor id
        // Store the queue endpoints on the VM
        let mut vm_ref = vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        assert!(actor_id == 0);
        vm_ref.next_actor_id += 1;

        // Store the queue endpoint and message allocator on the VM
        vm_ref.actor_txs.insert(actor_id, actor_tx);

        // Initialize the global slots
        let globals = vec![Value::Undef; vm_ref.prog.num_globals];

        drop(vm_ref);

        let mut actor = Actor::new(
            actor_id,
            None,
            vm_mutex,
            msg_alloc,
            queue_rx,
            globals,
        );

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
    fn insn_size()
    {
        use std::mem::size_of;
        dbg!(size_of::<Insn>());
        assert!(size_of::<Insn>() <= 24);

        dbg!(size_of::<ClassId>());
        assert!(size_of::<ClassId>() <= 32);
    }

    #[test]
    fn vm_new()
    {
        let prog = Program::new();
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
        eval_eq("return 2 * 2 - 1;", Value::Int64(3));
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
    fn logical_and()
    {
        eval_eq("if (false && false) return 1; else return 0;", Value::Int64(0));
        eval_eq("if (false && true) return 1; else return 0;", Value::Int64(0));
        eval_eq("if (true && false) return 1; else return 0;", Value::Int64(0));
        eval_eq("if (true && true) return 1; else return 0;", Value::Int64(1));
    }

    #[test]
    fn logical_or()
    {
        eval_eq("if (false || false) return 1; else return 0;", Value::Int64(0));
        eval_eq("if (false || true) return 1; else return 0;", Value::Int64(1));
        eval_eq("if (true || false) return 1; else return 0;", Value::Int64(1));
        eval_eq("if (true || true) return 1; else return 0;", Value::Int64(1));
    }

    #[test]
    fn let_expr()
    {
        eval_eq("let x = 1; return x;", Value::Int64(1));
        eval_eq("let var x = 1; return x;", Value::Int64(1));
        eval_eq("let x = 1; let y = 2; return x + y;", Value::Int64(3));
    }

    #[test]
    fn inc_dec()
    {
        eval_eq("let var x = 10; --x; return x;", Value::Int64(9));
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
    fn comparisons()
    {
        eval_eq("class F {} let o1 = F(); let o2 = F(); return o1 == o2;", Value::False);
        eval_eq("class F {} let o1 = F(); let o2 = F(); return o1 != o2;", Value::True);

        // Integer comparisons
        eval_eq("return 3 <= 5;", Value::True);

        // String comparison
        eval_eq("return 'foo' == 'bar';", Value::False);
        eval_eq("return 'foo' == 'foo';", Value::True);
        eval_eq("return 'foo' != 'foo';", Value::False);
    }

    #[test]
    fn ternary_expr()
    {
        eval_eq("return true? 1:2;", Value::Int64(1));
        eval_eq("return false? 1:2;", Value::Int64(2));
        eval_eq("let b = (1 < 5)? 1:2; return b;", Value::Int64(1));
    }

    #[test]
    fn scope_shadow()
    {
        eval("let x = 1; { let x = x + 1; assert(x==2); } assert(x==1);");
    }

    #[test]
    fn while_loop()
    {
        eval_eq("let x = 1; while (false) {} return x;", Value::Int64(1));
        eval_eq("let var x = 1; while (x < 10) { x = x + 1; } return x;", Value::Int64(10));
    }

    #[test]
    fn for_loop()
    {
        eval("for (;;) break;");
        eval_eq("let x = 1; for (let var x = 0; x < 10; ++x) {} return x;", Value::Int64(1));
        eval_eq("let var x = 0; for (let var i = 0; i < 10; ++i) { x = x + 2; } return x;", Value::Int64(20));
        eval_eq("let var x = 0; for (let var i = 0; i < 10; ++i) { ++x; assert(x < 11); continue; } return x;", Value::Int64(10));
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
    fn ret_clos()
    {
        eval_eq("fun a() { fun b() { return 33; } return b; } let f = a(); return f();", Value::Int64(33));
    }

    #[test]
    fn capture_local()
    {
        // Captured function argument
        eval_eq("fun f(n) { return || n+1; } let g = f(7); return g();", Value::Int64(8));

        // Capture local variable
        eval_eq("fun f(n) { let m = n+1; return || m+1; } let g = f(3); return g();", Value::Int64(5));
        eval_eq("fun f(n) { let m = n+1; return |x| m+x; } let g = f(3); return g(4);", Value::Int64(8));
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
        eval_eq("return $actor_parent();", Value::Nil);
        eval("return $print('hi');");
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
    fn actor_reads_global()
    {
        eval_eq(
            concat!(
                "let g = 33;",
                "fun f() { return g; }",
                "let id = $actor_spawn(f);",
                "return $actor_join(id);",
            ),
            Value::Int64(33)
        );
    }

    #[test]
    fn actor_copy_obj()
    {
        // g and g2 should point to the same object after
        // globals are copied for the new actor
        eval_eq(
            concat!(
                "class F {}",
                "let g = F();",
                "let g2 = g;",
                "fun f() { return g == g2; }",
                "let id = $actor_spawn(f);",
                "return $actor_join(id);",
            ),
            Value::True
        );
    }

    #[test]
    fn int64()
    {
        eval("let v = 15; assert(v.to_s() == '15');");
    }

    #[test]
    fn float64()
    {
        eval_eq("return 77.0 instanceof Float64;", Value::True);
        eval_eq("return 4.0 + 1.0;", Value::Float64(5.0));
        eval_eq("return 6.0 / 2.0;", Value::Float64(3.0));
        eval_eq("return 4.0.sqrt();", Value::Float64(2.0));
    }

    #[test]
    fn strings()
    {
        eval_eq("return ''.len;", Value::Int64(0));
        eval_eq("return 'hello'.len;", Value::Int64(5));
        eval_eq("let s1 = 'foo'; let s2 = 'bar'; return s1 + s2 == 'foobar';", Value::True);
    }

    /*
    #[test]
    fn dicts()
    {
        eval("let o = {};");
        eval("let o = { x: 1, y: 2 };");
        eval_eq("let o = { x: 1, y: 2 }; return o.x;", Value::Int64(1));
        eval_eq("let o = { x: 1, y: 2 }; return o.x + o.y;", Value::Int64(3));
    }
    */

    #[test]
    fn arrays()
    {
        eval("let a = [];");
        eval("let a = [1, 2, 3];");
        eval_eq("let a = [11, 22, 33]; return a[0];", Value::Int64(11));
        eval_eq("let a = [11, 22, 33]; return a[2];", Value::Int64(33));
        eval_eq("let a = [11, 22, 33]; a[2] = 44; return a[2];", Value::Int64(44));
        eval_eq("let a = [11, 22, 33]; return a.len;", Value::Int64(3));
        eval_eq("let a = [11, 22, 33]; a.push(44); return a.len;", Value::Int64(4));
        eval_eq("let a = Array.with_size(5, nil); return a.len;", Value::Int64(5));
    }

    #[test]
    fn bytearray()
    {
        eval("let a = ByteArray.new();");
        eval("let a = ByteArray.with_size(1024); assert(a.len == 1024);");
        eval("let a = ByteArray.with_size(32); a.write_u32(0, 0xFF_FF_FF_FF);");
        eval("let a = ByteArray.with_size(32); a.write_u32(0, 0xFF_00_00_00); assert(a[0] == 0 && a[3] == 255);");
        eval("let a = ByteArray.with_size(32); a[11] = 77; assert(a[11] == 77);");
    }

    #[test]
    fn classes()
    {
        eval("class Foo {}");
        eval("class Foo { init(self) {} }");
        eval("class Foo { init(self) { self.x = 1; } }");

        eval("class Foo {} let o = Foo();");
        eval("class Foo { init(s) {} } let o = Foo();");
        eval("class Foo { init(s, a) {} } let o = Foo(1);");

        eval("class Foo { init(s) { s.x = 1; } } let o = Foo();");
        eval("class Foo { init(s, a) { s.x = a; } } let o = Foo(7);");

        eval_eq("class Foo {} return Foo() != nil;", Value::True);
        eval_eq("class Foo { init(s) {} } return Foo() != nil;", Value::True);

        eval_eq("class Foo { init(s) { s.x = 1; } } let o = Foo(); return o.x;", Value::Int64(1));
        eval_eq("class Foo { init(s, a) { s.x = a; } } let o = Foo(7); return o.x;", Value::Int64(7));
        eval_eq("class Foo { init(s, a, b) { s.x = a; s.y = b; } } let o = Foo(5, 3); return o.x - o.y;", Value::Int64(2));
        eval_eq("class C { init(s) { s.c = 0; } inc(s) { ++s.c; } } let o = C(); o.inc(); return o.c;", Value::Int64(1));
    }

    #[test]
    fn instanceof()
    {
        eval_eq("class F {} return nil instanceof F;", Value::False);
        eval_eq("class F {} let o = F(); return o instanceof F;", Value::True);
        eval_eq("class F {} class G {} let o = F(); return o instanceof G;", Value::False);
        eval_eq("class F {} return F() instanceof F;", Value::True);

        // Basic runtime classes
        eval_eq("return 5 instanceof Int64;", Value::True);
        eval_eq("return 77 instanceof String;", Value::False);
        eval_eq("return 'foo' instanceof String;", Value::True);
        eval_eq("return [] instanceof Array;", Value::True);
    }
}
