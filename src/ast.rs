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
        frozen: bool,
        exprs: Vec<ExprBox>,
    },

    /*
    // Object literal
    Object {
        extensible: bool,
        fields: Vec<(bool, String, ExprBox)>,
    },
    */

    Ident(String),

    // Reference to a variable/function declaration
    Ref(Decl),

    // Function/closure expression
    Fun(Box<Function>),

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

    // New class instance
    New {
        // Class is initially an ident, but
        // may need to be resolved to a class id?
        class: ExprBox,
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
    pub id: usize,
}

/// Function
#[derive(Clone, Debug)]
pub struct Class
{
    /// Name of the class
    pub name: String,

    // Methods
    pub methods: HashMap<String, Function>,

    // Source position
    pub pos: SrcPos,
}

use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

pub fn next_id() -> usize
{
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub struct Unit
{
    // TODO: list of imports?

    // TODO: need list of classes

    // Unit-level function
    pub unit_fn: Function,
}
