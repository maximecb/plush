use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use crate::alloc::MAX_AUX24;
use crate::lexer::SrcPos;
use crate::symbols::Decl;
use crate::host::HostFnId;

/// Unary operator
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum UnOp
{
    Minus,
    Not,
}

/// Binary operator
/// https://en.cppreference.com/w/c/language/operator_precedence
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum BinOp
{
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    LShift,
    RShift,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical and, logical or
    And,
    Or,

}

/// Expression
#[derive(Clone, Debug)]
pub(crate) enum Expr
{
    True,
    False,
    Nil,
    Int64(i64),
    Float64(f64),
    String(String),

    // Host function
    HostFn(HostFnId),

    // ByteArray literal
    ByteArray(Vec<u8>),

    // Array literal
    Array {
        exprs: Vec<ExprBox>,
    },

    // Dictionary literal
    Dict {
        pairs: Vec<(String, ExprBox)>,
    },

    Ident(String),

    HostConst(String),

    // Resolved reference to a named entity
    Ref {
        name: String,
        decl: Decl,
    },

    // Function/closure expression
    Fun {
        fun_id: FunId,
        captured: Vec<Decl>,
    },

    // a[b]
    Index {
        base: ExprBox,
        index: ExprBox,
    },

    // a.b
    Member {
        base: ExprBox,
        field: String,
    },

    InstanceOf {
        val: ExprBox,
        class_name: String,
        class_id: ClassId,
    },

    Unary {
        op: UnOp,
        child: ExprBox,
    },

    Binary {
        op: BinOp,
        lhs: ExprBox,
        rhs: ExprBox,
    },

    Ternary {
        test_expr: ExprBox,
        then_expr: ExprBox,
        else_expr: ExprBox,
    },

    Call {
        callee: ExprBox,
        args: Vec<ExprBox>,
    },
}

impl Default for Expr
{
    fn default() -> Self
    {
        Expr::Nil
    }
}

/// Expression box
#[derive(Clone, Debug)]
pub(crate) struct ExprBox
{
    pub(crate) expr: Box<Expr>,
    pub(crate) pos: SrcPos,
}

impl ExprBox
{
    pub(crate) fn new(expr: Expr, pos: SrcPos) -> Self
    {
        Self {
            expr: Box::new(expr),
            pos,
        }
    }

    pub(crate) fn new_ok<E>(expr: Expr, pos: SrcPos) -> Result<Self, E>
    {
        Ok(Self::new(expr, pos))
    }
}

impl Default for ExprBox
{
    fn default() -> Self
    {
        Self::new(Expr::default(), SrcPos::default())
    }
}

/// Statement
#[derive(Clone, Debug)]
pub(crate) enum Stmt
{
    Expr(ExprBox),

    Assign {
        lhs: ExprBox,
        rhs: ExprBox,
    },

    Return(ExprBox),

    Break,
    Continue,

    Block(Vec<StmtBox>),

    If {
        test_expr: ExprBox,
        then_stmt: StmtBox,
        else_stmt: Option<StmtBox>,
    },

    For {
        init_stmt: StmtBox,
        test_expr: ExprBox,
        incr_stmt: StmtBox,
        body_stmt: StmtBox,
    },

    Assert {
        test_expr: ExprBox,
    },

    /// Local variable declaration
    Let {
        mutable: bool,
        var_name: String,
        init_expr: ExprBox,
        decl: Option<Decl>,
    },

    // Class declaration
    ClassDecl {
        class_id: ClassId,
    }
}

impl Default for Stmt
{
    fn default() -> Self
    {
        Stmt::Expr(ExprBox::default())
    }
}

/// Statement box
#[derive(Clone, Debug)]
pub(crate) struct StmtBox
{
    pub(crate) stmt: Box<Stmt>,
    pub(crate) pos: SrcPos,
}

impl StmtBox
{
    pub(crate) fn new(stmt: Stmt, pos: SrcPos) -> Self
    {
        Self {
            stmt: Box::new(stmt),
            pos,
        }
    }

    pub(crate) fn new_ok<E>(stmt: Stmt, pos: SrcPos) -> Result<Self, E>
    {
        Ok(Self::new(stmt, pos))
    }
}

impl Default for StmtBox
{
    fn default() -> Self
    {
        Self::new(Stmt::default(), SrcPos::default())
    }
}

/// Function
#[derive(Default, Clone, Debug)]
pub(crate) struct Function
{
    /// Name of the function
    pub(crate) name: String,

    /// Parameter list
    pub(crate) params: Vec<String>,

    /// Body of the function
    pub(crate) body: StmtBox,

    /// Number of local variables
    pub(crate) num_locals: usize,

    /// Map of captured closure variables to closure slots indices
    pub(crate) captured: HashMap<Decl, u32>,

    /// Set of mutable local variables which are captured by a nested function
    /// Note that this only applies to mutable locals which need a mutable closure cell
    pub(crate) escaping: HashSet<Decl>,

    /// Unit-level (global) function
    pub(crate) is_unit: bool,

    // Source position
    pub(crate) pos: SrcPos,

    /// Internal unique function id
    pub(crate) id: FunId,

    /// Class id this function is associated with
    /// This will be zero is none
    pub(crate) class_id: ClassId,
}

impl Function
{
    /// Register a captured closure variable and return its slot index
    pub(crate) fn reg_captured(&mut self, decl: &Decl) -> u32
    {
        match self.captured.get(decl) {
            Some(idx) => *idx,
            None => {
                let idx = self.captured.len() as u32;
                self.captured.insert(decl.clone(), idx);
                idx
            }
        }
    }

