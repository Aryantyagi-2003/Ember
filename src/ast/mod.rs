//! Ember AST — proposed shape, written for design review alongside
//! docs/LANGUAGE_SPEC.md. No parser exists yet; this is the target type
//! the hand-written recursive-descent parser will build and every later
//! stage (typecheck, interpreter) will match on exhaustively.

/// 1-indexed source location, threaded through every stage's diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

/// Surface type annotations as written by the programmer (§3, §11 `type`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeAnn {
    Int,
    Float,
    Bool,
    String,
    Array(Box<TypeAnn>),
    Function { params: Vec<TypeAnn>, ret: Box<TypeAnn> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub mutable: bool,
    pub ty: TypeAnn,
    pub span: Span,
}

/// A parsed function literal — shared by named `fn` items (§11: a named
/// `fn_decl` desugars to `let NAME = fn_lit`) and anonymous closures.
#[derive(Debug, Clone, PartialEq)]
pub struct FnLit {
    pub params: Vec<Param>,
    pub ret: TypeAnn,
    pub body: Box<Expr>, // always an ExprKind::Block
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StringLit(String),
    ArrayLit(Vec<Expr>),

    Ident(String),

    Unary { op: UnOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },

    /// `target = value` — target is restricted by the parser to an
    /// assignable place (Ident or Index); checked structurally rather
    /// than via a separate "place expression" AST node to keep the AST
    /// small. The type checker rejects non-place targets and non-`mut`
    /// targets with a location-carrying error.
    Assign { target: Box<Expr>, value: Box<Expr> },

    Call { callee: Box<Expr>, args: Vec<Expr> },
    Index { array: Box<Expr>, index: Box<Expr> },
    /// `receiver.method(args)` — the small closed set of array methods
    /// (`length`, `push`) from §9; not a general method-dispatch system.
    MethodCall { receiver: Box<Expr>, method: String, args: Vec<Expr>, span: Span },

    If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Option<Box<Expr>> },
    Block(Vec<Stmt>, Option<Box<Expr>>), // statements, optional trailing tail expr

    FnLit(FnLit),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Let { name: String, mutable: bool, ty: Option<TypeAnn>, value: Expr },
    While { cond: Expr, body: Expr }, // body is always ExprKind::Block
    Return(Option<Expr>),
    Expr(Expr), // expression-statement; `;`-terminated per grammar
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

/// A top-level item. Only function declarations exist in v1 (§11 `item`).
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    FnDecl { name: String, fn_lit: FnLit, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub items: Vec<Item>,
}
