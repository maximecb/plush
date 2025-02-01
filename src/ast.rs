use std::fmt;
use std::collections::HashMap;
use crate::parsing::SrcPos;
use crate::symbols::Decl;

/// Unary operator
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UnOp
{
    Minus,
    Not,
    TypeOf,
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
    Int(i128),
    Float64(f64),
    String(String),

    // Array literal
    Array {
        exprs: Vec<ExprBox>,
    },

    // Object literal
    Object {
        fields: Vec<(bool, String, ExprBox)>,
    },

    Ident(String),

    // Reference to a variable/function declaration
    Ref(Decl),

    // Function/closure expression
    Fun(FunId),

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

    Unary {
        op: UnOp,
        child: ExprBox,
    },

    Binary {
        op: BinOp,
        lhs: ExprBox,
        rhs: ExprBox,
    },

    /*
    Ternary {
        test_expr: ExprBox,
        then_expr: ExprBox,
        else_expr: ExprBox,
    },
    */

    Call {
        callee: ExprBox,
        args: Vec<ExprBox>,
    },

    HostCall {
        fun_name: String,
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

    While {
        test_expr: ExprBox,
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
    }
}

impl Default for Stmt
{
    fn default() -> Self
    {
        Stmt::Return(ExprBox::default())
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
#[derive(Clone, Debug)]
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

    /// Unit-level (global) function
    pub is_unit: bool,

    // Source position
    pub pos: SrcPos,

    /// Internal unique function id
    pub id: FunId,
}

#[derive(Default, Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct FunId(usize);

impl From<usize> for FunId {
    fn from(id: usize) -> Self {
        FunId(id)
    }
}

#[derive(Default, Clone, Debug)]
pub struct Unit
{
    // TODO: list of imports. Don't implement just yet.
    // These should be unit names
    // We'll want to import specific symbols from units

    // Unit-level (top level) function
    pub unit_fn: FunId,
}

/// Represents an entire program containing one or more units
#[derive(Default, Clone, Debug)]
pub struct Program
{
    // Last id assigned
    last_id: usize,

    // Map of parsed units by name
    pub units: HashMap<String, Unit>,

    // Having a hash map of ids to functions means that we can
    // prune unreferenced functions (remove dead code)
    pub funs: HashMap<FunId, Function>,

    // Main/top-level unit
    pub main_unit: Unit,

    // Top-level unit function
    pub main_fn: FunId,
}

impl Program
{
    pub fn reg_fun(&mut self, mut fun: Function) -> FunId
    {
        let id = self.last_id.into();
        self.last_id += 1;
        fun.id = id;
        self.funs.insert(id, fun);
        id
    }
}