    /// Check if this function is a constructor method
    pub(crate) fn is_ctor(&self) -> bool
    {
        return (
            self.class_id != ClassId::default() &&
            self.name == "init"
        );
    }
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Class
{
    // Class name
    pub(crate) name: String,

    // Name of the parent class
    pub(crate) parent_name: Option<String>,

    // Parent class id
    pub(crate) parent_id: ClassId,

    // Flag to indicate this class has subclasses
    // This is used to accelerate instanceof checks
    pub(crate) has_children: bool,

    // Map of field names to slot indices
    pub(crate) fields: HashMap<String, usize>,

    // Map of method names to function ids
    pub(crate) methods: HashMap<String, FunId>,

    // Source position
    pub(crate) pos: SrcPos,

    // Internal unique class id
    pub(crate) id: ClassId,
}

impl Class
{
    pub(crate) fn reg_field(&mut self, name: &str)
    {
        assert!(self.id.0 != 0);
        if self.fields.get(name).is_none() {
            let idx = self.fields.len();
            self.fields.insert(name.to_owned(), idx);
        }
    }
}

#[derive(Default, Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub(crate) struct FunId(u32);

#[derive(Default, Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub(crate) struct ClassId(u32);

impl From<usize> for FunId {
    fn from(id: usize) -> Self {
        // Closures keep this in a block header, which has room for 24 bits
        assert!(id <= MAX_AUX24, "too many functions to fit an id in a header");
        FunId(id.try_into().unwrap())
    }
}

impl From<FunId> for usize {
    fn from(id: FunId) -> Self {
        let FunId(id) = id;
        id as usize
    }
}

impl From<usize> for ClassId {
    fn from(id: usize) -> Self {
        // Objects keep this in a block header, which has room for 24 bits
        assert!(id <= MAX_AUX24, "too many classes to fit an id in a header");
        ClassId(id.try_into().unwrap())
    }
}

impl From<ClassId> for usize {
    fn from(id: ClassId) -> Self {
        let ClassId(id) = id;
        id as usize
    }
}

/// Constant class ids for basic classes
/// Note that id 0 is reserved as an unused value
pub(crate) const NIL_ID: ClassId = ClassId(1);
pub(crate) const BOOL_ID: ClassId = ClassId(2);
pub(crate) const INT64_ID: ClassId = ClassId(3);
pub(crate) const FLOAT64_ID: ClassId = ClassId(4);
pub(crate) const STRING_ID: ClassId = ClassId(5);
// Reserved for a future Object class
#[allow(dead_code)]
pub(crate) const OBJECT_ID: ClassId = ClassId(6);
pub(crate) const ARRAY_ID: ClassId = ClassId(7);
pub(crate) const BYTEARRAY_ID: ClassId = ClassId(8);
pub(crate) const DICT_ID: ClassId = ClassId(9);
pub(crate) const UIEVENT_ID: ClassId = ClassId(100);
pub(crate) const AUDIO_NEEDED_ID: ClassId = ClassId(101);
pub(crate) const AUDIO_DATA_ID: ClassId = ClassId(102);
pub(crate) const LAST_RESERVED_ID: usize = 0xFF;

#[derive(Default, Clone, Debug)]
pub(crate) struct Import
{
    // Full path to the imported unit
    pub(crate) full_path: String,

    // Imported symbols
    pub(crate) symbols: Vec<String>,

    // Import all symbols
    pub(crate) import_all: bool,

    // Source position
    pub(crate) pos: SrcPos,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct Unit
{
    // List of import directives
    pub(crate) imports: Vec<Import>,

    // Classes declared in this unit
    pub(crate) classes: HashMap<String, ClassId>,

    // Functions declared in this unit
    pub(crate) funs: HashMap<String, FunId>,

    // Top-level immutable globals declared in this unit
    pub(crate) consts: HashMap<String, u32>,

    // Unit-level (top level) function
    pub(crate) unit_fn: FunId,
}

/// Represents an entire program containing one or more units
#[derive(Clone, Debug)]
pub(crate) struct Program
{
    // Last id assigned
    // Zero is intentionally not used as an id
    last_id: usize,

    // Map of parsed units by name
    pub(crate) units: HashMap<String, Unit>,

    // Having a hash map of ids to functions means that we can
    // prune unreferenced functions (remove dead code)
    pub(crate) funs: HashMap<FunId, Function>,

    // Having a hash map of ids to functions means that we can
    // prune unreferenced classes (remove dead code)
    pub(crate) classes: HashMap<ClassId, Class>,

    // Unit function initialization order
    pub(crate) init_order: Vec<FunId>,

    // Number of global variable slots
    pub(crate) num_globals: u32,

    // Top-level unit function
    pub(crate) main_fn: FunId,

    // True when the program is being run in the REPL
    pub(crate) repl: bool,
}

impl Program
{
    pub(crate) fn new() -> Program
    {
        let mut prog = Self {
            last_id: LAST_RESERVED_ID,
            units: Default::default(),
            funs: Default::default(),
            classes: Default::default(),
            init_order: Default::default(),
            num_globals: Default::default(),
            main_fn: Default::default(),
            repl: false,
        };

        crate::libcore::init_runtime(&mut prog);
        prog
    }

    pub(crate) fn reg_fun(&mut self, mut fun: Function) -> FunId
    {
        self.last_id += 1;
        let id = self.last_id.into();
        fun.id = id;
        self.funs.insert(id, fun);
        id
    }

    pub(crate) fn reg_class(&mut self, mut class: Class) -> ClassId
    {
        // If the class doesn't have an id assigned yet
        if class.id == ClassId::default() {
            self.last_id += 1;
            let id = self.last_id.into();
            class.id = id;
        }

        let id = class.id;
        assert!(id != ClassId::default());
        assert!(!self.classes.contains_key(&id));
        self.classes.insert(id, class);
        id
    }
}
