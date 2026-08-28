use rustc_hash::FxHashMap as HashMap;
use std::thread;
use std::sync::{Arc, Weak, Mutex, mpsc};
use std::time::Duration;
use crate::dict::Dict;
#[cfg(feature = "log_gc")]
use crate::utils::thousands_sep;
use crate::ast::{Program, FunId, ClassId, Class};
use crate::alloc::{Alloc, Tag, HEADER_SIZE, INIT_SIZE, MSG_INIT_SIZE};
use crate::object::Object;
use crate::closure::Closure;
use crate::array::Array;
use crate::bytearray::ByteArray;
use crate::codegen::CompiledFun;
use crate::insns::{self, NameId, Opcode};
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

/// Interpreter instructions, one 64-bit word each. The opcode occupies
/// the low bits and the operands are packed above it, so an instruction
/// holds no heap pointers and the collector never walks the code
pub use crate::insns::Insn;

/// Cache for a field access site. The name is what the site was compiled
/// for; the class and slot are what it last resolved to
struct PropCache
{
    name: NameId,
    class_id: ClassId,
    slot_idx: u32,
}

/// Cache for a call site whose callee is not statically known.
///
/// A dynamic call guards on the function value it last resolved to, a
/// method call on the class it last looked the name up on, and a host
/// method on the type tag in the instruction. A site that has resolved
/// records everything the frame needs, so the call itself is a push and
/// a jump
#[derive(Default)]
struct CallCache
{
    name: NameId,

    // Class the site last looked the method up on, which is what a
    // method call guards against
    class_id: ClassId,

    // Callee the site resolved to
    fun_id: FunId,
    entry_pc: u32,
    frame_size: u16,

    // Set for a constructor site, which allocates before it calls
    num_slots: u16,

    // Set for a host method site, which the type tag guards
    host_fn: HostFnId,
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

    // Previous base pointer at the time of call
    prev_bp: usize,

    // How far the stack reached in the caller's frame, which is what a
    // return restores. The callee's frame starts inside the caller's, so
    // this is what tells the two apart
    prev_top: usize,

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

    // Heap constants the compiled code refers to. Instructions hold an
    // index into this rather than a heap pointer, so the collector traces
    // the pool and never walks the instruction stream
    consts: Vec<Value>,

    // Interned field and method names. Instructions name a field or a
    // method by id; `name_strs` holds the matching heap string, which the
    // dictionary paths need as a key
    names: Vec<String>,
    name_ids: HashMap<String, NameId>,
    name_strs: Vec<Value>,

    // Inline caches, one entry per site that can resolve to more than one
    // thing. Holding the payload out of line is what keeps an instruction
    // within a single word
    prop_caches: Vec<PropCache>,
    call_caches: Vec<CallCache>,
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
            consts: Vec::default(),
            names: Vec::default(),
            name_ids: HashMap::default(),
            name_strs: Vec::default(),
            prop_caches: Vec::default(),
            call_caches: Vec::default(),
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

    /// Cache a copy of a class in this actor, copying it from the parent
    /// VM the first time it is asked for. Splitting this out from
    /// `with_class` lets a caller borrow the class alongside something
    /// else the actor owns, such as an interned name
    fn load_class(&mut self, class_id: ClassId)
    {
        if !self.classes.contains_key(&class_id) {
            self.copy_class(class_id);
        }
    }

    /// Copy a class out of the parent VM. This locks the VM and clones,
    /// and happens once per class, so it is kept out of line rather than
    /// inlined into the interpreter at every site that touches a class
    #[cold]
    #[inline(never)]
    fn copy_class(&mut self, class_id: ClassId)
    {
        // Borrow the VM and clone the class
        let vm = self.vm.lock().unwrap();

        // Class ids come from the compiler and from the runtime itself,
        // so a missing class means we are at fault, not the running program
        let class = match vm.prog.classes.get(&class_id) {
            Some(class) => class.clone(),
            None => panic!("internal error: could not find class with id={:?}", class_id),
        };
        drop(vm);

        self.classes.insert(class_id, class);
    }

    /// Compute something requiring access to a class, lazily
    /// copying the class from the parent VM as needed
    pub fn with_class<F, T>(&mut self, class_id: ClassId, f: F) -> T
    where F: FnOnce(&Class) -> T
    {
        self.load_class(class_id);
        f(&self.classes[&class_id])
    }

    /// Disassemble a function value, compiling it first if it has not run
    /// yet. What comes back reflects any rewriting the sites in it have
    /// done, so a function dumped after it has been called shows the
    /// forms it settled on
    pub fn dump_fun_bytecode(&mut self, fun: Value) -> Result<String, String>
    {
        let fun_id = match fun.to_fun_id() {
            Some(fun_id) => fun_id,
            None => return Err(format!("expected a function value but got {:?}", fun)),
        };

        // Compiling can collect, so the callee is held where the
        // collector can see it
        let mut fun = fun;
        let entry = self.get_compiled_fun(&mut fun);

        let name = {
            let vm = self.vm.lock().unwrap();
            let fun = &vm.prog.funs[&fun_id];

            if fun.class_id != ClassId::default() {
                format!("{}.{}", vm.prog.classes[&fun.class_id].name, fun.name)
            } else {
                fun.name.clone()
            }
        };

        Ok(self.dump_bytecode(&name, &entry))
    }

    /// Disassemble one compiled function.
    ///
    /// The instruction's own formatting prints the operands, so this adds
    /// what they cannot say on their own: what a cache index or constant
    /// slot refers to, and where a branch actually lands
    pub fn dump_bytecode(&self, name: &str, fun: &CompiledFun) -> String
    {
        use std::fmt::Write;

        let mut out = String::new();

        writeln!(
            out,
            "fun {}: {} param(s), {} register frame, {} instruction(s)",
            name,
            fun.num_params,
            fun.frame_size,
            fun.end_pc - fun.entry_pc,
        ).unwrap();

        for pc in fun.entry_pc..fun.end_pc {
            let insn = self.insns[pc];

            // Offsets are printed relative to the function's entry, so
            // that a dump reads the same wherever the function landed
            write!(out, "  {:>4}  {}", pc - fun.entry_pc, insn).unwrap();

            if let Some(note) = self.insn_note(insn, pc, fun.entry_pc) {
                write!(out, "  ; {}", note).unwrap();
            }

            out.push('\n');
        }

        out
    }

