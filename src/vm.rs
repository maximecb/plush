use std::collections::{HashSet, HashMap};
use std::{thread, thread::sleep};
use std::sync::{Arc, Weak, Mutex, mpsc};
use std::time::Duration;
use crate::dict::Dict;
use crate::utils::thousands_sep;
use crate::lexer::SrcPos;
use crate::ast::{Program, FunId, ClassId, Class};
use crate::alloc::Alloc;
use crate::object::Object;
use crate::closure::Closure;
use crate::array::Array;
use crate::bytearray::ByteArray;
use crate::codegen::CompiledFun;
use crate::deepcopy::{deepcopy, remap};
use crate::host::*;
use crate::str::Str;

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

    // Mutable cell operations
    cell_new,
    cell_set,
    cell_get,

    // Create class instance
    new { class_id: ClassId, argc: u8 },

    // Create a class instance with a known number of slots and constructor
    new_known_ctor { class_id: ClassId, argc: u8, num_slots: u16, ctor_pc: u32, fun_id: FunId, num_locals: u16 },

    // Check if instance of class
    instanceof { class_id: ClassId },

    // Get/set field
    get_field { field: *const Str, class_id: ClassId, slot_idx: u32 },
    set_field { field: *const Str, class_id: ClassId, slot_idx: u32 },

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
    //call_host { host_fn: HostFn, argc: u8 },

    // Call a function using the call stack
    // call (arg0, arg1, ..., argN)
    call { argc: u8 },

    // Call a known function using its function id
    call_direct { fun_id: FunId, argc: u8 },

    // Call a known function by directly jumping to its entry point
    call_pc { entry_pc: u32, fun_id: FunId, num_locals: u16, argc: u8 },

    // Call a method on an object
    // call_method (self, arg0, ..., argN)
    call_method { name: *const Str, argc: u8 },

    // Call a method with a previously known pc
    call_method_pc { name: *const Str, argc: u8, class_id: ClassId, entry_pc: u32, fun_id: FunId, num_locals: u16 },

    // Return
    ret,
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

    // Immutable string
    String(*const Str),

    HostFn(&'static HostFn),
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

    pub fn unwrap_obj(&mut self) -> &mut Object
    {
        match self {
            Value::Object(p) => unsafe { &mut **p },
            _ => panic!("expected object value but got {:?}", self)
        }
    }

    pub fn unwrap_clos(&mut self) -> &mut Closure
    {
        match self {
            Value::Closure(p) => unsafe { &mut **p },
            _ => panic!("expected closure value but got {:?}", self)
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

    pub fn unwrap_dict(&mut self) -> &mut Dict
    {
        match self {
            Value::Dict(p) => unsafe { &mut **p },
            _ => panic!("expected dict value but got {:?}", self)
        }
    }

    pub fn unwrap_str(&mut self) -> &Str
    {
        match self {
            Value::String(p) => unsafe { &**p },
            _ => panic!("expected dict value but got {:?}", self)
        }
    }
}

// This error macro is to be used inside host functions
#[macro_export]
macro_rules! error {
    ($requester: literal, $format_str:literal $(, $arg:expr)* $(,)?) => {{
        return Err(
            format!($format_str $(, $arg)*)
        );
    }}
}

#[macro_export]
macro_rules! unwrap_i64 {
    // To be used inside the interpreter loop
    ($val: expr, $requester: literal) => {
        match $val {
            Value::Int64(v) => v,
            _ => error!($requester, "expected int64 value but got {:?}", $val)
        }
    };

    // To be used by host functions
    ($val: expr) => {
        unwrap_i64!($val, "")
    }
}

#[macro_export]
macro_rules! unwrap_usize {
    // To be used inside the interpreter loop
    ($val: expr, $requester: literal) => {
        match $val {
            Value::Int64(v) => {
                if (v < 0) {
                    error!($requester, "expected non-negative integer but got {:?}", v)
                }
                v as usize
            },
            _ => error!($requester, "expected int64 value but got {:?}", $val)
        }
    };

    // To be used by host functions
    ($val: expr) => {
        unwrap_usize!($val, "")
    }
}

#[macro_export]
macro_rules! unwrap_str {
    // To be used inside the interpreter loop
    ($val: expr, $requester: literal) => {
        match $val {
            Value::String(p) => unsafe { (*p).as_str() },
            _ => error!($requester, "expected string value but got {:?}", $val)
        }
    };

    // To be used by host functions
    ($val: expr) => {
        unwrap_str!($val, "")
    }
}

// Implement PartialEq for Value
impl PartialEq for Value
{
    fn eq(&self, other: &Self) -> bool
    {
        use Value::*;

        match (self, other) {
            (Undef, Undef) => true,
            (Nil, Nil) => true,
            (True, True) => true,
            (False, False) => true,

            // For strings, we do a structural equality comparison, so
            // that some strings can be interned (deduplicated)
            (String(p1), String(p2))    => p1 == p2 || unsafe { (**p1).as_str() == (**p2).as_str() },

            // For int & float, we may need type conversions
            (Float64(a), Int64(b))      => *a == *b as f64,
            (Int64(a), Float64(b))      => *a as f64 == *b,

            // For all other cases, use structural equality
            (Int64(a), Int64(b))        => a == b,
            (Float64(a), Float64(b))    => a == b,
            (HostFn(a), HostFn(b))      => *a as *const crate::host::HostFn == *b as *const crate::host::HostFn,
            (Fun(a), Fun(b))            => a == b,
            (Closure(a), Closure(b))    => a == b,
            (Object(a), Object(b))      => a == b,
            (Array(a), Array(b))            => a == b,
            (ByteArray(a), ByteArray(b))    => a == b,
            (Dict(a), Dict(b))          => a == b,

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

impl From<f64> for Value {
    fn from(val: f64) -> Self {
        Value::Float64(val)
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
    argc: u8,

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
    pub msg_alloc: Arc<Mutex<Alloc>>,

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

        // Call try_recv first to give the message allocator GC
        // a chance to run before we block and wait for a message
        if let Some(msg) = self.try_recv() {
            return msg;
        }

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

        // Lock on the message allocator
        // Senders cannot send us messages while we hold the lock
        // If we can get the lock, it also means senders are done
        let alloc_rc = self.msg_alloc.clone();
        let mut msg_alloc = alloc_rc.lock().unwrap();

        // Actor 0 (the main actor) needs to poll for UI events
        if self.actor_id == 0 {
            let ui_msg = poll_ui_msg(self);
            if let Some(msg) = ui_msg {
                return Some(msg);
            }
        }

        // Block on the message queue for up to 8ms
        if let Ok(msg) = self.queue_rx.try_recv() {
            return Some(msg.msg);
        }

        // If the message allocator is full
        if msg_alloc.bytes_free() < msg_alloc.mem_size() / 4 {
            // Perform a GC pass to copy messages into the main allocator
            self.gc_collect(0, &mut []);

            println!("Performing message allocator GC");

            // Clear the contents of the message allocator
            *msg_alloc = Alloc::with_size(msg_alloc.mem_size());
        }

        // No message received
        None
    }

    /// Send a message to another actor
    pub fn send(&mut self, actor_id: u64, msg: Value) -> Result<(), ()>
    {
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
        let msg = deepcopy(msg, alloc_rc.lock().as_mut().unwrap(), &mut dst_map).unwrap();
        remap(&mut dst_map);

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

        let class = vm.prog.classes.get(&class_id);

        if class.is_none() {
            panic!("could not find class with id={:?}", class_id);
        }

        let class = class.unwrap().clone();
        drop(vm);

        let ret = f(&class);

        // Save a cached copy of the class to avoid
        // locking if needed again
        self.classes.insert(class_id, class);

        ret
    }

    /// Get the class name for a given class
    pub fn get_class_name(&mut self, class_id: ClassId) -> String
    {
        self.with_class(class_id, |c| c.name.clone())
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
                        c.fields.keys().collect::<Vec<_>>()
                    )
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

        self.gc_check(
            size_of::<Object>() + size_of::<Value>() * num_slots,
            &mut []
        );

        Object::new(class_id, num_slots, &mut self.alloc).unwrap()
    }

    /// Set the value of an object field
    pub fn set_field(&mut self, obj: Value, field_name: &str, val: Value)
    {
        match obj {
            Value::Object(p) => {
                let obj = unsafe { &mut *p };
                let slot_idx = self.get_slot_idx(obj.class_id, field_name);
                obj.set(slot_idx, val);
            },
            _ => panic!()
        }
    }

    /// Allocate/intern a constant string used by the runtime
    /// or present as a constant in the program
    pub fn intern_str(&mut self, str_const: &str) -> Value
    {
        self.gc_check(
            size_of::<Str>() + str_const.len(),
            &mut []
        );

        // Note: for now this doesn't do interning but we
        // may choose to add this optimization later
        self.alloc.str_val(str_const).unwrap()
    }

    /// Perform a garbage collection cycle
    pub fn gc_collect(&mut self, bytes_needed: usize, extra_roots: &mut [&mut Value])
    {
        fn try_copy(
            actor: &mut Actor,
            dst_alloc: &mut Alloc,
            dst_map: &mut HashMap<Value, Value>,
            extra_roots: &mut [&mut Value],
        ) -> Result<(), ()>
        {
            // Copy the global variables
            for val in &mut actor.globals {
                deepcopy(*val, dst_alloc, dst_map)?;
            }

            // Copy values on the stack
            for val in &mut actor.stack {
                deepcopy(*val, dst_alloc, dst_map)?;
            }

            // Copy closures in the stack frames
            for frame in &mut actor.frames {
                deepcopy(frame.fun, dst_alloc, dst_map)?;
            }

            // Copy heap values referenced in instructions
            for insn in &mut actor.insns {
                match insn {
                    Insn::push { val } => {
                        deepcopy(*val, dst_alloc, dst_map)?;
                    }

                    // Instructions referencing name strings
                    Insn::get_field { field: s, .. } |
                    Insn::set_field { field: s, .. } |
                    Insn::call_method { name: s, .. } |
                    Insn::call_method_pc { name: s, .. } => {
                        deepcopy(Value::String(*s), dst_alloc, dst_map)?;
                    }

                    _ => {}
                }
            }

            // Copy extra roots supplied by the user
            for val in extra_roots {
                deepcopy(**val, dst_alloc, dst_map)?;
            }

            println!(
                "GC copied {} values, {} bytes free",
                thousands_sep(dst_map.len()),
                thousands_sep(dst_alloc.bytes_free()),
            );

            remap(dst_map);

            Ok(())
        }

        fn get_new_val(val: Value, dst_map: &HashMap<Value, Value>) -> Value
        {
            if !val.is_heap() {
                return val;
            }

            let new_val = *dst_map.get(&val).unwrap();
            new_val
        }

        println!("Running GC cycle, {} bytes free", self.alloc.bytes_free());
        let start_time = crate::host::get_time_ms();

        let mut new_mem_size = self.alloc.mem_size();

        // Create a new allocator to copy the data into
        let mut dst_alloc = Alloc::with_size(new_mem_size);

        // Hash map for remapping copied values
        let mut dst_map = HashMap::<Value, Value>::new();

        loop {
            // Clear the value map
            dst_map.clear();

            // Try to copy all objects into the new allocator
            let copy_fail = try_copy(self, &mut dst_alloc, &mut dst_map, extra_roots).is_err();

            // If there is not enough free memory after copying
            let min_free_bytes = std::cmp::max(self.alloc.mem_size() / 5, bytes_needed);
            let bytes_free = dst_alloc.bytes_free();
            let not_enough_space = bytes_free < min_free_bytes;

            // If we could not copy all the data or there is not enough free space
            // Increase the target heap size
            if copy_fail || not_enough_space {
                new_mem_size = std::cmp::max(
                    (new_mem_size * 3) / 2,
                    new_mem_size + bytes_needed,
                );

                println!(
                    "Increasing heap size to {} bytes",
                    thousands_sep(new_mem_size),
                );

                // Recreate the target allocator
                dst_alloc = Alloc::with_size(new_mem_size);

                // Try again
                continue;
            }

            // Copying successful
            break;
        }

        // Remap the global variables
        for val in &mut self.globals {
            *val = get_new_val(*val, &dst_map);
        }

        // Remap values on the stack
        for val in &mut self.stack {
            *val = get_new_val(*val, &dst_map);
        }

        // Remap closures in the stack frames
        for frame in &mut self.frames {
            frame.fun = get_new_val(frame.fun, &dst_map);
        }

        // Remap heap values referenced in instructions
        for insn in &mut self.insns {
            match insn {
                Insn::push { val } => {
                    *val = get_new_val(*val, &dst_map);
                }

                // Instructions referencing name strings
                Insn::get_field { field: s, .. } |
                Insn::set_field { field: s, .. } |
                Insn::call_method { name: s, .. } |
                Insn::call_method_pc { name: s, .. } => {
                    match get_new_val(Value::String(*s), &dst_map) {
                        Value::String(new_s) => *s = new_s,
                        _ => panic!(),
                    }
                }

                _ => {}
            }
        }

        // Remap extra roots supplied by the user
        for val in extra_roots {
            **val = get_new_val(**val, &dst_map);
        }

        // Drop and replace the old allocator
        // Note that we can only do this after remapping the values,
        // because we access string data while hashing string values
        self.alloc = dst_alloc;

        let end_time = crate::host::get_time_ms();
        let gc_time = end_time - start_time;
        println!("GC time: {} ms", gc_time);
    }

    /// Ensure that at least bytes_needed of free space are available in the
    /// allocator. If the memory is not available, perform GC.
    pub fn gc_check(&mut self, bytes_needed: usize, extra_roots: &mut [&mut Value])
    {
        // Add some extra bytes for alignment
        let bytes_needed = bytes_needed + 16;

        if self.alloc.bytes_free() >= bytes_needed {
            return;
        }

        self.gc_collect(bytes_needed, extra_roots);
    }

    /// Call a host function
    fn call_host(&mut self, host_fn: &HostFn, argc: usize) -> Result<(), String>
    {
        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        if host_fn.num_params() != argc {
            return Err(format!(
                "incorrect argument count for host function `{}`, got {}, expected {}",
                host_fn.name,
                argc,
                host_fn.num_params()
            ));
        }

        let result = match host_fn.f
        {
            FnPtr::Fn0(fun) => {
                fun(self)
            }

            FnPtr::Fn1(fun) => {
                let a0 = pop!();
                fun(self, a0)
            }

            FnPtr::Fn2(fun) => {
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1)
            }

            FnPtr::Fn3(fun) => {
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2)
            }

            FnPtr::Fn4(fun) => {
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2, a3)
            }

            FnPtr::Fn5(fun) => {
                let a4 = pop!();
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2, a3, a4)
            }

            FnPtr::Fn8(fun) => {
                let a7 = pop!();
                let a6 = pop!();
                let a5 = pop!();
                let a4 = pop!();
                let a3 = pop!();
                let a2 = pop!();
                let a1 = pop!();
                let a0 = pop!();
                fun(self, a0, a1, a2, a3, a4, a5, a6, a7)
            }
        };

        match result {
            Ok(v) => { push!(v); Ok(()) },
            Err(e) => Err(format!("error during call to host function `{}`:\n{}", host_fn.name, e)),
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
            _ => panic!("invalid function passed to Actor::call")
        };

        // Get a compiled address for this function
        let fun_entry = self.get_compiled_fun(fun_id);
        let mut pc = fun_entry.entry_pc;

        if args.len() != fun_entry.num_params {
            panic!("incorrect argument count for function passed to Actor::call");
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

        // Set up a new frame for a function call
        macro_rules! call_fun {
            ($fun: expr, $argc: expr) => {{
                if $argc as usize > self.stack.len() - bp {
                    error!("not enough call arguments on stack");
                }

                let fun_id = match $fun {
                    Value::Fun(id) => id,
                    Value::Closure(clos) => unsafe { (*clos).fun_id },
                    Value::HostFn(f) => {
                        match self.call_host(f, $argc.into()) {
                            Err(msg) => error!("{}", msg),
                            Ok(ret_val) => continue
                        }
                        //continue;
                    }
                    _ => error!("call to non-function value: `{:?}`", $fun)
                };

                // Get a compiled address for this function
                let fun_entry = self.get_compiled_fun(fun_id);

                if $argc as usize != fun_entry.num_params {
                    let vm = self.vm.lock().unwrap();
                    let fun = &vm.prog.funs[&fun_id];
                    error!(
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

        // Handle a runtime error
        // Print debug information including a stack trace
        // and terminate the execution
        macro_rules! error {
            ($insn_name: literal, $format_str:literal $(, $arg:expr)* $(,)?) => {{
                eprintln!();

                if $insn_name != "" {
                    eprintln!("Runtime error while executing `{}` instruction:", $insn_name);
                }

                // Print the error message to standard error
                eprintln!($format_str $(, $arg)*);
                eprintln!();

                // For each stack frame, from top to bottom
                for frame in self.frames.clone().into_iter().rev() {
                    let fun_id = match frame.fun {
                        Value::Fun(id) => id,
                        Value::Closure(clos) => unsafe { (*clos).fun_id },
                        _ => panic!("non-function on stack")
                    };

                    // Get the name of the function and its source position
                    let vm = self.vm.lock().unwrap();
                    let fun = &vm.prog.funs[&fun_id];
                    let fun_name = fun.name.clone();
                    let fun_pos = fun.pos;
                    let fun_class_id = fun.class_id;

                    // If this is a method, prepend the class name
                    let fun_name = if fun_class_id != ClassId::default() {
                        let class_name = &vm.prog.classes[&fun_class_id].name;
                        format!("{}.{}", class_name, fun_name)
                    } else {
                        fun_name
                    };

                    eprintln!("{}", fun_name);
                    eprintln!("  defined at {}", fun_pos);
                }

                // End program execution
                panic!();
            }};

            ($format_str:literal $(, $arg:expr)* $(,)?) => {
                error!("", $format_str $(, $arg)*)
            };
        }

        loop
        {
            if pc >= self.insns.len() {
                error!("pc out of bounds");
            }

            let insn = self.insns[pc];
            pc += 1;
            //println!("executing {:?}", insn);
            //println!("stack size: {}, executing {:?}", self.stack.len(), insn);

            match insn {
                Insn::nop => {},

                Insn::panic { pos } => {
                    error!("explicit panic at: {}", pos);
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
                        error!(
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
                        error!("invalid index {} in get_local", idx);
                    }

                    push!(self.stack[bp + idx]);
                }

                Insn::set_local { idx } => {
                    let idx = idx as usize;
                    let val = pop!();

                    if bp + idx >= self.stack.len() {
                        error!("invalid index in set_local");
                    }

                    self.stack[bp + idx] = val;
                }

                Insn::get_global { idx } => {
                    let idx = idx as usize;

                    if idx >= self.globals.len() {
                        error!("get_global", "invalid global index {}", idx);
                    }

                    let val = self.globals[idx];

                    if val == Value::Undef {
                        error!("get_global", "attempting to read uninitialized global");
                    }

                    push!(val);
                }

                Insn::set_global { idx } => {
                    let idx = idx as usize;
                    let val = pop!();

                    if idx >= self.globals.len() {
                        error!("set_global", "invalid global index {}", idx);
                    }

                    self.globals[idx] = val;
                }

                Insn::add => {
                    let mut v1 = pop!();
                    let mut v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 + v1),
                        (Float64(v0), Float64(v1)) => Float64(v0 + v1),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 + v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 + v1 as f64),

                        (Value::String(s0), Value::String(s1)) => {
                            let s0 = unsafe { &*s0 };
                            let s1 = unsafe { &*s1 };

                            self.gc_check(
                                std::mem::size_of::<Str>() +
                                s0.len() + s1.len(),
                                &mut [&mut v0, &mut v1],
                            );

                            let s0 = unwrap_str!(v0);
                            let s1 = unwrap_str!(v1);
                            self.alloc.str_val(&(s0.to_owned() + s1)).unwrap()
                        }

                        _ => error!("add", "unsupported operand types")
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
                        _ => error!("sub", "unsupported operand types")
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
                        _ => error!("mul", "unsupported operand types")
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
                        _ => error!("div", "unsupported operand types")
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
                        _ => error!("div_int", "integer division with non-integer types")
                    };

                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::modulo => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 % v1),
                        (Float64(v0), Float64(v1)) => Float64(v0 % v1),
                        (Float64(v0), Int64(v1)) => Float64(v0 % v1 as f64),
                        (Int64(v0), Float64(v1)) => Float64(v0 as f64 % v1),
                        _ => error!("modulo", "modulo with unsupported types")
                    };

                    push!(r);
                }

                // Add a constant int64 value
                Insn::add_i64 { val } => {
                    if let Some(top_val) = self.stack.last_mut() {
                        match top_val {
                            Int64(v0) => *v0 += val,
                            Float64(v0) => *v0 += val as f64,
                            _ => error!("add_i64", "unsupported operand type")
                        }
                    } else {
                        error!("add_i64", "stack is empty");
                    }
                }

                // Integer bitwise or
                Insn::bit_or => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 | v1),
                        _ => error!("bit_or", "unsupported operand types")
                    };

                    push!(r);
                }

                // Integer bitwise and
                Insn::bit_and => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 & v1),
                        _ => error!("bit_and", "bitwise AND with non-integer values")
                    };

                    push!(r);
                }

                // Integer bitwise XOR
                Insn::bit_xor => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 ^ v1),
                        _ => error!("bit_xor", "unsupported operand types")
                    };

                    push!(r);
                }

                // Integer left shift
                Insn::lshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 << v1),
                        _ => error!("lshift", "unsupported operand types")
                    };

                    push!(r);
                }

                // Integer right shift
                Insn::rshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let r = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => Int64(v0 >> v1),
                        _ => error!("rshift", "unsupported operand types")
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

                        (Value::String(s1), Value::String(s2)) => {
                            let s1 = unsafe { (*s1).as_str() };
                            let s2 = unsafe { (*s2).as_str() };
                            s1 < s2
                        }

                        _ => error!("lt", "unsupported types in less-than")
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

                        (Value::String(s1), Value::String(s2)) => {
                            let s1 = unsafe { (*s1).as_str() };
                            let s2 = unsafe { (*s2).as_str() };
                            s1 <= s2
                        }

                        _ => error!("le", "unsupported types in less-than-or-equal")
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

                        (Value::String(s1), Value::String(s2)) => {
                            let s1 = unsafe { (*s1).as_str() };
                            let s2 = unsafe { (*s2).as_str() };
                            s1 > s2
                        }

                        _ => error!("gt", "unsupported types in greather-than")
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

                        (Value::String(s1), Value::String(s2)) => {
                            let s1 = unsafe { (*s1).as_str() };
                            let s2 = unsafe { (*s2).as_str() };
                            s1 >= s2
                        }

                        _ => error!("ge", "unsupported types in greater-than-or-equal")
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
                        _ => error!("not", "unsupported type in logical not {:?}", v0)
                    };

                    push!(b);
                }

                // Create a new closure
                Insn::clos_new { fun_id, num_slots } => {
                    let num_slots = num_slots as usize;

                     self.gc_check(
                        std::mem::size_of::<Closure>() +
                        std::mem::size_of::<Value>() * num_slots,
                        &mut [],
                    );

                    let clos = Closure::new(fun_id, num_slots, &mut self.alloc).unwrap();
                    push!(clos);
                }

                // Set a closure slot
                Insn::clos_set { idx } => {
                    let val = pop!();
                    let clos = pop!();

                    match clos {
                        Value::Closure(clos) => {
                            let clos = unsafe { &mut *clos };
                            clos.set(idx as usize, val);
                        }
                        _ => error!("clos_set", "expected closure")
                    }
                }

                // Get a closure slot for the function currently executing
                Insn::clos_get { idx } => {
                    let fun = &self.frames[self.frames.len() - 1].fun;

                    let val = match fun {
                        Value::Closure(clos) => {
                            let clos = unsafe { &**clos };
                            clos.get(idx as usize)
                        }
                        _ => error!("clos_get", "not a closure")
                    };

                    if val == Value::Undef {
                        error!("clos_get", "executing uninitialized closure");
                    }

                    push!(val);
                }

                // Create a new mutable cell
                Insn::cell_new => {
                     self.gc_check(
                        std::mem::size_of::<Value>(),
                        &mut [],
                    );

                    let p_cell = self.alloc.alloc(Value::Nil).unwrap();
                    push!(Value::Cell(p_cell));
                }

                // Set the value stored in a mutable cell
                Insn::cell_set => {
                    let cell = pop!();
                    let val = pop!();

                    match cell {
                        Value::Cell(p_cell) => unsafe { *p_cell = val },
                        _ => error!("cell_set", "expected cell")
                    };
                }

                // Get the value stored in a mutable cell
                Insn::cell_get => {
                    let cell = pop!();

                    let val = match cell {
                        Value::Cell(p_cell) => unsafe { *p_cell },
                        _ => error!("cell_get", "invalid cell in cell_get")
                    };

                    push!(val);
                }

                // Create new empty dictionary
                Insn::dict_new => {
                    self.gc_check(
                        size_of::<Dict>() + Dict::size_of_slot(),
                        &mut []
                    );
                    let dict = Dict::with_capacity(0, &mut self.alloc).unwrap();
                    let new_obj = self.alloc.alloc(dict).unwrap();
                    push!(Value::Dict(new_obj))
                }

                // Set object field
                Insn::set_field { mut field, class_id, slot_idx } => {
                    let mut val = pop!();
                    let mut obj = pop!();
                    let mut field_name = unsafe { &*field };

                    match obj {
                        Value::Object(p) => {
                            let obj = unsafe { &mut *p };

                            if class_id == obj.class_id {
                                obj.set(slot_idx as usize, val);
                            } else {
                                let slot_idx = self.get_slot_idx(obj.class_id, field_name.as_str());
                                let class_id = obj.class_id;

                                // Update the cache
                                self.insns[pc - 1] = Insn::set_field {
                                    field,
                                    class_id,
                                    slot_idx: slot_idx as u32,
                                };

                                obj.set(slot_idx, val);
                            }
                        },

                        Value::Dict(p) => {
                            let dict = unsafe { &mut *p };
                            let mut field_name_val = Value::String(field);
                            let alloc_size = dict.will_allocate(field_name.as_str());

                            self.gc_check(
                                alloc_size,
                                &mut [&mut obj, &mut val, &mut field_name_val]
                            );

                            field_name = field_name_val.unwrap_str();
                            let dict = obj.unwrap_dict();
                            dict.set(field_name, val, &mut self.alloc).unwrap();
                        }

                        _ => error!("set_field", "set_field on non-object/dict value")
                    }
                }

                // Allocate a new class instance and call
                // the constructor for the given class
                Insn::new { class_id, argc } => {
                    let num_slots = self.get_num_slots(class_id);

                    self.gc_check(
                        std::mem::size_of::<Object>() +
                        std::mem::size_of::<Value>() * num_slots,
                        &mut [],
                    );

                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc).unwrap();

                    // If a constructor method is present
                    let init_fun = self.get_method(class_id, "init");
                    if let Some(fun_id) = init_fun {
                        let this_pc = pc - 1;

                        // The self value should be first argument to the constructor
                        // The constructor also returns the allocated object
                        self.stack.insert(self.stack.len() - argc as usize, obj_val);
                        let ctor_entry = call_fun!(Value::Fun(fun_id), argc + 1);

                        // Patch the instruction to avoid lookups next time
                        self.insns[this_pc] = Insn::new_known_ctor {
                            class_id,
                            argc,
                            num_slots: num_slots.try_into().unwrap(),
                            ctor_pc: ctor_entry.entry_pc as u32,
                            fun_id,
                            num_locals: ctor_entry.num_locals.try_into().unwrap(),
                        };
                    } else {
                        // Return the allocated object
                        push!(obj_val);
                    }
                }

                Insn::new_known_ctor { class_id, argc, num_slots, ctor_pc, fun_id, num_locals } => {
                    let num_slots = num_slots as usize;

                    self.gc_check(
                        std::mem::size_of::<Object>() +
                        std::mem::size_of::<Value>() * num_slots,
                        &mut [],
                    );

                    // Allocate the object
                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc).unwrap();

                    // The self value should be first argument to the constructor
                    // The constructor also returns the allocated object
                    self.stack.insert(self.stack.len() - argc as usize, obj_val);

                    // We add an extra argument for the self value
                    self.frames.push(StackFrame {
                        argc: argc + 1,
                        fun: Value::Fun(fun_id),
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                    pc = ctor_pc as usize;

                    // Allocate stack slots for the local variables
                    self.stack.resize(self.stack.len() + num_locals as usize, Value::Nil);
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
                                "len" => obj.unwrap_arr().len().into(),
                                _ => error!("get_field", "field not found on array")
                            }
                        }

                        Value::ByteArray(p) => {
                            match field_name.as_str() {
                                "len" => obj.unwrap_ba().num_bytes().into(),
                                _ => error!("get_field", "field not found on bytearray")
                            }
                        }

                        Value::String(p) => {
                            match field_name.as_str() {
                                "len" => {
                                    let s = unsafe { (*p).as_str() };
                                    s.len().into()
                                }
                                _ => error!("get_field", "field not found on string")
                            }
                        }

                        Value::Object(p) => {
                            let obj = unsafe { &*p };

                            // If the class id doesn't match the cache, update it
                            let val = if class_id == obj.class_id {
                                obj.get(slot_idx as usize)
                            } else {
                                let slot_idx = self.get_slot_idx(obj.class_id, field_name.as_str());
                                let class_id = obj.class_id;

                                // Update the cache
                                self.insns[pc - 1] = Insn::get_field {
                                    field,
                                    class_id,
                                    slot_idx: slot_idx as u32,
                                };

                                obj.get(slot_idx as usize)
                            };

                            if val == Value::Undef {
                                error!("get_field", "object field not initialized `{}`", field_name.as_str());
                            }

                            val
                        },

                        Value::Dict(p) => {
                            let dict = unsafe { &mut *p };
                            let key = field_name.as_str();

                            match dict.get(key) {
                                Some(v) => v,
                                None => error!("get_field", "key '{}' not found in dict", key)
                            }
                        }

                        _ => error!("get_field", "get_field on non-object value {:?}", obj)
                    };

                    push!(val);
                }

                Insn::get_index => {
                    let idx = pop!();
                    let mut arr = pop!();

                    let val = match arr {
                        Value::Array(p) => {
                            let arr = unsafe { &mut *p };
                            let idx = unwrap_usize!(idx, "get_index");
                            arr.get(idx)
                        }

                        Value::ByteArray(p) => {
                            let ba = unsafe { &mut *p };
                            let idx = unwrap_usize!(idx, "get_index");
                            Value::from(ba.get(idx))
                        }

                        Value::Dict(p) => {
                            let dict = unsafe { &mut *p };
                            let key = unwrap_str!(idx);

                            match dict.get(key) {
                                Some(v) => v,
                                None => error!("get_index", "key '{}' not found in dict", key)
                            }
                        }

                        _ => error!("get_index", "expected array or dict type in get_index")
                    };

                    push!(val);
                }

                Insn::set_index => {
                    let mut val = pop!();
                    let mut idx = pop!();
                    let mut arr = pop!();

                    match arr {
                        Value::Array(p) => {
                            let arr = unsafe { &mut *p };
                            let idx = unwrap_usize!(idx, "get_index");
                            arr.set(idx, val);
                        }

                        Value::ByteArray(p) => {
                            let ba = unsafe { &mut *p };
                            let idx = unwrap_usize!(idx, "get_index");
                            let b = val.unwrap_u8();
                            ba.set(idx, b);
                        }

                        Value::Dict(p) => {
                            let dict = unsafe { &mut *p };
                            let key = unwrap_str!(idx);

                            let alloc_size = dict.will_allocate(key);
                            self.gc_check(
                                alloc_size,
                                &mut [&mut arr, &mut idx, &mut val],
                            );

                            let dict = arr.unwrap_dict();
                            let key = idx.unwrap_str();
                            dict.set(key, val, &mut self.alloc).unwrap();
                        }

                        _ => error!("set_index", "expected array or dict type")
                    };
                }

                // Create new empty array
                Insn::arr_new { capacity } => {
                    let capacity = capacity as usize;

                    self.gc_check(
                        size_of::<Array>() + size_of::<Value>() * capacity,
                        &mut [],
                    );

                    let new_arr = Array::with_capacity(capacity, &mut self.alloc).unwrap();
                    push!(Value::Array(self.alloc.alloc(new_arr).unwrap()))
                }

                // Append an element at the end of an array
                // This instruction is used to construct array literals
                Insn::arr_push => {
                    let val = pop!();
                    let mut array = pop!();
                    crate::array::array_push(self, array, val).unwrap();
                }

                // Clone a bytearray
                Insn::ba_clone => {
                    let mut val = pop!();
                    let ba = val.unwrap_ba();

                    self.gc_check(
                        size_of::<ByteArray>() + ba.num_bytes(),
                        &mut [&mut val],
                    );

                    let ba = val.unwrap_ba();
                    let ba_clone = ba.clone(&mut self.alloc).unwrap();
                    let p_clone = self.alloc.alloc(ba_clone).unwrap();
                    push!(Value::ByteArray(p_clone));
                }

                // Jump if true
                Insn::if_true { target_ofs } => {
                    let v = pop!();

                    match v {
                        Value::True => { pc = ((pc as i64) + (target_ofs as i64)) as usize }
                        Value::False => {}
                        _ => error!("if_true", "if_true instruction only accepts boolean values")
                    }
                }

                // Jump if false
                Insn::if_false { target_ofs } => {
                    let v = pop!();

                    match v {
                        Value::False => { pc = ((pc as i64) + (target_ofs as i64)) as usize }
                        Value::True => {}
                        _ => error!("if_false", "if_false instruction only accepts boolean values")
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

                    match self_val {
                        Value::Object(p) => {
                            let obj = unsafe { &*p };
                            let fun_id = match self.get_method(obj.class_id, method_name.as_str()) {
                                None => error!(
                                    "call to method `{}`, not found on class `{}`",
                                    method_name.as_str(),
                                    self.get_class_name(obj.class_id)
                                ),
                                Some(fun_id) => fun_id,
                            };

                            let this_pc = pc - 1;
                            let fun_entry = call_fun!(Value::Fun(fun_id), argc + 1);

                            // Patch this instruction to avoid the method lookup next time
                            self.insns[this_pc] = Insn::call_method_pc {
                                name,
                                argc: argc.try_into().unwrap(),
                                class_id: obj.class_id,
                                entry_pc: fun_entry.entry_pc.try_into().unwrap(),
                                fun_id,
                                num_locals: fun_entry.num_locals.try_into().unwrap(),
                            };
                        }

                        _ => {
                            let fun = crate::runtime::get_method(self_val, method_name.as_str());

                            if fun == Value::Nil {
                                error!("call to unknown method `{}`", method_name.as_str());
                            }

                            call_fun!(fun, argc + 1);
                        }
                    };
                }

                Insn::call_method_pc { name, argc, class_id, entry_pc, fun_id, num_locals } => {
                    let self_val = self.stack[self.stack.len() - (1 + argc as usize)];

                    // Guard that self is an object with a matching class id
                    if let Value::Object(p_obj) = self_val {
                        let obj = unsafe { &*p_obj };

                        if obj.class_id == class_id {
                            let argc: u8 = argc.into();
                            self.frames.push(StackFrame {
                                argc: argc + 1,
                                fun: Value::Fun(fun_id),
                                prev_bp: bp,
                                ret_addr: pc,
                            });

                            // The base pointer will point at the first local
                            bp = self.stack.len();
                            pc = entry_pc as usize;

                            // Allocate stack slots for the local variables
                            self.stack.resize(self.stack.len() + num_locals as usize, Value::Nil);

                            // Proceed with the call
                            continue;
                        }
                    }

                    // The guard fail, deoptimize this instruction and try again
                    pc -= 1;
                    self.insns[pc] = Insn::call_method {
                        name,
                        argc: argc.into(),
                    };
                }

                Insn::ret => {
                    if self.stack.len() <= bp {
                        error!("ret", "no return value on stack");
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

                #[allow(unreachable_patterns)]
                _ => error!("unknown opcode {:?}", insn)
            }
        }
    }
}

