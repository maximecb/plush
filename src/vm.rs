use rustc_hash::FxHashMap as HashMap;
use std::thread;
use std::sync::{Arc, Weak, Mutex, mpsc};
use std::time::Duration;
use crate::dict::Dict;
#[cfg(feature = "log_gc")]
use crate::utils::thousands_sep;
use crate::lexer::SrcPos;
use crate::ast::{Program, FunId, ClassId, Class};
use crate::alloc::{Alloc, Tag, HEADER_SIZE, INIT_SIZE, MSG_INIT_SIZE};
use crate::object::Object;
use crate::closure::Closure;
use crate::array::Array;
use crate::bytearray::ByteArray;
use crate::codegen::CompiledFun;
use crate::gc::{undo_forwarding, Copier, StrTable, UndoLog};
use crate::host::*;
use crate::str::Str;
use crate::value::*;
use std::mem::size_of;
use std::ops::{Add, Sub, Mul};

/// How many bytes of undrained messages a sender will let pile up in a
/// receiver's message allocator before waiting for it to catch up.
///
/// This is backpressure, not a limit on message size: a message that is
/// already being copied grows the buffer past this point if it needs to.
/// It exists because the buffer is only reclaimed in one go, when the
/// receiver has drained its queue, so without it a fast sender would grow
/// the buffer without bound.
const MSG_BACKLOG_LIMIT: usize = 64 * 1024 * 1024;

/// Instruction opcodes
/// Note: commonly used upcodes should be in the [0, 127] range (one byte)
///       less frequently used opcodes can take multiple bytes if necessary.
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
pub enum Insn
{
    // Halt execution and produce an error
    panic { pos: SrcPos },

    // No-op
    // Not currently emitted by codegen, kept as a building block
    #[allow(dead_code)]
    nop,

    // Push a value to the stack
    push { val: Value },

    // Stack manipulation
    pop,
    dup,

    // Not currently emitted by codegen, kept as a building block
    #[allow(dead_code)]
    swap,

    // Push the nth-value (indexed from the stack top) on top of the stack
    // getn 0 is equivalent to dup
    getn { idx: u16 },

    // Get the function argument at a given index
    get_arg { idx: u32 },

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
    get_field { field: Value, class_id: ClassId, slot_idx: u32 },
    set_field { field: Value, class_id: ClassId, slot_idx: u32 },

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

    // Call a function using the call stack
    // call (arg0, arg1, ..., argN)
    call { argc: u8 },

    // Call a known function using its function id
    call_direct { fun_id: FunId, argc: u8 },

    // Call a known function by directly jumping to its entry point
    call_pc { entry_pc: u32, fun_id: FunId, num_locals: u16, argc: u8 },

    // Call a method on an object
    // call_method (self, arg0, ..., argN)
    call_method { name: Value, argc: u8 },

    // Call a method with a previously known pc
    call_method_pc { name: Value, argc: u8, class_id: ClassId, entry_pc: u32, fun_id: FunId, num_locals: u16 },

