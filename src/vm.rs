use std::collections::{HashSet, HashMap};
use std::{thread, thread::sleep};
use std::sync::{Arc, Mutex};
//use crate::host::*;

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
    get_arg { idx: u16 },

    // Set the function argument at a given index
    set_arg { idx: u16 },

    /*
    // Get a variadic argument with a dynamic index variable
    // get_arg (idx)
    get_var_arg,
    */

    // Get the local variable at a given stack slot index
    // The index is relative to the base of the stack frame
    // get_local <idx:u16>
    get_local { idx: u16 },

    // Set the local variable at a given stack slot index
    // The index is relative to the base of the stack frame
    // set_local <idx:u16> (value)
    set_local { idx: u16 },

    // Arithmetic
    add,
    sub,
    mul,
    div,

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

    // Objects manipulation
    obj_new,
    obj_copy,
    obj_def { field_name: *const String },
    obj_set { field_name: *const String },
    obj_get { field_name: *const String },
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

    // Jump if true/false
    if_true_stub { target_idx: u32 },
    if_true { target_pc: usize },
    if_false_stub { target_idx: u32 },
    if_false { target_pc: usize },

    // Unconditional jump
    jump_stub { target_idx: u32 },
    jump { target_pc: usize },

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

#[derive(Default, Copy, Clone, Debug, PartialEq)]
pub enum Value
{
    // The default value for uninitialized memory is none
    #[default]
    None,

    False,
    True,

    Int64(i64),
    Float64(f64),

    //String(*const String),
    //Object(*mut Object),
    //Array(*mut Array),
    //ByteArray(*mut crate::arrays::ByteArray),

    // Reference to a global definition in an image file
    //ImgRef(usize)
}
use Value::{False, True, Int64, Float64};

// Allow sending Value between threads
unsafe impl Send for Value {}
unsafe impl Sync for Value {}

impl Value
{
    pub fn is_falsy(&self) -> bool
    {
        use Value::*;
        match self {
            None => true,
            False => true,
            _ => false,
        }
    }

    pub fn is_truthy(&self) -> bool
    {
        !self.is_falsy()
    }

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






struct StackFrame
{
    // Callee function
    //fun: *mut Object,

    // Argument count (number of args supplied)
    argc: u16,

    // Previous base pointer at the time of call
    prev_bp: usize,

    // Return address
    ret_addr: usize,
}









pub struct Actor
{
    vm: Arc<Mutex<VM>>,

    // Value stack
    stack: Vec<Value>,

    // List of stack frames (activation records)
    frames: Vec<StackFrame>,
}



impl Actor
{
    pub fn new(vm: Arc<Mutex<VM>>) -> Self
    {
        todo!();
    }