#[derive(Clone)]
struct ActorTx
{
    sender: mpsc::SyncSender<Message>,
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
        let (queue_tx, queue_rx) = mpsc::sync_channel::<Message>(1024);

        // Create an allocator to send messages to the actor
        let mut msg_alloc = Alloc::new();

        // Hash map for remapping copied values
        let mut dst_map = HashMap::new();

        // We need to recursively copy the function/closure
        // using the actor's message allocator
        let fun = deepcopy(fun, &mut msg_alloc, &mut dst_map).unwrap();

        // Copy the global variables from the parent actor
        let mut globals = parent.globals.clone();
        for val in &mut globals {
            *val = deepcopy(*val, &mut msg_alloc, &mut dst_map).unwrap();
        }

        remap(&mut dst_map);

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

            let ret_val = actor.call(fun, &args);

            // TODO: a possible solution here would be to copy heap return
            // values into our own message allocator, which will continue to
            // live and won't be garbage collected since this actor is done
            // executing

            // Deny returning a heap-allocated value
            // This is because the allocator owning this memory is about
            // to die
            if ret_val.is_heap() {
                panic!("cannot return heap-allocated value from actor");
            }

            ret_val
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
        vm.actor_txs.remove(&tid).unwrap();
        drop(vm);

        // Note: there is no need to copy data when joining,
        // because the actor sending the data is done running
        handle.join().expect(&format!("could not join thread with id {}", tid))
    }

    // Call a function in the main actor
    pub fn call(vm: &mut Arc<Mutex<VM>>, fun_id: FunId, args: Vec<Value>) -> Value
    {
        let vm_mutex = vm.clone();

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::sync_channel::<Message>(1024);

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

    /// Send a message to an actor without copying it to its message allocator
    pub fn send_nocopy(&self, actor_id: u64, msg: Value) -> Result<(), ()>
    {
        let actor_tx = self.actor_txs.get(&actor_id).ok_or(())?;
        actor_tx.sender.send(Message { sender: 0, msg }).map_err(|_| ())
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

        dbg!(size_of::<Value>());
        assert!(size_of::<Value>() <= 16);

        dbg!(size_of::<Insn>());
        assert!(size_of::<Insn>() <= 24);

        dbg!(size_of::<ClassId>());
        assert!(size_of::<ClassId>() <= 4);
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

        // Global variable read
        eval_eq("let g = 3; fun f() { return g+1; } return f();", Value::Int64(4));

        // Function calling another function
        eval_eq("fun a() { return 8; } fun b() { return a(); } return b();", Value::Int64(8));
    }

    #[test]
    fn ret_clos()
    {
        // Function returning a closure
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
    fn counter_clos()
    {
        // Read mutable captured variable
        eval_eq("fun f() { let var n = 0; return || n; } let c = f(); return c();", Value::Int64(0));

        // Write mutable captured variable
        eval_eq("fun f() { let var n = 0; return || n = 1; } let c = f(); return c();", Value::Int64(1));

        // Counter
        eval_eq("fun f() { let var n = 0; return || ++n; } let c = f(); c(); return c();", Value::Int64(2));
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

    #[test]
    fn dicts()
    {
        eval("let o = {};");
        eval("let o = { x: 1, y: 2 };");
        eval_eq("let o = { x: 1, y: 2 }; return o.x;", Value::Int64(1));
        eval_eq("let o = { x: 1, y: 2 }; return o.x + o.y;", Value::Int64(3));
        eval_eq("let o = { 'x': 77 }; return o.x;", Value::Int64(77));
        eval_eq("let o = { 'foo bar': 5 }; return o['foo bar'];", Value::Int64(5));
        eval_eq("let o = { x:5 }; o['x'] = 3; return o.x;", Value::Int64(3));
        eval_eq("let o = { x:5 }; return o.has('x');", Value::True);
    }

    #[test]
    #[should_panic]
    fn dict_missing_key()
    {
        eval("let v = {}.x;");
    }

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
        eval("let a = ByteArray.with_size(0);");
        eval("let a = ByteArray.with_size(1024); assert(a.len == 1024);");
        eval("let a = ByteArray.with_size(32); a.store_u32(0, 0xFF_FF_FF_FF);");
        eval("let a = ByteArray.with_size(32); a.store_u32(0, 0xFF_00_00_00); assert(a[0] == 0 && a[3] == 255);");
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
    #[should_panic]
    fn get_undef_field()
    {
        // The field x exists on the class but is not initialized
        eval("class F { g(s) { s.x = 3; } } let o = F(); o.x;");
    }

    #[test]
    #[should_panic]
    fn ctor_argc_mismatch()
    {
        // Passing an argument to a constructor that accepts none
        eval("class Foo { init(s) {} } let o = Foo(1);");
    }

    #[test]
    #[should_panic]
    fn no_ctor_arg()
    {
        // Passing an argument to a non-existent constructor
        eval("class Foo {} let o = Foo(1);");
    }

    #[test]
    fn instanceof()
    {
        eval_eq("class F {} return nil instanceof F;", Value::False);
        eval_eq("class F {} let o = F(); return o instanceof F;", Value::True);
        eval_eq("class F {} class G {} let o = F(); return o instanceof G;", Value::False);
        eval_eq("class F {} return F() instanceof F;", Value::True);

        // Core runtime classes
        eval_eq("return nil instanceof Int64;", Value::False);
        eval_eq("return true instanceof Int64;", Value::False);
        eval_eq("return 5 instanceof Int64;", Value::True);
        eval_eq("return 77 instanceof String;", Value::False);
        eval_eq("return 'foo' instanceof String;", Value::True);
        eval_eq("return [] instanceof Array;", Value::True);
        eval_eq("return {} instanceof Dict;", Value::True);
    }
}