    /// What an instruction refers to through a side table, for the
    /// disassembly to spell out
    fn insn_note(&self, insn: Insn, pc: usize, entry_pc: usize) -> Option<String>
    {
        // A branch displacement counts from the instruction after it
        if let Some(disp) = insn.branch_disp() {
            let target = (pc as i64) + 1 + (disp as i64) - (entry_pc as i64);
            return Some(format!("-> {}", target));
        }

        match insn.opcode() {
            // The immediate is the tagged word, not the number the source
            // wrote, so the value it stands for is worth spelling out
            Opcode::load_imm32 => {
                let imm = insns::load_imm32::decode(insn).imm;
                Some(format!("{:?}", Value::from_raw_bits(imm as i64 as u64)))
            }

            Opcode::ret_imm32 => {
                let imm = insns::ret_imm32::decode(insn).imm;
                Some(format!("{:?}", Value::from_raw_bits(imm as i64 as u64)))
            }

            Opcode::load_const => {
                let slot = insns::load_const::decode(insn).slot;
                Some(format!("{:?}", self.consts[slot as usize]))
            }

            Opcode::get_field => {
                let cache = insns::get_field::decode(insn).cache;
                Some(format!(".{}", self.name_str(self.prop_caches[cache as usize].name)))
            }

            Opcode::set_field => {
                let cache = insns::set_field::decode(insn).cache;
                Some(format!(".{}", self.name_str(self.prop_caches[cache as usize].name)))
            }

            Opcode::call_method | Opcode::call_method_host => {
                let cache = match insn.opcode() {
                    Opcode::call_method => insns::call_method::decode(insn).cache,
                    _ => insns::call_method_host::decode(insn).cache,
                };
                Some(format!(".{}()", self.name_str(self.call_caches[cache as usize].name)))
            }

            Opcode::call_host => {
                let id = insns::call_host::decode(insn).host_fn;
                Some(format!("${}", HostFnId::from_index(id).get().name))
            }

            Opcode::call_pc => {
                let cache = insns::call_pc::decode(insn).cache;
                let fun_id = self.call_caches[cache as usize].fun_id;
                let vm = self.vm.lock().unwrap();
                Some(format!("{}()", vm.prog.funs[&fun_id].name))
            }

            Opcode::call_direct => {
                let fun_id = insns::call_direct::decode(insn).fun;
                let vm = self.vm.lock().unwrap();
                let fun_id = FunId::from(fun_id as usize);
                Some(format!("{}()", vm.prog.funs[&fun_id].name))
            }

            _ => None
        }
    }

    /// Text of an interned field or method name
    pub fn name_str(&self, name: NameId) -> &str
    {
        &self.names[name as usize]
    }

    /// Get the slot index for a field named by an interned name
    fn get_slot_idx_of(&mut self, class_id: ClassId, name: NameId) -> Option<usize>
    {
        self.load_class(class_id);
        self.classes[&class_id].fields.get(self.name_str(name)).copied()
    }

    /// Get the function id of a method named by an interned name
    fn get_method_of(&mut self, class_id: ClassId, name: NameId) -> Option<FunId>
    {
        self.load_class(class_id);
        self.classes[&class_id].methods.get(self.name_str(name)).copied()
    }

    /// Resolve a field read the cached path did not handle: a dict, an
    /// array, a string, or an object of a class this site has not seen.
    /// Kept out of line so that the interpreter loop carries only the
    /// cached case
    #[inline(never)]
    fn get_field_slow(&mut self, obj: Value, cache: u32) -> Result<Value, String>
    {
        let name = self.prop_caches[cache as usize].name;

        if !obj.is_heap() {
            return Err(format!("get_field on non-object value {:?}", obj));
        }

        // The block header says what the value points at, so one load
        // and one switch settle the type
        match obj.heap_tag() {
            Tag::Object => {
                let o = obj.as_obj();

                let slot_idx = match self.get_slot_idx_of(o.class_id, name) {
                    Some(slot_idx) => slot_idx,
                    None => {
                        let class_name = self.get_class_name(o.class_id);
                        let field_names = self.get_field_names(o.class_id);
                        return Err(format!(
                            "class `{}` has no field `{}`, known fields are: {}",
                            class_name,
                            self.name_str(name),
                            field_names,
                        ));
                    }
                };

                // Update the cache
                let entry = &mut self.prop_caches[cache as usize];
                entry.class_id = o.class_id;
                entry.slot_idx = slot_idx as u32;

                let val = o.get(slot_idx);

                if val.is_undef() {
                    return Err(format!("object field not initialized `{}`", self.name_str(name)));
                }

                Ok(val)
            }

            Tag::Dict => {
                let key = self.name_str(name);

                match obj.as_dict().get(key) {
                    Some(v) => Ok(v),
                    None => Err(format!("key '{}' not found in dict", key))
                }
            }

            Tag::Array => {
                match self.name_str(name) {
                    "len" => Ok(Value::fixnum(obj.as_arr().len() as i64)),
                    _ => Err("field not found on array".to_string())
                }
            }

            Tag::ByteArray => {
                match self.name_str(name) {
                    "len" => Ok(Value::fixnum(obj.as_ba().num_bytes() as i64)),
                    _ => Err("field not found on bytearray".to_string())
                }
            }

            Tag::Str => {
                match self.name_str(name) {
                    "len" => Ok(Value::fixnum(obj.as_str().len() as i64)),
                    _ => Err("field not found on string".to_string())
                }
            }

            _ => Err(format!("get_field on non-object value {:?}", obj))
        }
    }