    // Call a host method on a primitive, guarded on the type tag
    call_method_host { name: Value, argc: u8, type_tag: Type, host_fn: &'static HostFn },

    // Return
    ret,
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

/// Mesage to be sent to an actor
pub struct Message
{
    // Sender actor id
    // Can be none when the message is a callback
    #[allow(dead_code)]
    sender: u64,

    // Message to be sent
    msg: Value,

    // Number of bytes the message occupies in the receiver's message
    // allocator. The receiver uses this to make room in its own heap
    // before copying the message out.
    size: usize,
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

    // Spare allocator used as to-space for copying GC
    to_space: Option<Alloc>,

    // Strings copied during the current copy, so that equal strings can
    // share one allocation. Forwarding pointers work by address, so
    // nothing else would deduplicate them.
    str_table: StrTable,

    // Headers overwritten with forwarding addresses, kept when copying
    // out of this actor's own heap so that they can be put back
    undo_log: UndoLog,

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
    pub(crate) insns: Vec<Insn>,
}

/// Why an integer operation produced no result
fn int_op_error(insn: &str, divisor: i64) -> String
{
    if divisor == 0 {
        format!("division by zero in {}", insn)
    } else {
        format!("integer overflow in {}", insn)
    }
}

/// Slow path of an arithmetic instruction: integers that don't fit a
/// fixnum, floats, and mixed operands. `$checked` is the i64 operation
/// and `$op` the float one.
macro_rules! num_slow_path {
    ($name: ident, $insn: literal, $checked: ident, $op: tt) => {
        #[cold]
        fn $name(&mut self, v0: Value, v1: Value) -> Result<Value, String>
        {
            if let (Some(a), Some(b)) = (v0.to_i64(), v1.to_i64()) {
                return match a.$checked(b) {
                    Some(r) => Ok(self.int64(r)),
                    None => Err(int_op_error($insn, b)),
                };
            }

            if v0.is_num() && v1.is_num() {
                let r = v0.num_as_f64() $op v1.num_as_f64();
                return Ok(self.float64(r));
            }

            Err(format!("unsupported operand types for {}: {:?} and {:?}", $insn, v0, v1))
        }
    }
}

/// Slow path of an instruction that only accepts integers. `$op` is
/// the operation, and produces None where it is undefined.
macro_rules! int_slow_path {
    ($name: ident, $insn: literal, $op: expr) => {
        #[cold]
        fn $name(&mut self, v0: Value, v1: Value) -> Result<Value, String>
        {
            let a = unwrap_i64!(v0, $insn);
            let b = unwrap_i64!(v1, $insn);

            match ($op)(a, b) {
                Some(r) => Ok(self.int64(r)),
                None => Err(int_op_error($insn, b)),
            }
        }
    }
}

/// Slow path of a comparison: boxed integers, floats and strings
macro_rules! cmp_slow_path {
    ($name: ident, $insn: literal, $op: tt) => {
        #[cold]
        fn $name(v0: Value, v1: Value) -> Result<bool, String>
        {
            if let (Some(a), Some(b)) = (v0.to_i64(), v1.to_i64()) {
                return Ok(a $op b);
            }

            if v0.is_num() && v1.is_num() {
                return Ok(v0.num_as_f64() $op v1.num_as_f64());
            }

            if v0.is_string() && v1.is_string() {
                return Ok(v0.as_str() $op v1.as_str());
            }

            Err(format!("unsupported types in {}: {:?} and {:?}", $insn, v0, v1))
        }
    }
}

cmp_slow_path!(cmp_lt, "less-than", <);
cmp_slow_path!(cmp_le, "less-than-or-equal", <=);
cmp_slow_path!(cmp_gt, "greater-than", >);
cmp_slow_path!(cmp_ge, "greater-than-or-equal", >=);

impl Actor
{
    pub fn new(
        actor_id: u64,
        parent_id: Option<u64>,
        vm: Arc<Mutex<VM>>,
        alloc: Alloc,
        msg_alloc: Arc<Mutex<Alloc>>,
        queue_rx: mpsc::Receiver<Message>,
        globals: Vec<Value>,
    ) -> Self
    {
        Self {
            actor_id,
            parent_id,
            vm,
            alloc,
            msg_alloc,
            queue_rx,
            globals,
            to_space: None,
            str_table: StrTable::default(),
            undo_log: UndoLog::default(),
            actor_map: HashMap::default(),
            stack: Vec::default(),
            frames: Vec::default(),
            insns: Vec::default(),
            classes: HashMap::default(),
            funs: HashMap::default(),
        }
    }

    /// Copy a received message out of the message allocator and into this
    /// actor's own heap. Doing this on receipt means nothing the actor can
    /// reach ever lives outside its own heap, so the message allocator is
    /// a plain transfer buffer that the GC never has to look at.
    fn take_msg(&mut self, msg: Message) -> Value
    {
        if !msg.msg.is_heap() {
            return msg.msg;
        }

        // Make room for the copy first. The message is not reachable from
        // any root yet, so a collection here does not need to know about
        // it: it stays put in the message allocator until we copy it
        // below. Copying it here cannot come out larger than it already
        // is, since it was itself produced by a copy.
        self.gc_check(msg.size, &mut []);

        // The string table is shared with the collector, which sizes it
        // for the whole heap. Drop it if it has grown past what a message
        // needs, rather than keeping it around at that size.
        if self.str_table.capacity() > 1024 {
            self.str_table = StrTable::default();
        }

        // The message allocator holds nothing but messages waiting to be
        // taken, so the copy is free to leave forwarding addresses in it
        let mut str_table = std::mem::take(&mut self.str_table);
        let mut copier = Copier::new(&mut self.alloc, &mut str_table);
        let val = copier.forward(msg.msg);
        copier.run();
        self.str_table = str_table;

        val
    }

    /// Receive a message from the message queue
    /// This will block until a message is available
    pub fn recv(&mut self) -> Value
    {
        use crate::window::poll_ui_msg;

        // Call try_recv first, so that a message that is already waiting
        // is taken before we block
        if let Some(msg) = self.try_recv() {
            return msg;
        }

        if self.actor_id != 0 {
            let msg = self.queue_rx.recv().unwrap();
            return self.take_msg(msg);
        }

        // Actor 0 (the main actor) may need to poll for UI events
        loop {
            // Poll for UI messages. These are built directly in this
            // actor's heap, so they need no copying.
            let ui_msg = poll_ui_msg(self);
            if let Some(msg) = ui_msg {
                return msg;
            }

            // Block on the message queue for up to 8ms
            let msg = self.queue_rx.recv_timeout(Duration::from_millis(8));

            if let Ok(msg) = msg {
                return self.take_msg(msg);
            }
        }
    }

    /// Try to receive a message from the message queue
    /// This function will not block if no message is available
    pub fn try_recv(&mut self) -> Option<Value>
    {
        use crate::window::poll_ui_msg;

        // Actor 0 (the main actor) needs to poll for UI events
        if self.actor_id == 0 {
            let ui_msg = poll_ui_msg(self);
            if let Some(msg) = ui_msg {
                return Some(msg);
            }
        }

        // Take a message off the queue. This needs no lock on the message
        // allocator: the sender published the message through the channel,
        // and the buffer is only ever reset below, when the queue is known
        // to be empty.
        if let Ok(msg) = self.queue_rx.try_recv() {
            return Some(self.take_msg(msg));
        }

        // The queue is empty, and every message we received has already
        // been copied into our own heap, so the buffer holds nothing but
        // garbage and can simply be reset.
        //
        // Senders hold the buffer's lock across their channel send, so an
        // empty queue while we hold that lock means nothing is in flight.
        // We use try_lock rather than lock because a sender blocked on a
        // full queue holds the lock, and only we can drain the queue to
        // release it.
        //
        // We reset it every time the queue drains rather than waiting for
        // it to fill up. Resetting is just an index, so doing it often is
        // free, and it keeps the buffer from filling while a sender is
        // mid-flight.
        let alloc_rc = self.msg_alloc.clone();

        if let Ok(mut msg_alloc) = alloc_rc.try_lock() {
            if msg_alloc.bytes_used() > 0 {
                let queued = self.queue_rx.try_recv();

                match queued {
                    Ok(msg) => {
                        drop(msg_alloc);
                        return Some(self.take_msg(msg));
                    }

                    Err(_) => {
                        // Nothing but the copier ever allocates in here,
                        // and it writes every byte of every block it
                        // copies, so this buffer never has to be zeroed.
                        msg_alloc.reset();

                        // Only give memory back after something genuinely
                        // large came through. Shrinking is an mmap call, so
                        // it must stay out of the steady state.
                        if msg_alloc.mem_size() > 4 * MSG_BACKLOG_LIMIT {
                            msg_alloc.shrink_to(MSG_INIT_SIZE);
                        }
                    }
                }
            }
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

        // Take owned handles to the receiver so that we stop borrowing
        // ourselves, since the copy below needs our scratch buffers
        // Note: upgrading can fail if the receiving thread panics
        let actor_tx = actor_tx.unwrap();
        let sender = actor_tx.sender.clone();

        // Copy the message using the receiver's message allocator
        let alloc_rc = match actor_tx.msg_alloc.upgrade() {
            Some(rc) => rc,
            None => return Err(()),
        };
        // Wait if too many undrained messages have piled up. The receiver
        // resets the buffer once it has copied every message out, so this
        // makes progress as long as it is still running. Give up eventually
        // rather than spinning forever if it is not.
        let mut attempts = 0;

        let mut msg_alloc = loop {
            let msg_alloc = alloc_rc.lock().unwrap();

            if msg_alloc.bytes_used() <= MSG_BACKLOG_LIMIT {
                break msg_alloc;
            }

            drop(msg_alloc);
            attempts += 1;

            if attempts > 1_000_000 {
                return Err(());
            }

            std::thread::yield_now();
        };

        let bytes_before = msg_alloc.bytes_used();

        // Unlike the other two copies, this one reads out of our own live
        // heap, so the forwarding addresses it leaves behind have to be
        // taken back out afterwards.
        let mut str_table = std::mem::take(&mut self.str_table);
        let mut undo_log = std::mem::take(&mut self.undo_log);

        let mut copier = Copier::with_undo(
            &mut msg_alloc,
            &mut str_table,
            &mut undo_log
        );
        let msg = copier.forward(msg);
        copier.run();

        undo_forwarding(&mut undo_log);
        self.str_table = str_table;
        self.undo_log = undo_log;

        let size = msg_alloc.bytes_used() - bytes_before;

        // Queue the message while still holding the message allocator
        // lock. That way the receiver never observes an empty queue while
        // a message is sitting unqueued in its buffer, which is what makes
        // it safe for the receiver to reset the buffer.
        let res = sender.send(Message { sender: self.actor_id, msg, size });
        drop(msg_alloc);

        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    /// Get the number of parameters a function takes, without
    /// having to compile it first
    pub fn get_num_params(&self, fun_id: FunId) -> usize
    {
        let vm = self.vm.lock().unwrap();
        vm.prog.funs[&fun_id].params.len()
    }

    /// Get a compiled function entry for a given function id
    /// Compile a function, if it has not been compiled yet.
    ///
    /// Compiling allocates the names and literals the function needs, so
    /// this can collect. That makes it a safepoint: callers must hold no
    /// heap value across it that the collector cannot reach.
    #[inline]
    fn get_compiled_fun(&mut self, fun: &mut Value) -> CompiledFun
    {
        // A closure compiles as the function it closes over
        let fun_id = fun.to_fun_id().expect("function value");

        if let Some(entry) = self.funs.get(&fun_id) {
            return *entry;
        }

        self.compile_fun(fun_id, fun)
    }

    /// Compile a function that has not been compiled yet. Kept out of
    /// line so that calling an already compiled function, which is the
    /// common case, stays a lookup and nothing more.
    #[inline(never)]
    fn compile_fun(&mut self, fun_id: FunId, fun: &mut Value) -> CompiledFun
    {
        // The callee itself may be a closure that only the caller holds,
        // which the collection below would leave behind. Park it on the
        // stack, which the collector walks and updates, for as long as
        // compiling takes.
        self.stack.push(*fun);

        // Borrow the function from the VM and compile it. The handle is
        // cloned so that the lock does not borrow the actor, which
        // compiling needs to itself in order to make room as it goes.
        let vm = self.vm.clone();
        let vm = vm.lock().unwrap();
        let entry = vm.prog.funs[&fun_id].gen_code(self).unwrap();
        self.funs.insert(fun_id, entry);

        *fun = self.stack.pop().unwrap();

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

        // Class ids come from the compiler and from the runtime itself,
        // so a missing class means we are at fault, not the running program
        if class.is_none() {
            panic!("internal error: could not find class with id={:?}", class_id);
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
    /// Returns `None` if the class has no such field
    pub fn get_slot_idx(&mut self, class_id: ClassId, field_name: &str) -> Option<usize>
    {
        self.with_class(class_id, |c| c.fields.get(field_name).copied())
    }

    /// List the field names of a class, for error reporting
    fn get_field_names(&mut self, class_id: ClassId) -> String
    {
        self.with_class(class_id, |c| {
            let mut names: Vec<&str> = c.fields.keys().map(|s| s.as_str()).collect();
            names.sort();
            names.join(", ")
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
            Object::alloc_size(num_slots),
            &mut []
        );

        Object::new(class_id, num_slots, &mut self.alloc)
    }

    /// Set the value of an object field on an object the runtime itself
    /// allocated, e.g. a UI event. The class and its fields are known here,
    /// so a failure means the runtime is at fault, not the running program.
    pub fn set_field(&mut self, obj: Value, field_name: &str, val: Value)
    {
        let obj = match obj.to_obj() {
            Some(obj) => obj,
            None => panic!("internal error: set_field on non-object value {:?}", obj)
        };

        match self.get_slot_idx(obj.class_id, field_name) {
            Some(slot_idx) => obj.set(slot_idx, val),
            None => panic!(
                "internal error: no field `{}` on class `{}`",
                field_name,
                self.get_class_name(obj.class_id)
            )
        }
    }

    /// Allocate/intern a constant string used by the runtime
    /// or present as a constant in the program
    pub fn intern_str(&mut self, str_const: &str) -> Value
    {
        self.gc_check(
            Str::alloc_size(str_const.len()),
            &mut []
        );

        // Note: for now this doesn't do interning but we
        // may choose to add this optimization later
        Str::new(str_const, &mut self.alloc)
    }

    /// Perform a garbage collection cycle
    pub fn gc_collect(&mut self, bytes_needed: usize, extra_roots: &mut [&mut Value])
    {
        // Collections can happen many times a second, so the reporting here,
        // its argument formatting, and the timing it needs all compile out
        // unless the `log_gc` feature is enabled.
        #[cfg(feature = "log_gc")]
        println!("Running GC cycle, {} bytes free", self.alloc.bytes_free());

        #[cfg(feature = "log_gc")]
        let start_time = crate::host::get_time_ms();

        // How big to make the to-space. A block costs its header plus its
        // own rounded size and nothing else, and each one is copied at
        // most once, so the copy can never come out larger than what it
        // copies. That makes what the copy needs exactly the bytes in use
        // plus what the allocation waiting on us wants.
        //
        // The heap is also sized from the live data below, and it has to
        // be grown for that here rather than after the fact, so take
        // whichever of the two is larger.
        let used_bytes = self.alloc.bytes_used();
        let to_space_bytes = std::cmp::max(
            ((used_bytes + bytes_needed) * 3) / 2,
            INIT_SIZE,
        );

        // Get an allocator to copy the data into. It still holds whatever
        // it had when it was last a heap, and its allocation point is the
        // high water mark of that: everything past it was never written
        // and is still zero. The copy overwrites every byte of what it
        // allocates, so only the rest has to be cleared, once we know how
        // far the copy got.
        let mut dst_alloc = match self.to_space.take() {
            Some(alloc) => alloc,
            None => Alloc::new()
        };
        let dirty_bytes = dst_alloc.bytes_used();

        // The to-space has to be empty for its reservation to be replaced
        dst_alloc.reset();
        dst_alloc.grow_reserve(to_space_bytes);

        // A replaced reservation is freshly mapped, so nothing in it is
        // stale any more. Growing only ever commits zeroed pages.
        let dirty_bytes = std::cmp::min(dirty_bytes, dst_alloc.mem_size());
        dst_alloc.grow(to_space_bytes);

        // Copy the roots into the new allocator, then everything they
        // reach. The from-space is discarded below, so the copier is free
        // to leave forwarding addresses behind in it.
        //
        // Roots are updated in place as they are forwarded, so unlike a
        // copy through a translation map this needs no second pass.
        let mut str_table = std::mem::take(&mut self.str_table);
        {
            let mut copier = Copier::new(&mut dst_alloc, &mut str_table);

            // Global variables
            for val in &mut self.globals {
                *val = copier.forward(*val);
            }

            // Values on the stack
            for val in &mut self.stack {
                *val = copier.forward(*val);
            }

            // Closures in the stack frames
            for frame in &mut self.frames {
                frame.fun = copier.forward(frame.fun);
            }

            // Heap values referenced in instructions
            for insn in &mut self.insns {
                match insn {
                    Insn::push { val } |
                    Insn::get_field { field: val, .. } |
                    Insn::set_field { field: val, .. } |
                    Insn::call_method { name: val, .. } |
                    Insn::call_method_pc { name: val, .. } |
                    Insn::call_method_host { name: val, .. } => {
                        *val = copier.forward(*val);
                    }

                    _ => {}
                }
            }

            // Extra roots supplied by the user
            for val in extra_roots {
                **val = copier.forward(**val);
            }

            // Sized as above, this cannot run out of space
            copier.run();

            #[cfg(feature = "log_gc")]
            println!(
                "GC copied {} blocks, {} bytes",
                thousands_sep(copier.num_blocks()),
                thousands_sep(dst_alloc.bytes_used()),
            );
        }
        self.str_table = str_table;

        // Size the heap from the live data we just measured, rather than
        // guessing from the old heap size. This lets the heap shrink again
        // when a program's live set gets smaller.
        let live_bytes = dst_alloc.bytes_used();
        let new_mem_size = std::cmp::max(
            ((live_bytes + bytes_needed) * 3) / 2,
            INIT_SIZE,
        );
        dst_alloc.shrink_to(new_mem_size);

        // Clear what the copy did not overwrite, so that the mutator
        // still allocates out of zeroed memory. Shrinking above released
        // its pages, which come back zeroed, so only what is left of the
        // old high water mark is worth touching.
        dst_alloc.zero_up_to(dirty_bytes);

        #[cfg(feature = "log_gc")]
        println!(
            "Heap size now {} bytes ({}% free)",
            thousands_sep(dst_alloc.mem_size()),
            100 * dst_alloc.bytes_free() / dst_alloc.mem_size(),
        );

        // Swap the old and new allocators. The from-space is left as it
        // is: keeping its allocation point is what tells the next cycle
        // how much of it holds stale bytes.
        std::mem::swap(&mut self.alloc, &mut dst_alloc);
        self.to_space = Some(dst_alloc);

        #[cfg(feature = "verify_gc")]
        crate::gc::verify_heap(&self.alloc);

        #[cfg(feature = "log_gc")]
        println!("GC time: {} ms", crate::host::get_time_ms() - start_time);
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

    /// Wrap an integer in a value, boxing it if it is too large to be
    /// a fixnum. Boxing allocates, so this may collect.
    #[inline(always)]
    pub fn int64(&mut self, val: i64) -> Value
    {
        match Value::try_fixnum(val) {
            Some(v) => v,
            None => self.box_int64(val),
        }
    }

    #[cold]
    #[inline(never)]
    fn box_int64(&mut self, val: i64) -> Value
    {
        self.gc_check(HEADER_SIZE + size_of::<i64>(), &mut []);
        self.alloc.heap_int64(val)
    }

    /// Wrap a double in a value, boxing it if it has no inline encoding
    #[inline(always)]
    pub fn float64(&mut self, val: f64) -> Value
    {
        match Value::try_flonum(val) {
            Some(v) => v,
            None => self.box_float64(val),
        }
    }

    #[cold]
    #[inline(never)]
    fn box_float64(&mut self, val: f64) -> Value
    {
        self.gc_check(HEADER_SIZE + size_of::<f64>(), &mut []);
        self.alloc.heap_float64(val)
    }

    /// Slow path for `add`: anything the fixnum fast path leaves over,
    /// which is boxed integers, floats and string concatenation
    #[cold]
    fn add_slow(&mut self, mut v0: Value, mut v1: Value) -> Result<Value, String>
    {
        if let (Some(a), Some(b)) = (v0.to_i64(), v1.to_i64()) {
            return match a.checked_add(b) {
                Some(r) => Ok(self.int64(r)),
                None => Err(int_op_error("add", b)),
            };
        }

        if v0.is_num() && v1.is_num() {
            let r = v0.num_as_f64() + v1.num_as_f64();
            return Ok(self.float64(r));
        }

        if v0.is_string() && v1.is_string() {
            let len = v0.as_string().len() + v1.as_string().len();

            // The operands are only reachable through us here, so they
            // have to be handed to the collector as roots
            self.gc_check(Str::alloc_size(len), &mut [&mut v0, &mut v1]);

            let cat = v0.as_str().to_owned() + v1.as_str();
            return Ok(Str::new(&cat, &mut self.alloc));
        }

        Err(format!("unsupported operand types for add: {:?} and {:?}", v0, v1))
    }

    /// Division always produces a float, whatever the operand types
    fn div_num(&mut self, v0: Value, v1: Value) -> Result<Value, String>
    {
        if v0.is_num() && v1.is_num() {
            let r = v0.num_as_f64() / v1.num_as_f64();
            return Ok(self.float64(r));
        }

        Err(format!("unsupported operand types for div: {:?} and {:?}", v0, v1))
    }

    num_slow_path!(sub_slow, "sub", checked_sub, -);
    num_slow_path!(mul_slow, "mul", checked_mul, *);
    num_slow_path!(modulo_slow, "modulo", checked_rem, %);

    int_slow_path!(div_int_slow, "div_int", |a: i64, b: i64| a.checked_div(b));
    int_slow_path!(bit_and_slow, "bit_and", |a: i64, b: i64| Some(a & b));
    int_slow_path!(bit_or_slow, "bit_or", |a: i64, b: i64| Some(a | b));
    int_slow_path!(bit_xor_slow, "bit_xor", |a: i64, b: i64| Some(a ^ b));
    int_slow_path!(lshift_slow, "lshift", |a: i64, b: i64| Some(a << b));
    int_slow_path!(rshift_slow, "rshift", |a: i64, b: i64| Some(a >> b));

    /// Call a host function
    /// Kept out of line so its arity dispatch doesn't bloat the interpreter loop
    #[inline(never)]
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

    /// Report a runtime error, printing the message along with a stack
    /// trace, then terminate the execution. The instruction name is empty
    /// for errors that don't come from executing an instruction.
    ///
    /// Marked cold so that the error paths in the interpreter loop, which
    /// call this at many sites, stay out of the way of the hot code
    #[cold]
    #[inline(never)]
    fn report_error(&self, insn_name: &str, msg: &str) -> !
    {
        eprintln!();

        if insn_name != "" {
            eprintln!("Runtime error while executing `{}` instruction:", insn_name);
        }

        // Print the error message to standard error
        eprintln!("{}", msg);
        eprintln!();

        // For each stack frame, from top to bottom
        for frame in self.frames.clone().into_iter().rev() {
            // A frame we can't identify shouldn't keep us from
            // reporting the error that got us here
            let fun_id = match frame.fun.to_fun_id() {
                Some(id) => id,
                None => {
                    eprintln!("<unknown function>");
                    continue;
                }
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
    }

    /// Call and execute a function in this actor
    pub fn call(&mut self, fun: Value, args: &[Value]) -> Value
    {
        assert!(self.stack.len() == 0);
        assert!(self.frames.len() == 0);

        if fun.to_fun_id().is_none() {
            self.report_error("", &format!("expected function value but got {:?}", fun));
        }

        // Push the arguments on the stack. Compiling below can collect,
        // and the stack is where the collector looks for them.
        for arg in args {
            self.stack.push(*arg);
        }

        // Get a compiled address for this function
        let mut fun = fun;
        let fun_entry = self.get_compiled_fun(&mut fun);
        let mut pc = fun_entry.entry_pc;

        if args.len() != fun_entry.num_params {
            self.report_error("", &format!(
                "function takes {} argument(s) but was called with {}",
                fun_entry.num_params,
                args.len()
            ));
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
        self.stack.resize(self.stack.len() + fun_entry.num_locals, Value::NIL);

        macro_rules! pop {
            () => { self.stack.pop().unwrap() }
        }

        macro_rules! push {
            ($val: expr) => { self.stack.push($val) }
        }

        macro_rules! push_bool {
            ($b: expr) => { push!(Value::bool_val($b)) }
        }

        // Fast path for two inline doubles: decode, apply and re-encode.
        // Falls through when either operand is not one, or when the
        // result is a double that has to be boxed.
        macro_rules! flonum_op {
            ($v0: expr, $v1: expr, $op: tt) => {
                if $v0.is_flonum() && $v1.is_flonum() {
                    if let Some(r) = Value::try_flonum($v0.as_flonum() $op $v1.as_flonum()) {
                        push!(r);
                        continue;
                    }
                }
            }
        }

        // Take the result of a slow path, reporting a type error the
        // same way the instruction itself would
        macro_rules! slow {
            ($insn_name: literal, $res: expr) => {
                match $res {
                    Ok(v) => v,
                    Err(msg) => error!($insn_name, "{}", msg),
                }
            }
        }

        // Arithmetic on the tagged words themselves. `$rhs` says how the
        // right operand enters: `raw` keeps its tag, which is what add
        // and sub want, and `as_fixnum` drops it, so that a product comes
        // out tagged exactly once.
        macro_rules! arith_insn {
            ($insn: literal, $checked_op: ident, $rhs: ident, $float_op: path, $slow_path: ident) => {{
                let v1 = pop!();
                let v0 = pop!();

                // A 64-bit overflow is exactly the case where the result
                // no longer fits in a fixnum
                if v0.is_fixnum() && v1.is_fixnum() {
                    if let Some(r) = (v0.raw() as i64).$checked_op(v1.$rhs() as i64) {
                        push!(Value::from_raw(r as u64));
                        continue;
                    }
                }

                if v0.is_flonum() && v1.is_flonum() {
                    let f = $float_op(v0.as_flonum(), v1.as_flonum());

                    if let Some(r) = Value::try_flonum(f) {
                        push!(r);
                        continue;
                    }
                }

                let r = slow!($insn, self.$slow_path(v0, v1));
                push!(r);
            }}
        }

        // Bitwise ops. Fixnum tags are zero, so the op keeps them that
        // way and the result needs no retagging.
        macro_rules! bitop_insn {
            ($insn: literal, $op: tt, $slow_path: ident) => {{
                let v1 = pop!();
                let v0 = pop!();

                if v0.is_fixnum() && v1.is_fixnum() {
                    push!(Value::from_raw(v0.raw() $op v1.raw()));
                    continue;
                }

                let r = slow!($insn, self.$slow_path(v0, v1));
                push!(r);
            }}
        }

        // Comparisons. Tagged fixnums order like the integers they hold,
        // and inline doubles are decoded and compared directly.
        macro_rules! cmp_insn {
            ($insn: literal, $op: tt, $slow_path: ident) => {{
                let v1 = pop!();
                let v0 = pop!();

                if v0.is_fixnum() && v1.is_fixnum() {
                    push_bool!((v0.raw() as i64) $op (v1.raw() as i64));
                } else if v0.is_flonum() && v1.is_flonum() {
                    push_bool!(v0.as_flonum() $op v1.as_flonum());
                } else {
                    push_bool!(slow!($insn, $slow_path(v0, v1)));
                }
            }}
        }

        // Set up a new frame for a function call
        macro_rules! call_fun {
            ($fun: expr, $argc: expr) => {{
                if $argc as usize > self.stack.len() - bp {
                    error!("not enough call arguments on stack");
                }

                // The callee can be a closure, and compiling below can
                // collect, so it is held where it can be rooted
                let mut fun_val = $fun;

                let fun_id = match fun_val.to_fun_id() {
                    Some(id) => id,

                    None => match fun_val.to_host_fn() {
                        Some(f) => {
                            match self.call_host(f, $argc.into()) {
                                Err(msg) => error!("{}", msg),
                                Ok(()) => continue
                            }
                        }
                        None => error!("call to non-function value: `{:?}`", fun_val)
                    }
                };

                // Get a compiled address for this function
                let fun_entry = self.get_compiled_fun(&mut fun_val);

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
                    fun: fun_val,
                    prev_bp: bp,
                    ret_addr: pc,
                });

                // The base pointer will point at the first local
                bp = self.stack.len();
                pc = fun_entry.entry_pc;

                // Allocate stack slots for the local variables
                self.stack.resize(self.stack.len() + fun_entry.num_locals, Value::NIL);

                fun_entry
            }}
        }

        // Handle a runtime error
        // Print debug information including a stack trace
        // and terminate the execution
        macro_rules! error {
            ($insn_name: literal, $format_str:literal $(, $arg:expr)* $(,)?) => {{
                // The message is formatted first because the arguments may
                // need to borrow the actor
                let msg = format!($format_str $(, $arg)*);
                self.report_error($insn_name, &msg);
            }};

            ($format_str:literal $(, $arg:expr)* $(,)?) => {
                error!("", $format_str $(, $arg)*)
            };
        }

        loop
        {
            // Every compiled function ends with a ret instruction, so
            // execution can never run past the end of the insn stream.
            // That makes the load in bounds and keeps the increment below
            // the length, so neither needs to be checked here.
            debug_assert!(pc < self.insns.len());
            let insn = unsafe { *self.insns.get_unchecked(pc) };
            pc = unsafe { pc.unchecked_add(1) };
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

                    if val.is_undef() {
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

                Insn::add => arith_insn!("add", checked_add, raw, f64::add, add_slow),
                Insn::sub => arith_insn!("sub", checked_sub, raw, f64::sub, sub_slow),
                Insn::mul => arith_insn!("mul", checked_mul, as_fixnum, f64::mul, mul_slow),

                // Division always produces a float
                // Division by zero produces an infinity (this is intentional)
                Insn::div => {
                    let v1 = pop!();
                    let v0 = pop!();

                    flonum_op!(v0, v1, /);

                    let r = slow!("div", self.div_num(v0, v1));
                    push!(r);
                }

                // Integer division
                // Division by zero will cause a panic (this is intentional)
                Insn::div_int => {
                    let v1 = pop!();
                    let v0 = pop!();

                    // Dividing shrinks the magnitude, so the quotient
                    // fits unless it is the one case that overflows
                    if v0.is_fixnum() && v1.is_fixnum() {
                        if let Some(q) = v0.as_fixnum().checked_div(v1.as_fixnum()) {
                            if let Some(r) = Value::try_fixnum(q) {
                                push!(r);
                                continue;
                            }
                        }
                    }

                    let r = slow!("div_int", self.div_int_slow(v0, v1));
                    push!(r);
                }

                // Division by zero will cause a panic (this is intentional)
                Insn::modulo => {
                    let v1 = pop!();
                    let v0 = pop!();

                    // A remainder is smaller than its divisor, so it
                    // always fits back into a fixnum
                    if v0.is_fixnum() && v1.is_fixnum() {
                        if let Some(rem) = v0.as_fixnum().checked_rem(v1.as_fixnum()) {
                            push!(Value::fixnum(rem));
                            continue;
                        }
                    }

                    flonum_op!(v0, v1, %);

                    let r = slow!("modulo", self.modulo_slow(v0, v1));
                    push!(r);
                }

                // Add a constant int64 value
                Insn::add_i64 { val } => {
                    // The constant fits a fixnum, so the tagged words add
                    let cst = Value::fixnum(val);

                    let top = match self.stack.last_mut() {
                        Some(top) => top,
                        None => error!("add_i64", "stack is empty"),
                    };

                    if top.is_fixnum() {
                        if let Some(sum) = (top.raw() as i64).checked_add(cst.raw() as i64) {
                            *top = Value::from_raw(sum as u64);
                            continue;
                        }
                    }

                    let v0 = pop!();
                    let r = slow!("add_i64", self.add_slow(v0, cst));
                    push!(r);
                }

                Insn::bit_or => bitop_insn!("bit_or", |, bit_or_slow),
                Insn::bit_and => bitop_insn!("bit_and", &, bit_and_slow),
                Insn::bit_xor => bitop_insn!("bit_xor", ^, bit_xor_slow),

                // Integer left shift
                Insn::lshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    // The shift can push bits out of fixnum range, so the
                    // result has to be checked rather than retagged
                    if v0.is_fixnum() && v1.is_fixnum() {
                        let shift = v1.as_fixnum();

                        if shift >= 0 && shift < 62 {
                            if let Some(r) = Value::try_fixnum(v0.as_fixnum() << shift) {
                                push!(r);
                                continue;
                            }
                        }
                    }

                    let r = slow!("lshift", self.lshift_slow(v0, v1));
                    push!(r);
                }

                // Integer right shift
                Insn::rshift => {
                    let v1 = pop!();
                    let v0 = pop!();

                    // Shifting a fixnum right keeps it in range, so only
                    // the bits the tag occupies have to be cleared
                    if v0.is_fixnum() && v1.is_fixnum() {
                        let shift = v1.as_fixnum();

                        if shift >= 0 && shift < 62 {
                            push!(Value::fixnum(v0.as_fixnum() >> shift));
                            continue;
                        }
                    }

                    let r = slow!("rshift", self.rshift_slow(v0, v1));
                    push!(r);
                }

                Insn::lt => cmp_insn!("lt", <, cmp_lt),
                Insn::le => cmp_insn!("le", <=, cmp_le),
                Insn::gt => cmp_insn!("gt", >, cmp_gt),
                Insn::ge => cmp_insn!("ge", >=, cmp_ge),

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

                    match v0.to_bool() {
                        Some(b) => push_bool!(!b),
                        None => error!("not", "unsupported type in logical not {:?}", v0)
                    }
                }

                // Create a new closure
                Insn::clos_new { fun_id, num_slots } => {
                    let num_slots = num_slots as usize;

                     self.gc_check(
                        Closure::alloc_size(num_slots),
                        &mut [],
                    );

                    let clos = Closure::new(fun_id, num_slots, &mut self.alloc);
                    push!(clos);
                }

                // Set a closure slot
                Insn::clos_set { idx } => {
                    let val = pop!();
                    let clos = pop!();

                    match clos.to_clos() {
                        Some(clos) => clos.set(idx as usize, val),
                        None => error!("clos_set", "expected closure")
                    }
                }

                // Get a closure slot for the function currently executing
                Insn::clos_get { idx } => {
                    let fun = self.frames[self.frames.len() - 1].fun;

                    let val = match fun.to_clos() {
                        Some(clos) => clos.get(idx as usize),
                        None => error!("clos_get", "not a closure")
                    };

                    if val.is_undef() {
                        error!("clos_get", "executing uninitialized closure");
                    }

                    push!(val);
                }

                // Create a new mutable cell
                Insn::cell_new => {
                     self.gc_check(
                        HEADER_SIZE + std::mem::size_of::<Value>(),
                        &mut [],
                    );

                    let p_cell = self.alloc.alloc(Value::NIL, Tag::Cell);
                    push!(Value::cell(p_cell));
                }

                // Set the value stored in a mutable cell
                Insn::cell_set => {
                    let cell = pop!();
                    let val = pop!();

                    match cell.to_cell() {
                        Some(p_cell) => *p_cell = val,
                        None => error!("cell_set", "expected cell")
                    };
                }

                // Get the value stored in a mutable cell
                Insn::cell_get => {
                    let cell = pop!();

                    let val = match cell.to_cell() {
                        Some(p_cell) => *p_cell,
                        None => error!("cell_get", "invalid cell in cell_get")
                    };

                    push!(val);
                }

                // Create new empty dictionary
                Insn::dict_new => {
                    self.gc_check(
                        Dict::alloc_size(0),
                        &mut []
                    );
                    push!(Dict::with_capacity(0, &mut self.alloc))
                }

                // Set object field
                Insn::set_field { field, class_id, slot_idx } => {
                    let mut val = pop!();
                    let mut obj = pop!();

                    if let Some(obj) = obj.to_obj() {
                        if class_id == obj.class_id {
                            obj.set(slot_idx as usize, val);
                        } else {
                            let slot_idx = match self.get_slot_idx(obj.class_id, field.as_str()) {
                                Some(slot_idx) => slot_idx,
                                None => error!(
                                    "set_field",
                                    "class `{}` has no field `{}`, known fields are: {}",
                                    self.get_class_name(obj.class_id),
                                    field.as_str(),
                                    self.get_field_names(obj.class_id),
                                )
                            };
                            let class_id = obj.class_id;

                            // Update the cache
                            self.insns[pc - 1] = Insn::set_field {
                                field,
                                class_id,
                                slot_idx: slot_idx as u32,
                            };

                            obj.set(slot_idx, val);
                        }
                    }
                    else if obj.is_dict() {
                        let alloc_size = obj.as_dict().will_allocate();

                        // The field name is reachable from the instruction
                        // stream, which the collector updates, so it has to
                        // be read back after the check
                        let mut field = field;
                        self.gc_check(
                            alloc_size,
                            &mut [&mut obj, &mut val, &mut field]
                        );

                        obj.as_dict().set(field.as_string() as *const Str, val, &mut self.alloc);
                    }
                    else {
                        error!("set_field", "set_field on non-object/dict value")
                    }
                }

                // Allocate a new class instance and call
                // the constructor for the given class
                Insn::new { class_id, argc } => {
                    let num_slots = self.get_num_slots(class_id);

                    self.gc_check(
                        Object::alloc_size(num_slots),
                        &mut [],
                    );

                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc);

                    // If a constructor method is present
                    let init_fun = self.get_method(class_id, "init");
                    if let Some(fun_id) = init_fun {
                        let this_pc = pc - 1;

                        // The self value should be first argument to the constructor
                        // The constructor also returns the allocated object
                        self.stack.insert(self.stack.len() - argc as usize, obj_val);
                        let ctor_entry = call_fun!(Value::fun(fun_id), argc + 1);

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
                        Object::alloc_size(num_slots),
                        &mut [],
                    );

                    // Allocate the object
                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc);

                    // The self value should be first argument to the constructor
                    // The constructor also returns the allocated object
                    self.stack.insert(self.stack.len() - argc as usize, obj_val);

                    // We add an extra argument for the self value
                    self.frames.push(StackFrame {
                        argc: argc + 1,
                        fun: Value::fun(fun_id),
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                    pc = ctor_pc as usize;

                    // Allocate stack slots for the local variables
                    self.stack.resize(self.stack.len() + num_locals as usize, Value::NIL);
                }

                Insn::instanceof { class_id } => {
                    // Check that the class id matches
                    let val = pop!();
                    let id = crate::runtime::get_class_id(val);
                    push_bool!(id == class_id);
                }

                // Get object field
                Insn::get_field { field, class_id, slot_idx } => {
                    let obj = pop!();

                    if !obj.is_heap() {
                        error!("get_field", "get_field on non-object value {:?}", obj);
                    }

                    // The block header says what the value points at, so
                    // one load and one switch settle the type
                    let val = match obj.heap_tag() {
                        Tag::Object => {
                            let obj = obj.as_obj();

                            // If the class id doesn't match the cache, update it
                            let val = if class_id == obj.class_id {
                                obj.get(slot_idx as usize)
                            } else {
                                let slot_idx = match self.get_slot_idx(obj.class_id, field.as_str()) {
                                    Some(slot_idx) => slot_idx,
                                    None => error!(
                                        "get_field",
                                        "class `{}` has no field `{}`, known fields are: {}",
                                        self.get_class_name(obj.class_id),
                                        field.as_str(),
                                        self.get_field_names(obj.class_id),
                                    )
                                };
                                let class_id = obj.class_id;

                                // Update the cache
                                self.insns[pc - 1] = Insn::get_field {
                                    field,
                                    class_id,
                                    slot_idx: slot_idx as u32,
                                };

                                obj.get(slot_idx as usize)
                            };

                            if val.is_undef() {
                                error!("get_field", "object field not initialized `{}`", field.as_str());
                            }

                            val
                        }

                        Tag::Dict => {
                            let key = field.as_str();

                            match obj.as_dict().get(key) {
                                Some(v) => v,
                                None => error!("get_field", "key '{}' not found in dict", key)
                            }
                        }

                        Tag::Array => {
                            match field.as_str() {
                                "len" => Value::fixnum(obj.as_arr().len() as i64),
                                _ => error!("get_field", "field not found on array")
                            }
                        }

                        Tag::ByteArray => {
                            match field.as_str() {
                                "len" => Value::fixnum(obj.as_ba().num_bytes() as i64),
                                _ => error!("get_field", "field not found on bytearray")
                            }
                        }

                        Tag::Str => {
                            match field.as_str() {
                                "len" => Value::fixnum(obj.as_str().len() as i64),
                                _ => error!("get_field", "field not found on string")
                            }
                        }

                        _ => error!("get_field", "get_field on non-object value {:?}", obj)
                    };

                    push!(val);
                }

                Insn::get_index => {
                    let idx = pop!();
                    let arr = pop!();

                    if !arr.is_heap() {
                        error!("get_index", "expected array or dict type in get_index");
                    }

                    let val = match arr.heap_tag() {
                        Tag::Array => {
                            let idx = unwrap_usize!(idx, "get_index");
                            arr.as_arr().get(idx)
                        }

                        Tag::ByteArray => {
                            let idx = unwrap_usize!(idx, "get_index");
                            Value::from(arr.as_ba().get::<u8>(idx))
                        }

                        Tag::Dict => {
                            let key = unwrap_str!(idx, "get_index");

                            match arr.as_dict().get(key) {
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

                    if !arr.is_heap() {
                        error!("set_index", "expected array or dict type");
                    }

                    match arr.heap_tag() {
                        Tag::Array => {
                            let elem_idx = unwrap_usize!(idx, "set_index");
                            arr.as_arr().set(elem_idx, val);
                        }

                        Tag::ByteArray => {
                            let byte_idx = unwrap_usize!(idx, "set_index");
                            let b = unwrap_u8!(val, "set_index");
                            arr.as_ba().set::<u8>(byte_idx, b);
                        }

                        Tag::Dict => {
                            if !idx.is_string() {
                                error!("set_index", "expected string key but got {:?}", idx);
                            }

                            let alloc_size = arr.as_dict().will_allocate();
                            self.gc_check(
                                alloc_size,
                                &mut [&mut arr, &mut idx, &mut val],
                            );

                            let key = idx.as_string() as *const Str;
                            arr.as_dict().set(key, val, &mut self.alloc);
                        }

                        _ => error!("set_index", "expected array or dict type")
                    };
                }

                // Create new empty array
                Insn::arr_new { capacity } => {
                    let capacity = capacity as usize;

                    self.gc_check(
                        Array::alloc_size(capacity),
                        &mut [],
                    );

                    push!(Array::with_capacity(capacity, &mut self.alloc))
                }

                // Append an element at the end of an array
                // This instruction is used to construct array literals
                Insn::arr_push => {
                    let val = pop!();
                    let array = pop!();
                    crate::array::array_push(self, array, val).unwrap();
                }

                // Clone a bytearray
                Insn::ba_clone => {
                    let mut val = pop!();
                    let ba = unwrap_ba!(val, "ba_clone");

                    self.gc_check(
                        ByteArray::alloc_size(ba.num_bytes()),
                        &mut [&mut val],
                    );

                    let ba_clone = val.as_ba().clone(&mut self.alloc);
                    push!(ba_clone);
                }

                // Jump if true
                Insn::if_true { target_ofs } => {
                    let v = pop!();

                    if v.is_true() {
                        pc = ((pc as i64) + (target_ofs as i64)) as usize;
                    } else if !v.is_false() {
                        error!("if_true", "if_true instruction only accepts boolean values");
                    }
                }

                // Jump if false
                Insn::if_false { target_ofs } => {
                    let v = pop!();

                    if v.is_false() {
                        pc = ((pc as i64) + (target_ofs as i64)) as usize;
                    } else if !v.is_true() {
                        error!("if_false", "if_false instruction only accepts boolean values");
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
                    let fun_entry = call_fun!(Value::fun(fun_id), argc);

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
                        fun: Value::fun(fun_id),
                        prev_bp: bp,
                        ret_addr: pc,
                    });

                    // The base pointer will point at the first local
                    bp = self.stack.len();
                    pc = entry_pc as usize;

                    // Allocate stack slots for the local variables
                    self.stack.resize(self.stack.len() + num_locals as usize, Value::NIL);
                }

                // Call a method with a known name
                // call_method (self, arg0, ..., argN)
                Insn::call_method { name, argc } => {
                    let self_val = self.stack[self.stack.len() - (1 + argc as usize)];

                    match self_val.to_obj() {
                        Some(obj) => {
                            // Read before the call below, which can compile
                            // the callee and collect, leaving obj behind
                            let class_id = obj.class_id;

                            let fun_id = match self.get_method(class_id, name.as_str()) {
                                None => error!(
                                    "call to method `{}`, not found on class `{}`",
                                    name.as_str(),
                                    self.get_class_name(class_id)
                                ),
                                Some(fun_id) => fun_id,
                            };

                            let this_pc = pc - 1;
                            let fun_entry = call_fun!(Value::fun(fun_id), argc + 1);

                            // Patch this instruction to avoid the method lookup
                            // next time. The name is read back out of the
                            // instruction rather than reused, since a collection
                            // in the call above would have moved the string.
                            let name = match self.insns[this_pc] {
                                Insn::call_method { name, .. } => name,
                                _ => panic!("call_method instruction expected")
                            };

                            self.insns[this_pc] = Insn::call_method_pc {
                                name,
                                argc: argc.try_into().unwrap(),
                                class_id,
                                entry_pc: fun_entry.entry_pc.try_into().unwrap(),
                                fun_id,
                                num_locals: fun_entry.num_locals.try_into().unwrap(),
                            };
                        }

                        // Call to a primitive e.g. Int64/Float64/immediate (not an object)
                        None => {
                            // Lookup the method to call
                            let fun = crate::runtime::get_method(self_val, name.as_str());

                            let host_fn = match fun.to_host_fn() {
                                None => error!("call to unknown method `{}`", name.as_str()),
                                Some(f) => f,
                            };

                            if argc as usize + 1 > self.stack.len() - bp {
                                error!("not enough call arguments on stack");
                            }

                            // Patch this instruction to avoid the method
                            // lookup next time. Bools and classes are left
                            // alone because their methods depend on more
                            // than the type tag. Nothing has allocated since
                            // the name was read, so it hasn't moved.
                            let type_tag = self_val.type_of();
                            if !matches!(type_tag, Type::Bool | Type::Class) {
                                self.insns[pc - 1] = Insn::call_method_host {
                                    name,
                                    argc,
                                    type_tag,
                                    host_fn,
                                };
                            }

                            if let Err(msg) = self.call_host(host_fn, argc as usize + 1) {
                                error!("{}", msg);
                            }
                        }
                    };
                }

                Insn::call_method_pc { name, argc, class_id, entry_pc, fun_id, num_locals } => {
                    let self_val = self.stack[self.stack.len() - (1 + argc as usize)];

                    // Guard that self is an object with a matching class id
                    if let Some(obj) = self_val.to_obj() {
                        if obj.class_id == class_id {
                            let argc: u8 = argc.into();
                            self.frames.push(StackFrame {
                                argc: argc + 1,
                                fun: Value::fun(fun_id),
                                prev_bp: bp,
                                ret_addr: pc,
                            });

                            // The base pointer will point at the first local
                            bp = self.stack.len();
                            pc = entry_pc as usize;

                            // Allocate stack slots for the local variables
                            self.stack.resize(self.stack.len() + num_locals as usize, Value::NIL);

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

                Insn::call_method_host { name, argc, type_tag, host_fn } => {
                    // Checked when the instruction was patched in
                    debug_assert!(argc as usize + 1 <= self.stack.len() - bp);
                    let self_val = self.stack[self.stack.len() - (1 + argc as usize)];

                    // Guard that self still has the type the method was found on
                    if self_val.type_of() == type_tag {
                        if let Err(msg) = self.call_host(host_fn, argc as usize + 1) {
                            error!("{}", msg);
                        }

                        continue;
                    }

                    // The guard failed, deoptimize this instruction and try again
                    pc -= 1;
                    self.insns[pc] = Insn::call_method { name, argc };
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

                    // The pop below already panics on an empty frame stack
                    debug_assert!(self.frames.len() > 0);
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
        let msg_alloc = Alloc::for_messages();

        // Build the new actor's heap here and copy its function and
        // globals directly into it. Routing them through the message
        // allocator instead would cap them at its fixed size, and would
        // leave the actor referencing memory outside its own heap.
        // What we copy over cannot come out larger than the parent's
        // heap, and the new heap is sized from the live data below, so
        // this covers both
        let mut alloc = Alloc::new();
        let heap_bytes = std::cmp::max(
            (parent.alloc.bytes_used() * 3) / 2,
            INIT_SIZE,
        );
        alloc.grow_reserve(heap_bytes);
        alloc.grow(heap_bytes);

        // Copy the function/closure and the parent's globals. This reads
        // out of the parent's live heap, so the forwarding addresses left
        // behind have to be taken back out afterwards.
        let mut globals = parent.globals.clone();
        let mut str_table = std::mem::take(&mut parent.str_table);
        let mut undo_log = std::mem::take(&mut parent.undo_log);

        let mut copier = Copier::with_undo(
            &mut alloc,
            &mut str_table,
            &mut undo_log
        );
        let fun = copier.forward(fun);
        for val in &mut globals {
            *val = copier.forward(*val);
        }
        copier.run();

        undo_forwarding(&mut undo_log);
        parent.str_table = str_table;
        parent.undo_log = undo_log;

        // Give the new heap room to run, but not the whole upper bound
        let live_bytes = alloc.bytes_used();
        alloc.shrink_to(std::cmp::max((live_bytes * 3) / 2, INIT_SIZE));

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
                alloc,
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
                actor.report_error("", &format!(
                    "actor cannot return heap-allocated value of type {:?}, \
                    only primitive values can be returned",
                    ret_val.type_of()
                ));
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
        let handle = vm.threads.remove(&tid).unwrap();
        vm.actor_txs.remove(&tid).unwrap();
        drop(vm);

        // Note: there is no need to copy data when joining,
        // because the actor sending the data is done running
        match handle.join() {
            Ok(val) => val,

            // The actor reported its own error before dying, so there is
            // nothing useful to add here
            Err(_) => panic!("actor with id {} terminated with an error", tid)
        }
    }

    // Call a function in the main actor
    pub fn call(vm: &mut Arc<Mutex<VM>>, fun_id: FunId, args: Vec<Value>) -> Value
    {
        let vm_mutex = vm.clone();

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::sync_channel::<Message>(1024);

        // Create an allocator to send messages to the actor
        let msg_alloc = Arc::new(Mutex::new(Alloc::for_messages()));

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
        let globals = vec![Value::UNDEF; vm_ref.prog.num_globals as usize];

        drop(vm_ref);

        let mut actor = Actor::new(
            actor_id,
            None,
            vm_mutex,
            Alloc::new(),
            msg_alloc,
            queue_rx,
            globals,
        );

        actor.call(Value::fun(fun_id), &args)
    }

    // Compile every function in a program without running it, which is
    // what --no-exec does to check that code generation works
    pub fn compile_all(prog: Program)
    {
        let fun_ids: Vec<FunId> = prog.funs.keys().copied().collect();
        let vm = VM::new(prog);
        let vm_mutex = vm.clone();

        // Create a message queue for the actor
        let (queue_tx, queue_rx) = mpsc::sync_channel::<Message>(1024);

        // Create an allocator to send messages to the actor
        let msg_alloc = Arc::new(Mutex::new(Alloc::for_messages()));

        // Info needed to send the actor a message
        let actor_tx = ActorTx {
            sender: queue_tx,
            msg_alloc: Arc::downgrade(&msg_alloc),
        };

        // Assign an actor id
        // Store the queue endpoints on the VM
        let mut vm_ref = vm.lock().unwrap();
        let actor_id = vm_ref.next_actor_id;
        vm_ref.next_actor_id += 1;
        vm_ref.actor_txs.insert(actor_id, actor_tx);

        // Initialize the global slots
        let globals = vec![Value::UNDEF; vm_ref.prog.num_globals as usize];

        drop(vm_ref);

        let mut actor = Actor::new(
            actor_id,
            None,
            vm_mutex,
            Alloc::new(),
            msg_alloc,
            queue_rx,
            globals,
        );

        for fun_id in fun_ids {
            actor.get_compiled_fun(&mut Value::fun(fun_id));
        }
    }

    /// Send a message to an actor without copying it to its message allocator
    pub fn send_nocopy(&self, actor_id: u64, msg: Value, size: usize) -> Result<(), ()>
    {
        let actor_tx = self.actor_txs.get(&actor_id).ok_or(())?;
        actor_tx.sender.send(Message { sender: 0, msg, size }).map_err(|_| ())
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

    fn flonum(v: f64) -> Value
    {
        Value::try_flonum(v).unwrap()
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
        assert!(size_of::<Value>() == 8);

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
        eval_eq("", Value::NIL);
    }

    #[test]
    fn simple_exprs()
    {
        eval_eq("return 77;", Value::fixnum(77));
        eval_eq("return -77;", Value::fixnum(-77));
        eval_eq("return 1 + 5;", Value::fixnum(6));
        eval_eq("return 5 - 3;", Value::fixnum(2));
        eval_eq("return 2 * 3 + 4;", Value::fixnum(10));
        eval_eq("return 5 + 2 * -2;", Value::fixnum(1));
        eval_eq("return 2 * 2 - 1;", Value::fixnum(3));
    }

    #[test]
    fn if_else()
    {
        eval_eq("if (true) return 1; return 2;", Value::fixnum(1));
        eval_eq("if (false) return 1; return 2;", Value::fixnum(2));
        eval_eq("if (true) return 77; else return 88;", Value::fixnum(77));
        eval_eq("if (false) return 77; else return 88;", Value::fixnum(88));
        eval_eq("if (3 < 5) return 1; return 2;", Value::fixnum(1));
    }

    #[test]
    fn logical_and()
    {
        eval_eq("if (false && false) return 1; else return 0;", Value::fixnum(0));
        eval_eq("if (false && true) return 1; else return 0;", Value::fixnum(0));
        eval_eq("if (true && false) return 1; else return 0;", Value::fixnum(0));
        eval_eq("if (true && true) return 1; else return 0;", Value::fixnum(1));
    }

    #[test]
    fn logical_or()
    {
        eval_eq("if (false || false) return 1; else return 0;", Value::fixnum(0));
        eval_eq("if (false || true) return 1; else return 0;", Value::fixnum(1));
        eval_eq("if (true || false) return 1; else return 0;", Value::fixnum(1));
        eval_eq("if (true || true) return 1; else return 0;", Value::fixnum(1));
    }

    #[test]
    fn let_expr()
    {
        eval_eq("let x = 1; return x;", Value::fixnum(1));
        eval_eq("let var x = 1; return x;", Value::fixnum(1));
        eval_eq("let x = 1; let y = 2; return x + y;", Value::fixnum(3));
    }

    #[test]
    fn inc_dec()
    {
        eval_eq("let var x = 10; --x; return x;", Value::fixnum(9));
    }

    #[test]
    fn assign()
    {
        eval_eq("let var x = 1; x = 2; return x;", Value::fixnum(2));
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
        eval_eq("class F {} let o1 = F(); let o2 = F(); return o1 == o2;", Value::FALSE);
        eval_eq("class F {} let o1 = F(); let o2 = F(); return o1 != o2;", Value::TRUE);

        // Integer comparisons
        eval_eq("return 3 <= 5;", Value::TRUE);

        // String comparison
        eval_eq("return 'foo' == 'bar';", Value::FALSE);
        eval_eq("return 'foo' == 'foo';", Value::TRUE);
        eval_eq("return 'foo' != 'foo';", Value::FALSE);
    }

    #[test]
    fn ternary_expr()
    {
        eval_eq("return true? 1:2;", Value::fixnum(1));
        eval_eq("return false? 1:2;", Value::fixnum(2));
        eval_eq("let b = (1 < 5)? 1:2; return b;", Value::fixnum(1));
    }

    #[test]
    fn scope_shadow()
    {
        eval("let x = 1; { let x = x + 1; assert(x==2); } assert(x==1);");
    }

    #[test]
    fn while_loop()
    {
        eval_eq("let x = 1; while (false) {} return x;", Value::fixnum(1));
        eval_eq("let var x = 1; while (x < 10) { x = x + 1; } return x;", Value::fixnum(10));
    }

    #[test]
    fn for_loop()
    {
        eval("for (;;) break;");
        eval_eq("let x = 1; for (let var x = 0; x < 10; ++x) {} return x;", Value::fixnum(1));
        eval_eq("let var x = 0; for (let var i = 0; i < 10; ++i) { x = x + 2; } return x;", Value::fixnum(20));
        eval_eq("let var x = 0; for (let var i = 0; i < 10; ++i) { ++x; assert(x < 11); continue; } return x;", Value::fixnum(10));
    }

    #[test]
    fn fun_call()
    {
        eval_eq("fun f() { return 7; } return f();", Value::fixnum(7));
        eval_eq("fun f(x) { return x + 1; } return f(7);", Value::fixnum(8));
        eval_eq("fun f(a, b) { return a - b; } return f(7, 2);", Value::fixnum(5));

        // Global variable read
        eval_eq("let g = 3; fun f() { return g+1; } return f();", Value::fixnum(4));

        // Function calling another function
        eval_eq("fun a() { return 8; } fun b() { return a(); } return b();", Value::fixnum(8));
    }

    #[test]
    fn ret_clos()
    {
        // Function returning a closure
        eval_eq("fun a() { fun b() { return 33; } return b; } let f = a(); return f();", Value::fixnum(33));
    }

    #[test]
    fn capture_local()
    {
        // Captured function argument
        eval_eq("fun f(n) { return || n+1; } let g = f(7); return g();", Value::fixnum(8));

        // Capture local variable
        eval_eq("fun f(n) { let m = n+1; return || m+1; } let g = f(3); return g();", Value::fixnum(5));
        eval_eq("fun f(n) { let m = n+1; return |x| m+x; } let g = f(3); return g(4);", Value::fixnum(8));
    }

    #[test]
    fn counter_clos()
    {
        // Read mutable captured variable
        eval_eq("fun f() { let var n = 0; return || n; } let c = f(); return c();", Value::fixnum(0));

        // Write mutable captured variable
        eval_eq("fun f() { let var n = 0; return || n = 1; } let c = f(); return c();", Value::fixnum(1));

        // Counter
        eval_eq("fun f() { let var n = 0; return || ++n; } let c = f(); c(); return c();", Value::fixnum(2));
    }

    #[test]
    fn fact()
    {
        // Recursive factorial function
        eval_eq("fun f(n) { if (n < 2) return 1; return n * f(n-1); } return f(6);", Value::fixnum(720));
    }

    #[test]
    fn fib()
    {
        // Recursive fibonacci function
        eval_eq("fun f(n) { if (n < 2) return n; return f(n-1) + f(n-2); } return f(10);", Value::fixnum(55));
    }

    #[test]
    fn call_ahead()
    {
        // Call a function before its definition
        eval_eq("fun a() { return b(); } fun b() { return 7; } return a();", Value::fixnum(7));
    }

    #[test]
    fn mutual_rec()
    {
        // Mutual recursion
        eval_eq("fun a(n) { return b(n-1); } fun b(n) { if (n<1) return 0; return a(n-1); } return a(8);", Value::fixnum(0));
    }

    #[test]
    fn host_call()
    {
        eval_eq("return $actor_id();", Value::fixnum(0));
        eval_eq("return $actor_parent();", Value::NIL);
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
            Value::fixnum(77)
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
            Value::fixnum(1337)
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
            Value::fixnum(33)
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
            Value::TRUE
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
        eval_eq("return 77.0 instanceof Float64;", Value::TRUE);
        eval_eq("return 4.0 + 1.0;", flonum(5.0));
        eval_eq("return 6.0 / 2.0;", flonum(3.0));
        eval_eq("return 4.0.sqrt();", flonum(2.0));
    }

    #[test]
    fn boxed_nums()
    {
        // Integers past the fixnum range are boxed, and behave the same
        eval_eq("return 4611686018427387903 + 1 == 4611686018427387904;", Value::TRUE);
        eval_eq("return -4611686018427387904 - 1 == -4611686018427387905;", Value::TRUE);
        eval_eq("return (1 << 62) >> 62;", Value::fixnum(1));
        eval_eq("return (4611686018427387904 & 0xFF) == 0;", Value::TRUE);
        eval_eq("let x = 4611686018427387904; return x.to_s().len;", Value::fixnum(19));

        // Doubles with no inline encoding are boxed
        eval_eq("let x = 1e300; return x * 1e-300;", flonum(1.0));
        eval_eq("return 1e-300 == 1e-300;", Value::TRUE);
        eval_eq("return 1e300 > 1e-300;", Value::TRUE);

        // Boxed and inline representations of the same number are equal
        eval_eq("return 4611686018427387904 == 4611686018427387904.0;", Value::TRUE);
    }

    #[test]
    #[should_panic]
    fn int_overflow()
    {
        eval("return 4611686018427387904 * 4;");
    }

    #[test]
    fn strings()
    {
        eval_eq("return ''.len;", Value::fixnum(0));
        eval_eq("return 'hello'.len;", Value::fixnum(5));
        eval_eq("let s1 = 'foo'; let s2 = 'bar'; return s1 + s2 == 'foobar';", Value::TRUE);
    }

    #[test]
    fn dicts()
    {
        eval("let o = {};");
        eval("let o = { x: 1, y: 2 };");
        eval_eq("let o = { x: 1, y: 2 }; return o.x;", Value::fixnum(1));
        eval_eq("let o = { x: 1, y: 2 }; return o.x + o.y;", Value::fixnum(3));
        eval_eq("let o = { 'x': 77 }; return o.x;", Value::fixnum(77));
        eval_eq("let o = { 'foo bar': 5 }; return o['foo bar'];", Value::fixnum(5));
        eval_eq("let o = { x:5 }; o['x'] = 3; return o.x;", Value::fixnum(3));
        eval_eq("let o = { x:5 }; return o.has('x');", Value::TRUE);
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
        eval_eq("let a = [11, 22, 33]; return a[0];", Value::fixnum(11));
        eval_eq("let a = [11, 22, 33]; return a[2];", Value::fixnum(33));
        eval_eq("let a = [11, 22, 33]; a[2] = 44; return a[2];", Value::fixnum(44));
        eval_eq("let a = [11, 22, 33]; return a.len;", Value::fixnum(3));
        eval_eq("let a = [11, 22, 33]; a.push(44); return a.len;", Value::fixnum(4));
        eval_eq("let a = Array.with_size(5, nil); return a.len;", Value::fixnum(5));
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

        eval_eq("class Foo {} return Foo() != nil;", Value::TRUE);
        eval_eq("class Foo { init(s) {} } return Foo() != nil;", Value::TRUE);

        eval_eq("class Foo { init(s) { s.x = 1; } } let o = Foo(); return o.x;", Value::fixnum(1));
        eval_eq("class Foo { init(s, a) { s.x = a; } } let o = Foo(7); return o.x;", Value::fixnum(7));
        eval_eq("class Foo { init(s, a, b) { s.x = a; s.y = b; } } let o = Foo(5, 3); return o.x - o.y;", Value::fixnum(2));
        eval_eq("class C { init(s) { s.c = 0; } inc(s) { ++s.c; } } let o = C(); o.inc(); return o.c;", Value::fixnum(1));
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
        eval_eq("class F {} return nil instanceof F;", Value::FALSE);
        eval_eq("class F {} let o = F(); return o instanceof F;", Value::TRUE);
        eval_eq("class F {} class G {} let o = F(); return o instanceof G;", Value::FALSE);
        eval_eq("class F {} return F() instanceof F;", Value::TRUE);

        // Core runtime classes
        eval_eq("return nil instanceof Int64;", Value::FALSE);
        eval_eq("return true instanceof Int64;", Value::FALSE);
        eval_eq("return 5 instanceof Int64;", Value::TRUE);
        eval_eq("return 77 instanceof String;", Value::FALSE);
        eval_eq("return 'foo' instanceof String;", Value::TRUE);
        eval_eq("return [] instanceof Array;", Value::TRUE);
        eval_eq("return {} instanceof Dict;", Value::TRUE);
    }
}