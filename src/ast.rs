use std::fmt;
use std::collections::{HashMap, HashSet};
use crate::lexer::SrcPos;
use crate::symbols::Decl;
use crate::host::HostFn;

/// Unary operator
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UnOp
{
    Minus,
    Not,
}

/// Binary operator
/// https://en.cppreference.com/w/c/language/operator_precedence
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BinOp
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
    IntDiv,
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

    // Assignment
    Assign,
}

/// Expression
#[derive(Clone, Debug)]
pub enum Expr
{
    True,
    False,
    Nil,
    Int64(i64),
    Float64(f64),
    String(String),

    // Host function
    HostFn(HostFn),

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

    // Reference to a variable/function declaration
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
pub struct ExprBox
{
    pub expr: Box<Expr>,
    pub pos: SrcPos,
}

impl ExprBox
{
    pub fn new(expr: Expr, pos: SrcPos) -> Self
    {
        Self {
            expr: Box::new(expr),
            pos,
        }
    }

    pub fn new_ok<E>(expr: Expr, pos: SrcPos) -> Result<Self, E>
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
pub enum Stmt
{
    Expr(ExprBox),

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
        incr_expr: ExprBox,
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
pub struct StmtBox
{
    pub stmt: Box<Stmt>,
    pub pos: SrcPos,
}

impl StmtBox
{
    pub fn new(stmt: Stmt, pos: SrcPos) -> Self
    {
        Self {
            stmt: Box::new(stmt),
            pos,
        }
    }

    pub fn new_ok<E>(stmt: Stmt, pos: SrcPos) -> Result<Self, E>
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
pub struct Function
{
    /// Name of the function
    pub name: String,

    /// Parameter list
    pub params: Vec<String>,

    /// Variadic function, variable argument count
    pub var_arg: bool,

    /// Body of the function
    pub body: StmtBox,

    /// Number of local variables
    pub num_locals: usize,

    /// Map of captured closure variables to closure slots indices
    pub captured: HashMap<Decl, u32>,

    /// Set of mutable local variables which are captured by a nested function
    /// Note that this only applies to mutable locals which need a mutable closure cell
    pub escaping: HashSet<Decl>,

    /// Unit-level (global) function
    pub is_unit: bool,

    // Source position
    pub pos: SrcPos,

    /// Internal unique function id
    pub id: FunId,

    /// Class id this function is associated with
    /// This will be zero is none
    pub class_id: ClassId,
}

impl Function
{
    /// Register a captured closure variable and return its slot index
    pub fn reg_captured(&mut self, decl: &Decl) -> u32
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
    pub fn is_ctor(&self) -> bool
    {
        return (
            self.class_id != ClassId::default() &&
            self.name == "init"
        );
    }
}

#[derive(Default, Clone, Debug)]
pub struct Class
{
    pub name: String,

    // Map of field names to slot indices
    pub fields: HashMap<String, usize>,

    // Map of method names to function ids
    pub methods: HashMap<String, FunId>,

    // Source position
    pub pos: SrcPos,

    // Internal unique class id
    pub id: ClassId,
}

impl Class
{
    pub fn reg_field(&mut self, name: &str)
    {
        assert!(self.id.0 != 0);
        if self.fields.get(name).is_none() {
            let idx = self.fields.len();
            self.fields.insert(name.to_owned(), idx);
        }
    }
}

#[derive(Default, Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct FunId(u32);

#[derive(Default, Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct ClassId(u32);

impl From<usize> for FunId {
    fn from(id: usize) -> Self {
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
pub const NIL_ID: ClassId = ClassId(1);
pub const OBJECT_ID: ClassId = ClassId(2);
pub const INT64_ID: ClassId = ClassId(3);
pub const FLOAT64_ID: ClassId = ClassId(4);
pub const STRING_ID: ClassId = ClassId(5);
pub const ARRAY_ID: ClassId = ClassId(6);
pub const BYTEARRAY_ID: ClassId = ClassId(7);
pub const UIEVENT_ID: ClassId = ClassId(8);
pub const AUDIO_NEEDED_ID: ClassId = ClassId(9);
pub const LAST_RESERVED_ID: usize = 0xFF;

#[derive(Default, Clone, Debug)]
pub struct Unit
{
    // TODO: list of imports. Don't implement just yet.
    // We'll probably want to import specific symbols from units

    // Classes declared in this unit
    pub classes: HashMap<String, ClassId>,

    // Unit-level (top level) function
    pub unit_fn: FunId,
}

/// Represents an entire program containing one or more units
#[derive(Clone, Debug)]
pub struct Program
{
    // Last id assigned
    // Zero is intentionally not used as an id
    last_id: usize,

    // Map of parsed units by name
    pub units: HashMap<String, Unit>,

    // Having a hash map of ids to functions means that we can
    // prune unreferenced functions (remove dead code)
    pub funs: HashMap<FunId, Function>,

    // Having a hash map of ids to functions means that we can
    // prune unreferenced classes (remove dead code)
    pub classes: HashMap<ClassId, Class>,

    // Number of global variable slots
    pub num_globals: usize,

    // Main/top-level unit
    pub main_unit: Unit,

    // Top-level unit function
    pub main_fn: FunId,
}

impl Program
{
    pub fn new() -> Program
    {
        let mut prog = Self {
            last_id: LAST_RESERVED_ID,
            units: Default::default(),
            funs: Default::default(),
            classes: Default::default(),
            num_globals: Default::default(),
            main_unit: Default::default(),
            main_fn: Default::default(),
        };

        crate::runtime::init_runtime(&mut prog);
        prog
    }

    pub fn reg_fun(&mut self, mut fun: Function) -> FunId
    {
        self.last_id += 1;
        let id = self.last_id.into();
        fun.id = id;
        self.funs.insert(id, fun);
        id
    }

    pub fn reg_class(&mut self, mut class: Class) -> ClassId
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