    /// Resolve a field write the cached path did not handle: a dict, or
    /// an object of a class this site has not seen
    #[inline(never)]
    fn set_field_slow(&mut self, mut obj: Value, mut val: Value, cache: u32)
        -> Result<(), String>
    {
        let name = self.prop_caches[cache as usize].name;

        if let Some(o) = obj.to_obj() {
            let slot_idx = match self.get_slot_idx_of(o.class_id, name) {
                Some(slot_idx) => slot_idx,
                None => {
                    let class_name = self.get_class_name(o.class_id);
                    let field_names = self.get_field_names(o.class_id);
                    return Err(format!(
                        "class `{}` has no field `{}`, known fields are: {}",
                        class_name,
                        self.name_str(name),
                        field_names,
                    ));
                }
            };

            // Update the cache
            let entry = &mut self.prop_caches[cache as usize];
            entry.class_id = o.class_id;
            entry.slot_idx = slot_idx as u32;

            o.set(slot_idx, val);
            return Ok(());
        }

        if obj.is_dict() {
            let alloc_size = obj.as_dict().will_allocate();
            self.gc_check(alloc_size, &mut [&mut obj, &mut val]);

            // Read after the check: the collector moves the interned
            // name along with everything else
            let key = self.name_strs[name as usize];
            obj.as_dict().set(key.as_string() as *const Str, val, &mut self.alloc);
            return Ok(());
        }

        Err("set_field on non-object/dict value".to_string())
    }

    /// Create a cache entry for a field access site
    pub fn new_prop_cache(&mut self, name: &str) -> u32
    {
        let name = self.intern_name(name);

        // The class id no real object has, so a fresh site always misses
        self.prop_caches.push(PropCache {
            name,
            class_id: ClassId::default(),
            slot_idx: 0,
        });

        (self.prop_caches.len() - 1) as u32
    }

    /// Create a cache entry for a call site
    pub fn new_call_cache(&mut self, name: &str) -> u32
    {
        let name = if name.is_empty() { 0 } else { self.intern_name(name) };

        self.new_call_cache_entry(CallCache {
            name,
            ..CallCache::default()
        })
    }

    /// Add a call cache entry and return its index. A site that is
    /// rewritten into a cached form allocates its entry the first time it
    /// runs, which happens once
    fn new_call_cache_entry(&mut self, cache: CallCache) -> u32
    {
        self.call_caches.push(cache);
        (self.call_caches.len() - 1) as u32
    }

    /// Add a constant to this actor's pool and return its index. The
    /// pool holds the heap constants the code refers to, along with the
    /// immediates too wide to sit in an instruction. It is a GC root, so
    /// a constant is safe to hold from the moment it lands here
    pub fn push_const(&mut self, val: Value) -> u32
    {
        self.consts.push(val);
        (self.consts.len() - 1) as u32
    }