    pub fn call(&mut self, fun: Value, args: &[Value]) -> Value
    {
        assert!(self.stack.len() == 0);
        assert!(self.frames.len() == 0);

        /*
        let mut fun = fun.unwrap_obj();

        // Push a new stack frame
        self.frames.push(StackFrame {
            fun,
            argc: args.len().try_into().unwrap(),
            prev_bp: usize::MAX,
            ret_addr: usize::MAX,
        });

        // Push the arguments on the stack
        for arg in args {
            self.stack.push(*arg);
        }

        // The base pointer will point at the first local
        let mut bp = self.stack.len();

        // Get a compiled address for this function
        let mut pc = self.get_version(fun, 0);

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

                Insn::set_arg { idx } => {
                    let argc = self.frames[self.frames.len() - 1].argc as usize;
                    let idx = idx as usize;

                    if idx >= argc {
                        panic!("invalid index in set_arg, idx={}, argc={}", idx, argc);
                    }

                    // Last argument is at bp - 1 (if there are arguments)
                    let stack_idx = (bp - argc) + idx;
                    let val = pop!();
                    self.stack[stack_idx] = val;
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
                        (Value::String(p0), Value::String(p1)) => p0 == p1,
                        _ => panic!()
                    };

                    push_bool!(b);
                }

                Insn::ne => {
                    let v1 = pop!();
                    let v0 = pop!();

                    let b = match (v0, v1) {
                        (Int64(v0), Int64(v1)) => v0 != v1,
                        (Value::String(p0), Value::String(p1)) => p0 != p1,
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

                // Get value type
                Insn::type_of => {
                    let v0 = pop!();

                    let s = match v0 {
                        Value::None => "none",

                        Value::True => "bool",
                        Value::False => "bool",

                        Value::Int64(_) => "int64",
                        Value::String(_) => "string",

                        Value::Object(_) => "object",
                        Value::Array(_) => "array",

                        _ => panic!()
                    };

                    // FIXME: locking here is slow/inefficient
                    // We ideally want to cache the type strings somewhere
                    let s = self.alloc.get_string(s);
                    push!(s);
                }

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
                    let arr = pop!().unwrap_arr();
                    let val = Array::get(arr, idx);
                    push!(val);
                }

                Insn::arr_set => {
                    let idx = pop!().unwrap_u64();
                    let arr = pop!().unwrap_arr();
                    let val = pop!();
                    Array::set(arr, idx, val);
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

                // Create new empty bytearray
                Insn::ba_new { capacity } => {
                    let new_arr = ByteArray::new(
                        &mut self.alloc,
                        capacity as usize
                    );
                    push!(Value::ByteArray(new_arr))
                }

                // Jump if true
                Insn::if_true_stub { target_idx } => {
                    let v = pop!();

                    match v {
                        Value::True => {
                            let target_pc = self.get_version(fun, target_idx);
                            self.insns[pc - 1] = Insn::if_true { target_pc };
                            pc = target_pc;
                        }
                        Value::False => {},
                        _ => panic!()
                    }
                }

                Insn::if_true { target_pc } => {
                    let v = pop!();

                    match v {
                        Value::True => { pc = target_pc; }
                        Value::False => {}
                        _ => panic!()
                    }
                }

                // Jump if false
                Insn::if_false_stub { target_idx } => {
                    let v = pop!();

                    match v {
                        Value::False => {
                            let target_pc = self.get_version(fun, target_idx);
                            self.insns[pc - 1] = Insn::if_false { target_pc };
                            pc = target_pc;
                        }
                        Value::True => {},
                        _ => panic!()
                    }
                }

                Insn::if_false { target_pc } => {
                    let v = pop!();

                    match v {
                        Value::False => { pc = target_pc; }
                        Value::True => {}
                        _ => panic!()
                    }
                }

                // Unconditional jump
                Insn::jump_stub{ target_idx } => {
                    let target_pc = self.get_version(fun, target_idx);
                    self.insns[pc - 1] = Insn::jump { target_pc };
                    pc = target_pc;
                }

                Insn::jump { target_pc } => {
                    pc = target_pc;
                }

                Insn::call_host { host_fn, argc } => {
                    if host_fn.num_params() != (argc as usize) {
                        panic!();
                    }

                    match host_fn
                    {
                        HostFn::Fn0_0(fun) => {
                            fun(&mut self.vm, &mut self.alloc);
                            push!(Value::None);
                        }

                        HostFn::Fn0_1(fun) => {
                            let v = fun(&mut self.vm, &mut self.alloc);
                            push!(v);
                        }

                        HostFn::Fn1_0(fun) => {
                            let a0 = pop!();
                            fun(&mut self.vm, &mut self.alloc, a0);
                            push!(Value::None);
                        }

                        HostFn::Fn1_1(fun) => {
                            let a0 = pop!();
                            let v = fun(&mut self.vm, &mut self.alloc, a0);
                            push!(v);
                        }

                        HostFn::Fn2_0(fun) => {
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(&mut self.vm, &mut self.alloc, a0, a1);
                            push!(Value::None);
                        }

                        HostFn::Fn2_1(fun) => {
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(&mut self.vm, &mut self.alloc, a0, a1);
                            push!(v);
                        }

                        HostFn::Fn3_0(fun) => {
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(&mut self.vm, &mut self.alloc, a0, a1, a2);
                            push!(Value::None);
                        }

                        HostFn::Fn3_1(fun) => {
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(&mut self.vm, &mut self.alloc, a0, a1, a2);
                            push!(v);
                        }

                        HostFn::Fn4_0(fun) => {
                            let a3 = pop!();
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            fun(&mut self.vm, &mut self.alloc, a0, a1, a2, a3);
                            push!(Value::None);
                        }

                        HostFn::Fn4_1(fun) => {
                            let a3 = pop!();
                            let a2 = pop!();
                            let a1 = pop!();
                            let a0 = pop!();
                            let v = fun(&mut self.vm, &mut self.alloc, a0, a1, a2, a3);
                            push!(v);
                        }
                    }
                }

                // call (arg0, arg1, ..., argN, fun)
                Insn::call { argc } => {
                    // Function to call
                    let fun_val = pop!();
                    fun = fun_val.unwrap_obj();

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
                    pc = self.get_version(fun, 0);
                }

                // call (arg0, arg1, ..., argN, fun)
                Insn::call_known { argc, callee } => {
                    // Get a compiled address for this function
                    let target_pc = self.get_version(callee, 0);

                    // Patch this instruction with the compiled pc
                    self.insns[pc - 1] = Insn::call_pc { argc, callee, target_pc };

                    // Executed the patched instruction next
                    pc -= 1;
                }

                // call (arg0, arg1, ..., argN, fun)
                Insn::call_pc { argc, callee, target_pc } => {
                    // Function being currently executed
                    fun = callee;

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
                    fun = self.frames[self.frames.len()-1].fun;

                    push!(ret_val);
                }

                _ => panic!("unknown opcode {:?}", insn)
            }
        }
        */



        todo!();
    }
}

pub struct VM
{