    /// Intern a field or method name. The heap string a name needs as a
    /// dict key is allocated once, when the name is first seen, and is
    /// rooted from then on
    pub fn intern_name(&mut self, name: &str) -> NameId
    {
        if let Some(id) = self.name_ids.get(name) {
            return *id;
        }

        self.gc_check(Str::alloc_size(name.len()), &mut []);
        let val = Str::new(name, &mut self.alloc);

        let id = self.names.len() as NameId;
        self.names.push(name.to_string());
        self.name_ids.insert(name.to_string(), id);
        self.name_strs.push(val);
        id
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

            // Constants and interned names the compiled code refers to.
            // Instructions index into these, so the instruction stream
            // itself holds nothing the collector has to look at
            for val in &mut self.consts {
                *val = copier.forward(*val);
            }
            for val in &mut self.name_strs {
                *val = copier.forward(*val);
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
    /// Call a host function. Its arguments sit in consecutive registers
    /// starting at `base`, and the value it returns is written back over
    /// the first of them, which is where a call leaves its result
    fn call_host(&mut self, host_fn: &HostFn, base: usize, argc: usize) -> Result<(), String>
    {
        macro_rules! arg {
            ($idx: expr) => { self.stack[base + $idx] }
        }

        if host_fn.num_params() != argc {
            return Err(format!(
                "incorrect argument count for host function `{}`, got {}, expected {}",
                host_fn.name,
                argc,
                host_fn.num_params()
            ));
        }

        debug_assert!(base + argc <= self.stack.len());

        let result = match host_fn.f
        {
            FnPtr::Fn0(fun) => {
                fun(self)
            }

            FnPtr::Fn1(fun) => {
                let a0 = arg!(0);
                fun(self, a0)
            }

            FnPtr::Fn2(fun) => {
                let (a0, a1) = (arg!(0), arg!(1));
                fun(self, a0, a1)
            }

            FnPtr::Fn3(fun) => {
                let (a0, a1, a2) = (arg!(0), arg!(1), arg!(2));
                fun(self, a0, a1, a2)
            }

            FnPtr::Fn4(fun) => {
                let (a0, a1, a2, a3) = (arg!(0), arg!(1), arg!(2), arg!(3));
                fun(self, a0, a1, a2, a3)
            }

            FnPtr::Fn5(fun) => {
                let (a0, a1, a2, a3, a4) = (arg!(0), arg!(1), arg!(2), arg!(3), arg!(4));
                fun(self, a0, a1, a2, a3, a4)
            }

            FnPtr::Fn8(fun) => {
                let (a0, a1, a2, a3) = (arg!(0), arg!(1), arg!(2), arg!(3));
                let (a4, a5, a6, a7) = (arg!(4), arg!(5), arg!(6), arg!(7));
                fun(self, a0, a1, a2, a3, a4, a5, a6, a7)
            }
        };

        match result {
            // A host function can collect, so the result is written back
            // by index rather than through a reference taken beforehand
            Ok(v) => { self.stack[base] = v; Ok(()) },
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

        // The arguments become the first registers of the frame.
        // Compiling below can collect, and the stack is where the
        // collector looks for them.
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
            prev_bp: usize::MAX,
            prev_top: 0,
            ret_addr: usize::MAX,
        });

        // The frame of the outermost call starts at the bottom of the stack
        let mut bp = 0;

        // Every register the frame covers has to hold a value the
        // collector can look at, so the ones past the arguments start nil
        self.stack.resize(fun_entry.frame_size, Value::NIL);

        // Read a register of the current frame. Codegen sizes every frame
        // to the registers its function uses, so the index is in bounds.
        macro_rules! get_reg {
            ($reg: expr) => {{
                let idx = bp + ($reg as usize);
                debug_assert!(idx < self.stack.len());
                unsafe { *self.stack.get_unchecked(idx) }
            }}
        }

        // Write a register of the current frame
        macro_rules! set_reg {
            ($reg: expr, $val: expr) => {{
                let idx = bp + ($reg as usize);
                let val = $val;
                debug_assert!(idx < self.stack.len());
                unsafe { *self.stack.get_unchecked_mut(idx) = val; }
            }}
        }

        macro_rules! set_reg_bool {
            ($reg: expr, $b: expr) => { set_reg!($reg, Value::bool_val($b)) }
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
            ($insn_word: expr, $insn: literal, $opnds: ident, $checked_op: ident, $rhs: ident, $float_op: path, $slow_path: ident) => {{
                let opnds = insns::$opnds::decode($insn_word);
                let v0 = get_reg!(opnds.a);
                let v1 = get_reg!(opnds.b);

                // A 64-bit overflow is exactly the case where the result
                // no longer fits in a fixnum
                if v0.is_fixnum() && v1.is_fixnum() {
                    if let Some(r) = (v0.raw_bits() as i64).$checked_op(v1.$rhs() as i64) {
                        set_reg!(opnds.dst, Value::from_raw_bits(r as u64));
                        continue;
                    }
                }

                if v0.is_flonum() && v1.is_flonum() {
                    let f = $float_op(v0.as_flonum(), v1.as_flonum());

                    if let Some(r) = Value::try_flonum(f) {
                        set_reg!(opnds.dst, r);
                        continue;
                    }
                }

                let r = slow!($insn, self.$slow_path(v0, v1));
                set_reg!(opnds.dst, r);
            }}
        }

        // Arithmetic against a fixnum immediate
        macro_rules! arith_imm_insn {
            ($insn_word: expr, $insn: literal, $opnds: ident, $checked_op: ident, $rhs: ident, $slow_path: ident) => {{
                let opnds = insns::$opnds::decode($insn_word);
                let v0 = get_reg!(opnds.a);

                // The immediate always fits a fixnum, so the tagged words
                // can be combined directly
                let cst = Value::fixnum(opnds.imm as i64);

                if v0.is_fixnum() {
                    if let Some(r) = (v0.raw_bits() as i64).$checked_op(cst.$rhs() as i64) {
                        set_reg!(opnds.dst, Value::from_raw_bits(r as u64));
                        continue;
                    }
                }

                let r = slow!($insn, self.$slow_path(v0, cst));
                set_reg!(opnds.dst, r);
            }}
        }

        // Bitwise ops. Fixnum tags are zero, so the op keeps them that
        // way and the result needs no retagging.
        macro_rules! bitop_insn {
            ($insn_word: expr, $insn: literal, $opnds: ident, $op: tt, $slow_path: ident) => {{
                let opnds = insns::$opnds::decode($insn_word);
                let v0 = get_reg!(opnds.a);
                let v1 = get_reg!(opnds.b);

                if v0.is_fixnum() && v1.is_fixnum() {
                    set_reg!(opnds.dst, Value::from_raw_bits(v0.raw_bits() $op v1.raw_bits()));
                    continue;
                }

                let r = slow!($insn, self.$slow_path(v0, v1));
                set_reg!(opnds.dst, r);
            }}
        }

        // Compare and branch. Tagged fixnums order like the integers they
        // hold, and inline doubles are decoded and compared directly.
        macro_rules! cmp_branch {
            ($insn_word: expr, $insn: literal, $opnds: ident, $op: tt, $slow_path: ident) => {{
                let opnds = insns::$opnds::decode($insn_word);
                let v0 = get_reg!(opnds.a);
                let v1 = get_reg!(opnds.b);

                let taken = if v0.is_fixnum() && v1.is_fixnum() {
                    (v0.raw_bits() as i64) $op (v1.raw_bits() as i64)
                } else if v0.is_flonum() && v1.is_flonum() {
                    v0.as_flonum() $op v1.as_flonum()
                } else {
                    slow!($insn, $slow_path(v0, v1))
                };

                if taken {
                    pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                }
            }}
        }

        // Set up a new frame and jump into a callee. The callee's frame
        // starts at the caller's `start_reg`, which is where its
        // arguments already sit and where its return value will land.
        macro_rules! push_frame {
            ($fun_val: expr, $start_reg: expr, $entry_pc: expr, $frame_size: expr) => {{
                let new_bp = bp + ($start_reg as usize);

                self.frames.push(StackFrame {
                    fun: $fun_val,
                    prev_bp: bp,
                    prev_top: self.stack.len(),
                    ret_addr: pc,
                });

                bp = new_bp;
                pc = $entry_pc as usize;

                // Registers the callee has not written yet still have to
                // hold something the collector can look at
                self.stack.resize(bp + ($frame_size as usize), Value::NIL);
            }}
        }

        // Hand a value back to the caller and drop this frame. The
        // value goes to the callee's frame base, which is the register
        // the caller set the call up in
        macro_rules! do_return {
            ($val: expr) => {{
                let ret_val = $val;

                // If this is a top-level return
                if self.frames.len() == 1 {
                    self.stack.clear();
                    self.frames.clear();
                    return ret_val;
                }

                let frame = self.frames.pop().unwrap();
                self.stack[bp] = ret_val;

                // Restoring the caller's frame drops whatever the callee
                // used above it, and refills what it left short
                self.stack.resize(frame.prev_top, Value::NIL);

                bp = frame.prev_bp;
                pc = frame.ret_addr;
            }}
        }

        // Resolve a callee that is not statically known, checking its
        // argument count. This can compile the callee and therefore
        // collect, so the callee is held where the collector can see it.
        macro_rules! resolve_callee {
            ($fun_val: expr, $argc: expr) => {{
                let mut fun_val = $fun_val;

                let fun_id = match fun_val.to_fun_id() {
                    Some(id) => id,
                    None => error!("call to non-function value: `{:?}`", fun_val)
                };

                let entry = self.get_compiled_fun(&mut fun_val);

                if ($argc as usize) != entry.num_params {
                    let vm = self.vm.lock().unwrap();
                    let fun = &vm.prog.funs[&fun_id];
                    error!(
                        "incorrect argument count in call to function \"{}\", defined at {}, received {} arguments, expected {}",
                        fun.name,
                        fun.pos,
                        $argc,
                        entry.num_params
                    );
                }

                (fun_val, fun_id, entry)
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
            // Every compiled function ends with a return instruction, so
            // execution can never run past the end of the insn stream.
            // That makes the load in bounds and keeps the increment below
            // the length, so neither needs to be checked here.
            debug_assert!(pc < self.insns.len());
            let insn = unsafe { *self.insns.get_unchecked(pc) };
            let this_pc = pc;
            pc = unsafe { pc.unchecked_add(1) };

            match insn.opcode() {
                Opcode::nop => {},

                Opcode::breakpoint => {},

                Opcode::panic => {
                    let opnds = insns::panic::decode(insn);
                    error!(
                        "explicit panic at: {}@{}:{}",
                        crate::lexer::name_from_id(opnds.file_id as u32),
                        opnds.line_no,
                        opnds.col_no,
                    );
                }

                Opcode::load_imm32 => {
                    let opnds = insns::load_imm32::decode(insn);
                    set_reg!(opnds.dst, Value::from_raw_bits(opnds.imm as i64 as u64));
                }

                Opcode::load_const => {
                    let opnds = insns::load_const::decode(insn);
                    let val = self.consts[opnds.slot as usize];
                    set_reg!(opnds.dst, val);
                }

                Opcode::mov => {
                    let opnds = insns::mov::decode(insn);
                    set_reg!(opnds.dst, get_reg!(opnds.src));
                }

                Opcode::get_global => {
                    let opnds = insns::get_global::decode(insn);
                    let idx = opnds.idx as usize;

                    if idx >= self.globals.len() {
                        error!("get_global", "invalid global index {}", idx);
                    }

                    let val = self.globals[idx];

                    if val.is_undef() {
                        error!("get_global", "attempting to read uninitialized global");
                    }

                    set_reg!(opnds.dst, val);
                }

                Opcode::set_global => {
                    let opnds = insns::set_global::decode(insn);
                    let idx = opnds.idx as usize;

                    if idx >= self.globals.len() {
                        error!("set_global", "invalid global index {}", idx);
                    }

                    self.globals[idx] = get_reg!(opnds.src);
                }

                Opcode::add => arith_insn!(insn, "add", add, checked_add, raw_bits, f64::add, add_slow),
                Opcode::sub => arith_insn!(insn, "sub", sub, checked_sub, raw_bits, f64::sub, sub_slow),
                Opcode::mul => arith_insn!(insn, "mul", mul, checked_mul, as_fixnum, f64::mul, mul_slow),

                Opcode::add_imm16 => arith_imm_insn!(insn, "add", add_imm16, checked_add, raw_bits, add_slow),
                Opcode::sub_imm16 => arith_imm_insn!(insn, "sub", sub_imm16, checked_sub, raw_bits, sub_slow),
                Opcode::mul_imm16 => arith_imm_insn!(insn, "mul", mul_imm16, checked_mul, as_fixnum, mul_slow),

                // Division always produces a float
                // Division by zero produces an infinity (this is intentional)
                Opcode::div => {
                    let opnds = insns::div::decode(insn);
                    let v0 = get_reg!(opnds.a);
                    let v1 = get_reg!(opnds.b);

                    if v0.is_flonum() && v1.is_flonum() {
                        if let Some(r) = Value::try_flonum(v0.as_flonum() / v1.as_flonum()) {
                            set_reg!(opnds.dst, r);
                            continue;
                        }
                    }

                    let r = slow!("div", self.div_num(v0, v1));
                    set_reg!(opnds.dst, r);
                }

                // Integer division
                // Division by zero will cause a panic (this is intentional)
                Opcode::div_int => {
                    let opnds = insns::div_int::decode(insn);
                    let v0 = get_reg!(opnds.a);
                    let v1 = get_reg!(opnds.b);

                    // Dividing shrinks the magnitude, so the quotient
                    // fits unless it is the one case that overflows
                    if v0.is_fixnum() && v1.is_fixnum() {
                        if let Some(q) = v0.as_fixnum().checked_div(v1.as_fixnum()) {
                            if let Some(r) = Value::try_fixnum(q) {
                                set_reg!(opnds.dst, r);
                                continue;
                            }
                        }
                    }

                    let r = slow!("div_int", self.div_int_slow(v0, v1));
                    set_reg!(opnds.dst, r);
                }

                // Division by zero will cause a panic (this is intentional)
                Opcode::modulo => {
                    let opnds = insns::modulo::decode(insn);
                    let v0 = get_reg!(opnds.a);
                    let v1 = get_reg!(opnds.b);

                    // A remainder is smaller than its divisor, so it
                    // always fits back into a fixnum
                    if v0.is_fixnum() && v1.is_fixnum() {
                        if let Some(rem) = v0.as_fixnum().checked_rem(v1.as_fixnum()) {
                            set_reg!(opnds.dst, Value::fixnum(rem));
                            continue;
                        }
                    }

                    if v0.is_flonum() && v1.is_flonum() {
                        if let Some(r) = Value::try_flonum(v0.as_flonum() % v1.as_flonum()) {
                            set_reg!(opnds.dst, r);
                            continue;
                        }
                    }

                    let r = slow!("modulo", self.modulo_slow(v0, v1));
                    set_reg!(opnds.dst, r);
                }

                Opcode::bit_or => bitop_insn!(insn, "bit_or", bit_or, |, bit_or_slow),
                Opcode::bit_and => bitop_insn!(insn, "bit_and", bit_and, &, bit_and_slow),
                Opcode::bit_xor => bitop_insn!(insn, "bit_xor", bit_xor, ^, bit_xor_slow),

                Opcode::bit_not => {
                    let opnds = insns::bit_not::decode(insn);
                    let v0 = get_reg!(opnds.src);

                    match v0.to_i64() {
                        Some(v) => { let r = self.int64(!v); set_reg!(opnds.dst, r); }
                        None => error!("bit_not", "unsupported type in bitwise not {:?}", v0)
                    }
                }

                // Integer left shift
                Opcode::lshift => {
                    let opnds = insns::lshift::decode(insn);
                    let v0 = get_reg!(opnds.a);
                    let v1 = get_reg!(opnds.b);

                    // The shift can push bits out of fixnum range, so the
                    // result has to be checked rather than retagged
                    if v0.is_fixnum() && v1.is_fixnum() {
                        let shift = v1.as_fixnum();

                        if shift >= 0 && shift < 62 {
                            if let Some(r) = Value::try_fixnum(v0.as_fixnum() << shift) {
                                set_reg!(opnds.dst, r);
                                continue;
                            }
                        }
                    }

                    let r = slow!("lshift", self.lshift_slow(v0, v1));
                    set_reg!(opnds.dst, r);
                }

                // Integer right shift
                Opcode::rshift => {
                    let opnds = insns::rshift::decode(insn);
                    let v0 = get_reg!(opnds.a);
                    let v1 = get_reg!(opnds.b);

                    // Shifting a fixnum right keeps it in range, so only
                    // the bits the tag occupies have to be cleared
                    if v0.is_fixnum() && v1.is_fixnum() {
                        let shift = v1.as_fixnum();

                        if shift >= 0 && shift < 62 {
                            set_reg!(opnds.dst, Value::fixnum(v0.as_fixnum() >> shift));
                            continue;
                        }
                    }

                    let r = slow!("rshift", self.rshift_slow(v0, v1));
                    set_reg!(opnds.dst, r);
                }

                // Logical negation
                Opcode::not => {
                    let opnds = insns::not::decode(insn);
                    let v0 = get_reg!(opnds.src);

                    match v0.to_bool() {
                        Some(b) => set_reg_bool!(opnds.dst, !b),
                        None => error!("not", "unsupported type in logical not {:?}", v0)
                    }
                }

                // Create a new closure
                Opcode::clos_new => {
                    let opnds = insns::clos_new::decode(insn);
                    let num_slots = opnds.num_slots as usize;

                    self.gc_check(Closure::alloc_size(num_slots), &mut []);

                    let fun_id = FunId::from(opnds.fun_id as usize);
                    let clos = Closure::new(fun_id, num_slots, &mut self.alloc);
                    set_reg!(opnds.dst, clos);
                }

                // Set a closure slot
                Opcode::clos_set => {
                    let opnds = insns::clos_set::decode(insn);
                    let clos = get_reg!(opnds.clos);
                    let val = get_reg!(opnds.src);

                    match clos.to_clos() {
                        Some(clos) => clos.set(opnds.idx as usize, val),
                        None => error!("clos_set", "expected closure")
                    }
                }

                // Get a closure slot for the function currently executing
                Opcode::clos_get => {
                    let opnds = insns::clos_get::decode(insn);
                    let fun = self.frames[self.frames.len() - 1].fun;

                    let val = match fun.to_clos() {
                        Some(clos) => clos.get(opnds.idx as usize),
                        None => error!("clos_get", "not a closure")
                    };

                    if val.is_undef() {
                        error!("clos_get", "executing uninitialized closure");
                    }

                    set_reg!(opnds.dst, val);
                }

                // Create a new mutable cell
                Opcode::cell_new => {
                    let opnds = insns::cell_new::decode(insn);

                    self.gc_check(HEADER_SIZE + size_of::<Value>(), &mut []);

                    let p_cell = self.alloc.alloc(Value::NIL, Tag::Cell);
                    set_reg!(opnds.dst, Value::cell(p_cell));
                }

                // Set the value stored in a mutable cell
                Opcode::cell_set => {
                    let opnds = insns::cell_set::decode(insn);
                    let cell = get_reg!(opnds.cell);
                    let val = get_reg!(opnds.src);

                    match cell.to_cell() {
                        Some(p_cell) => *p_cell = val,
                        None => error!("cell_set", "expected cell")
                    };
                }

                // Get the value stored in a mutable cell
                Opcode::cell_get => {
                    let opnds = insns::cell_get::decode(insn);
                    let cell = get_reg!(opnds.cell);

                    let val = match cell.to_cell() {
                        Some(p_cell) => *p_cell,
                        None => error!("cell_get", "invalid cell in cell_get")
                    };

                    set_reg!(opnds.dst, val);
                }

                Opcode::instanceof => {
                    let opnds = insns::instanceof::decode(insn);
                    let val = get_reg!(opnds.val);
                    let class_id = ClassId::from(opnds.class_id as usize);
                    set_reg_bool!(opnds.dst, crate::runtime::get_class_id(val) == class_id);
                }

                // Create new empty dictionary
                Opcode::dict_new => {
                    let opnds = insns::dict_new::decode(insn);
                    self.gc_check(Dict::alloc_size(0), &mut []);
                    let dict = Dict::with_capacity(0, &mut self.alloc);
                    set_reg!(opnds.dst, dict);
                }

                // Create new empty array
                Opcode::arr_new => {
                    let opnds = insns::arr_new::decode(insn);
                    let capacity = opnds.capacity as usize;

                    self.gc_check(Array::alloc_size(capacity), &mut []);
                    let arr = Array::with_capacity(capacity, &mut self.alloc);
                    set_reg!(opnds.dst, arr);
                }

                // Append an element at the end of an array
                // This instruction is used to construct array literals
                Opcode::arr_push => {
                    let opnds = insns::arr_push::decode(insn);
                    let arr = get_reg!(opnds.arr);
                    let val = get_reg!(opnds.val);
                    crate::array::array_push(self, arr, val).unwrap();
                }

                // Clone a bytearray
                Opcode::ba_clone => {
                    let opnds = insns::ba_clone::decode(insn);
                    let mut val = get_reg!(opnds.src);
                    let ba = unwrap_ba!(val, "ba_clone");

                    self.gc_check(ByteArray::alloc_size(ba.num_bytes()), &mut [&mut val]);

                    let ba_clone = val.as_ba().clone(&mut self.alloc);
                    set_reg!(opnds.dst, ba_clone);
                }

                // Get object field
                Opcode::get_field => {
                    let opnds = insns::get_field::decode(insn);
                    let obj = get_reg!(opnds.obj);
                    let cache = &self.prop_caches[opnds.cache as usize];
                    let (class_id, slot_idx) = (cache.class_id, cache.slot_idx);

                    // Fast path: an object of the class this site last
                    // resolved. Everything else goes out of line, so that
                    // the interpreter loop carries only this
                    if let Some(o) = obj.to_obj() {
                        if o.class_id == class_id {
                            let val = o.get(slot_idx as usize);

                            if !val.is_undef() {
                                set_reg!(opnds.dst, val);
                                continue;
                            }
                        }
                    }

                    let val = match self.get_field_slow(obj, opnds.cache) {
                        Ok(val) => val,
                        Err(msg) => error!("get_field", "{}", msg),
                    };
                    set_reg!(opnds.dst, val);
                }

                // Set object field
                Opcode::set_field => {
                    let opnds = insns::set_field::decode(insn);
                    let obj = get_reg!(opnds.obj);
                    let val = get_reg!(opnds.src);
                    let cache = &self.prop_caches[opnds.cache as usize];
                    let (class_id, slot_idx) = (cache.class_id, cache.slot_idx);

                    if let Some(o) = obj.to_obj() {
                        if o.class_id == class_id {
                            o.set(slot_idx as usize, val);
                            continue;
                        }
                    }

                    if let Err(msg) = self.set_field_slow(obj, val, opnds.cache) {
                        error!("set_field", "{}", msg);
                    }
                }

                Opcode::get_index => {
                    let opnds = insns::get_index::decode(insn);
                    let arr = get_reg!(opnds.arr);
                    let idx = get_reg!(opnds.idx);

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

                    set_reg!(opnds.dst, val);
                }

                Opcode::set_index => {
                    let opnds = insns::set_index::decode(insn);
                    let mut arr = get_reg!(opnds.arr);
                    let mut idx = get_reg!(opnds.idx);
                    let mut val = get_reg!(opnds.src);

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

                // Jump if true
                Opcode::if_true => {
                    let opnds = insns::if_true::decode(insn);
                    let v = get_reg!(opnds.val);

                    if v.is_true() {
                        pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                    } else if !v.is_false() {
                        error!("if_true", "if_true instruction only accepts boolean values");
                    }
                }

                // Jump if false
                Opcode::if_false => {
                    let opnds = insns::if_false::decode(insn);
                    let v = get_reg!(opnds.val);

                    if v.is_false() {
                        pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                    } else if !v.is_true() {
                        error!("if_false", "if_false instruction only accepts boolean values");
                    }
                }

                Opcode::jlt => cmp_branch!(insn, "lt", jlt, <, cmp_lt),
                Opcode::jle => cmp_branch!(insn, "le", jle, <=, cmp_le),
                Opcode::jgt => cmp_branch!(insn, "gt", jgt, >, cmp_gt),
                Opcode::jge => cmp_branch!(insn, "ge", jge, >=, cmp_ge),

                Opcode::jeq => {
                    let opnds = insns::jeq::decode(insn);

                    if get_reg!(opnds.a) == get_reg!(opnds.b) {
                        pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                    }
                }

                Opcode::jne => {
                    let opnds = insns::jne::decode(insn);

                    if get_reg!(opnds.a) != get_reg!(opnds.b) {
                        pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                    }
                }

                // Unconditional jump
                Opcode::jmp => {
                    let opnds = insns::jmp::decode(insn);
                    pc = ((pc as i64) + (opnds.disp as i64)) as usize;
                }

                // Call a host function the VM provides
                Opcode::call_host => {
                    let opnds = insns::call_host::decode(insn);
                    let host_fn = HostFnId::from_index(opnds.host_fn).get();
                    let base = bp + opnds.start_reg as usize;

                    if let Err(msg) = self.call_host(host_fn, base, opnds.argc as usize) {
                        error!("{}", msg);
                    }
                }

                // Call a function by id. The callee is statically known,
                // so this resolves once and becomes a call_pc
                Opcode::call_direct => {
                    let opnds = insns::call_direct::decode(insn);
                    let fun_id = FunId::from(opnds.fun as usize);
                    let (fun_val, _, entry) = resolve_callee!(Value::fun(fun_id), opnds.argc);

                    // The callee cannot change, so the site is rewritten
                    // rather than guarded
                    let cache = self.new_call_cache_entry(CallCache {
                        fun_id,
                        entry_pc: entry.entry_pc as u32,
                        frame_size: entry.frame_size as u16,
                        ..CallCache::default()
                    });
                    self.insns[this_pc] = Insn::call_pc(opnds.start_reg, opnds.argc, cache);

                    push_frame!(fun_val, opnds.start_reg, entry.entry_pc, entry.frame_size);
                }

                // Call a function whose entry point is already known
                Opcode::call_pc => {
                    let opnds = insns::call_pc::decode(insn);
                    let cache = &self.call_caches[opnds.cache as usize];
                    let (fun_id, entry_pc, frame_size) =
                        (cache.fun_id, cache.entry_pc, cache.frame_size);

                    push_frame!(Value::fun(fun_id), opnds.start_reg, entry_pc, frame_size);
                }

                // Call a function value. The callee sits just past the
                // arguments, where this frame will overwrite it
                Opcode::call_opnd => {
                    let opnds = insns::call_opnd::decode(insn);
                    let fun_val = get_reg!(opnds.start_reg as usize + opnds.argc as usize);

                    // A closure and the function it closes over share an
                    // entry point, so the guard is on the function id and
                    // the frame records the closure itself
                    if let Some(fun_id) = fun_val.to_fun_id() {
                        let cache = &self.call_caches[opnds.cache as usize];

                        if cache.fun_id == fun_id {
                            let (entry_pc, frame_size) = (cache.entry_pc, cache.frame_size);
                            push_frame!(fun_val, opnds.start_reg, entry_pc, frame_size);
                            continue;
                        }
                    } else if let Some(f) = fun_val.to_host_fn() {
                        let base = bp + opnds.start_reg as usize;

                        if let Err(msg) = self.call_host(f, base, opnds.argc as usize) {
                            error!("{}", msg);
                        }

                        continue;
                    }

                    let (fun_val, fun_id, entry) = resolve_callee!(fun_val, opnds.argc);

                    let cache = &mut self.call_caches[opnds.cache as usize];
                    cache.fun_id = fun_id;
                    cache.entry_pc = entry.entry_pc as u32;
                    cache.frame_size = entry.frame_size as u16;

                    push_frame!(fun_val, opnds.start_reg, entry.entry_pc, entry.frame_size);
                }

                // Call a method on the value in start_reg
                Opcode::call_method => {
                    let opnds = insns::call_method::decode(insn);
                    let self_val = get_reg!(opnds.start_reg);

                    let cache = &self.call_caches[opnds.cache as usize];
                    let (name, class_id) = (cache.name, cache.class_id);

                    // Guard that self is an object of the class this site
                    // last looked the method up on
                    if let Some(obj) = self_val.to_obj() {
                        if obj.class_id == class_id {
                            let cache = &self.call_caches[opnds.cache as usize];
                            let (fun_id, entry_pc, frame_size) =
                                (cache.fun_id, cache.entry_pc, cache.frame_size);
                            push_frame!(Value::fun(fun_id), opnds.start_reg, entry_pc, frame_size);
                            continue;
                        }
                    }

                    match self_val.to_obj() {
                        Some(obj) => {
                            // Read before the call below, which can compile
                            // the callee and collect, leaving obj behind
                            let class_id = obj.class_id;

                            let fun_id = match self.get_method_of(class_id, name) {
                                None => {
                                    let method = self.name_str(name).to_string();
                                    let class = self.get_class_name(class_id);
                                    error!("call to method `{}`, not found on class `{}`", method, class)
                                }
                                Some(fun_id) => fun_id,
                            };

                            let (fun_val, _, entry) = resolve_callee!(Value::fun(fun_id), opnds.argc);

                            let cache = &mut self.call_caches[opnds.cache as usize];
                            cache.class_id = class_id;
                            cache.fun_id = fun_id;
                            cache.entry_pc = entry.entry_pc as u32;
                            cache.frame_size = entry.frame_size as u16;

                            push_frame!(fun_val, opnds.start_reg, entry.entry_pc, entry.frame_size);
                        }

                        // Call to a primitive e.g. Int64/Float64/immediate (not an object)
                        None => {
                            let host_fn = match crate::runtime::get_method(self_val, self.name_str(name)) {
                                None => {
                                    let method = self.name_str(name).to_string();
                                    error!("call to unknown method `{}`", method)
                                }
                                Some(id) => id,
                            };

                            // Patch this instruction to avoid the method
                            // lookup next time. Bools and classes are left
                            // alone because their methods depend on more
                            // than the type tag.
                            let type_tag = self_val.type_of();
                            if !matches!(type_tag, Type::Bool | Type::Class) {
                                self.call_caches[opnds.cache as usize].host_fn = host_fn;
                                self.insns[this_pc] = Insn::call_method_host(
                                    opnds.start_reg,
                                    opnds.argc,
                                    type_tag as u8,
                                    opnds.cache,
                                );
                            }

                            let base = bp + opnds.start_reg as usize;

                            if let Err(msg) = self.call_host(host_fn.get(), base, opnds.argc as usize) {
                                error!("{}", msg);
                            }
                        }
                    }
                }

                // Call a host method, guarded on the receiver's type tag
                Opcode::call_method_host => {
                    let opnds = insns::call_method_host::decode(insn);
                    let self_val = get_reg!(opnds.start_reg);

                    // Guard that self still has the type the method was found on
                    if self_val.type_of() as u8 == opnds.type_tag {
                        let host_fn = self.call_caches[opnds.cache as usize].host_fn.get();
                        let base = bp + opnds.start_reg as usize;

                        if let Err(msg) = self.call_host(host_fn, base, opnds.argc as usize) {
                            error!("{}", msg);
                        }

                        continue;
                    }

                    // The guard failed. Both forms take the same operands,
                    // so deoptimizing is a write of the opcode
                    self.insns[this_pc] = Insn::call_method(
                        opnds.start_reg,
                        opnds.argc,
                        opnds.cache,
                    );
                    pc = this_pc;
                }

                // Allocate a class instance and run its constructor
                Opcode::new => {
                    let opnds = insns::new::decode(insn);
                    let class_id = ClassId::from(opnds.class_id as usize);
                    let num_slots = self.get_num_slots(class_id);

                    self.gc_check(Object::alloc_size(num_slots), &mut []);

                    // The object is the constructor's self argument, and
                    // also what the call leaves behind as its result
                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc);
                    set_reg!(opnds.start_reg, obj_val);

                    let init_fun = self.get_method(class_id, "init");

                    if let Some(fun_id) = init_fun {
                        let (fun_val, _, entry) = resolve_callee!(Value::fun(fun_id), opnds.argc);

                        // The class is statically known, so the site is
                        // rewritten rather than guarded
                        let cache = self.new_call_cache_entry(CallCache {
                            class_id,
                            fun_id,
                            entry_pc: entry.entry_pc as u32,
                            frame_size: entry.frame_size as u16,
                            num_slots: num_slots as u16,
                            ..CallCache::default()
                        });
                        self.insns[this_pc] = Insn::new_known_ctor(opnds.start_reg, opnds.argc, cache);

                        push_frame!(fun_val, opnds.start_reg, entry.entry_pc, entry.frame_size);
                    } else if opnds.argc != 1 {
                        error!(
                            "class `{}` has no constructor but was given {} argument(s)",
                            self.get_class_name(class_id),
                            opnds.argc - 1,
                        );
                    }
                }

                // Allocate a class instance whose constructor is known
                Opcode::new_known_ctor => {
                    let opnds = insns::new_known_ctor::decode(insn);
                    let cache = &self.call_caches[opnds.cache as usize];
                    let (class_id, num_slots) = (cache.class_id, cache.num_slots as usize);
                    let (fun_id, entry_pc, frame_size) =
                        (cache.fun_id, cache.entry_pc, cache.frame_size);

                    self.gc_check(Object::alloc_size(num_slots), &mut []);

                    let obj_val = Object::new(class_id, num_slots, &mut self.alloc);
                    set_reg!(opnds.start_reg, obj_val);

                    push_frame!(Value::fun(fun_id), opnds.start_reg, entry_pc, frame_size);
                }

                Opcode::ret => {
                    let opnds = insns::ret::decode(insn);
                    let ret_val = get_reg!(opnds.src);
                    do_return!(ret_val);
                }

                Opcode::ret_imm32 => {
                    let opnds = insns::ret_imm32::decode(insn);
                    do_return!(Value::from_raw_bits(opnds.imm as i64 as u64));
                }
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