    // Next actor id to assign
    next_actor_id: u64,

    // Map from thread ids to join handles
    threads: HashMap<u64, thread::JoinHandle<Value>>,

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
    pub fn new() -> Arc<Mutex<VM>>
    {
        /*
        let vm = Self {
            root_alloc,
            next_tid: 0,
            threads: HashMap::default(),
            vm: None
        };

        let vm = Arc::new(Mutex::new(vm));

        // Store a reference to the mutex on the VM
        // This is so we can pass this reference to threads
        vm.lock().unwrap().vm = Some(vm.clone());

        vm
        */

        todo!();
    }

    // Create a new actor
    pub fn new_thread(vm: &mut Arc<Mutex<VM>>, fun: Value, args: Vec<Value>) -> u64
    {
        /*
        let vm_mutex = vm.clone();

        // Assign a thread id
        let mut vm_ref = vm.lock().unwrap();
        let tid = vm_ref.next_tid;
        vm_ref.next_tid += 1;
        drop(vm_ref);

        // Spawn the new thread
        let handle = thread::spawn(move || {
            let mut t = Thread::new(vm_mutex);
            t.call(fun, &args)
        });

        // Add the thread to the map
        let mut vm_ref = vm.lock().unwrap();
        vm_ref.threads.insert(tid, handle);
        drop(vm_ref);

        tid as u64
        */

        todo!();
    }

    /*
    // Wait for a thread to produce a result and return it.
    pub fn join_thread(vm: &mut Arc<Mutex<VM>>, tid: u64) -> Value
    {
        let mut vm = vm.lock().unwrap();

        let mut handle = vm.threads.remove(&tid).unwrap();

        // Release the VM lock
        drop(vm);

        handle.join().expect(&format!("could not join thread with id {}", tid))
    }
    */

    /*
    // Call a function in the main thread
    pub fn call(vm: &mut Arc<Mutex<VM>>, fun: Value, args: Vec<Value>) -> Value
    {
        // Note: we use join_thread here because we don't want to lock
        // on the VM during the call
        let tid = Self::new_thread(vm, fun, args);

        Self::join_thread(vm, tid)
    }
    */
}

#[cfg(test)]
mod tests
{
    use super::*;
    //use crate::image::*;

    /*
    fn run_image(file_name: &str) -> Value
    {
        let mut root_alloc = RootAlloc::new();
        let mut alloc = Alloc::new(root_alloc.clone());
        let fun = parse_file(&mut alloc, file_name).unwrap();
        let mut vm = VM::new(root_alloc);
        let ret = VM::call(&mut vm, fun, vec![]);
        ret
    }
    */

    /*
    #[test]
    fn vm_new()
    {
        let mut root_alloc = RootAlloc::new();
        let vm = VM::new(root_alloc);
    }
    */

    /*
    #[test]
    fn str_interning()
    {
        let mut root_alloc = RootAlloc::new();
        let mut alloc = Alloc::new(root_alloc);
        let foo_str = alloc.get_string("foo");
        let foo_str2 = alloc.get_string("foo");
        let bar_str = alloc.get_string("bar");
        assert!(foo_str == foo_str2);
        assert!(foo_str != bar_str);
    }

    #[test]
    fn ret1()
    {
        let ret = run_image("tests/ret_1.zim");
        assert!(ret == Value::Int64(1));
    }

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
