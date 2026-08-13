//! Ember tree-walking interpreter — type-checked AST -> program execution.
//!
//! Runtime errors (division/modulo by zero, array index out of bounds —
//! genuinely runtime-only checks the type checker cannot perform) produce
//! a located `RuntimeError`, never a Rust panic, same discipline as every
//! prior stage. A handful of "should be impossible after type checking"
//! cases (e.g. calling a non-function `Value`) also produce a
//! `RuntimeError` rather than `unwrap`/`panic!`, defensively, since the
//! interpreter's real contract is "should be given checked input," not
//! "is guaranteed checked input by construction."
//!
//! Deep recursion (§6's ≥1000-level requirement) is handled by running
//! program execution on a dedicated thread with a large stack
//! (`run_program`), not by a trampoline or tail-call optimization — Ember
//! makes no TCO guarantee. See the README's known-limitations section
//! for the same note.

use crate::ast::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[cfg(test)]
mod tests;

/// The stack size given to the dedicated interpreter thread `run_program`
/// spawns, chosen to comfortably clear the ≥1000-level recursion bar (and
/// a good deal more) without a trampoline rewrite.
const INTERPRETER_STACK_SIZE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "runtime error at line {}, column {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

fn internal_type_mismatch(span: Span) -> Signal {
    Signal::Error(RuntimeError {
        message: "internal: value had an unexpected runtime type (should have been caught by the type checker)"
            .to_string(),
        span,
    })
}

/// The runtime value representation — distinct from both `ast::TypeAnn`
/// and `typecheck::Type`.
#[derive(Clone)]
pub enum Value {
    Unit,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(Rc<str>),
    /// Shared, mutable array storage — this is what makes §5's
    /// `push`/index-assignment semantics work: cloning a `Value::Array`
    /// clones the `Rc` (aliasing the same `Vec`), never deep-copies it.
    Array(Rc<RefCell<Vec<Value>>>),
    Function(Rc<Closure>),
    Builtin(Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Print,
    Println,
    Concat,
    StrLength,
    IntToFloat,
    FloatToInt,
    Panic,
    ToString,
}

/// A closure: the function literal it was created from, plus a reference
/// (an `Rc` clone, never a copy) to the environment active at its
/// definition site. This is the entire mechanism behind capture-by-
/// reference — see `Scope` below and the design walkthrough in the
/// module-level docs of the project's design discussion.
pub struct Closure {
    fn_lit: Rc<FnLit>,
    env: Scope,
}

// Deliberately not deriving Debug/PartialEq on Value via #[derive]: a
// recursive closure's captured Scope ends up containing a Value::Function
// that points back to that same Scope (a genuine Rc reference cycle —
// see the known-limitations note in the README once written). A derived
// Debug would recurse into that cycle and hang; these hand-written impls
// stay shallow on Function on purpose.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Unit => write!(f, "()"),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(x) => write!(f, "{}", x),
            Value::Bool(b) => write!(f, "{}", b),
            Value::String(s) => write!(f, "{:?}", s),
            Value::Array(rc) => write!(f, "{:?}", rc.borrow()),
            Value::Function(_) => write!(f, "<function>"),
            Value::Builtin(b) => write!(f, "<builtin {:?}>", b),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Unit, Value::Unit) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            // Arrays can never contain a cycle (Ember has no recursive
            // types in v1), so content equality is safe and terminates.
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
            // Functions compare by identity, not structurally — avoids
            // ever touching a captured (possibly cyclic) environment.
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::Builtin(a), Value::Builtin(b)) => a == b,
            _ => false,
        }
    }
}

/// One variable binding — its own `Rc<RefCell<Value>>` cell, not just a
/// `Value` stored directly in the scope's map. This is what makes
/// mutation-after-capture *structurally* correct: a closure that captures
/// the `Scope` also, transitively, shares this exact cell for every
/// binding already declared in it, so a later mutation writes through
/// the same cell the closure will read from.
type Cell = Rc<RefCell<Value>>;

struct ScopeData {
    vars: HashMap<String, Cell>,
    parent: Option<Scope>,
}

/// A lexical scope frame. Cloning a `Scope` clones the `Rc` pointer, not
/// the frame's contents — this is the entire mechanism behind "a closure
/// captures its enclosing environment by reference."
#[derive(Clone)]
struct Scope(Rc<RefCell<ScopeData>>);

impl Scope {
    fn root() -> Scope {
        Scope(Rc::new(RefCell::new(ScopeData {
            vars: HashMap::new(),
            parent: None,
        })))
    }

    fn child(&self) -> Scope {
        Scope(Rc::new(RefCell::new(ScopeData {
            vars: HashMap::new(),
            parent: Some(self.clone()),
        })))
    }

    /// Binds `name` to a *new* cell in this scope (shadowing any binding
    /// of the same name in this or an outer scope, per normal lexical
    /// shadowing rules).
    fn declare(&self, name: String, value: Value) {
        self.0
            .borrow_mut()
            .vars
            .insert(name, Rc::new(RefCell::new(value)));
    }

    fn lookup_cell(&self, name: &str) -> Option<Cell> {
        let data = self.0.borrow();
        if let Some(cell) = data.vars.get(name) {
            return Some(cell.clone());
        }
        data.parent.as_ref().and_then(|p| p.lookup_cell(name))
    }

    fn get(&self, name: &str) -> Option<Value> {
        self.lookup_cell(name).map(|c| c.borrow().clone())
    }

    /// Mutates the cell of an *already-declared* binding in place,
    /// wherever in the scope chain it lives. Returns `false` if no such
    /// binding exists (defensive — the checker guarantees this can't
    /// happen for a checked program).
    fn assign(&self, name: &str, value: Value) -> bool {
        match self.lookup_cell(name) {
            Some(cell) => {
                *cell.borrow_mut() = value;
                true
            }
            None => false,
        }
    }
}

fn register_builtins(scope: &Scope) {
    scope.declare("print".to_string(), Value::Builtin(Builtin::Print));
    scope.declare("println".to_string(), Value::Builtin(Builtin::Println));
    scope.declare("concat".to_string(), Value::Builtin(Builtin::Concat));
    scope.declare("str_length".to_string(), Value::Builtin(Builtin::StrLength));
    scope.declare(
        "int_to_float".to_string(),
        Value::Builtin(Builtin::IntToFloat),
    );
    scope.declare(
        "float_to_int".to_string(),
        Value::Builtin(Builtin::FloatToInt),
    );
    scope.declare("panic".to_string(), Value::Builtin(Builtin::Panic));
    // Unlike the type checker (which special-cases `to_string` as an
    // intrinsic because static overloading is genuinely a problem for a
    // one-signature-per-name environment), the interpreter has no such
    // issue — dynamic dispatch on the argument's runtime tag is trivial,
    // so `to_string` is just another Builtin here, handled uniformly
    // with everything else in `call_builtin`.
    scope.declare("to_string".to_string(), Value::Builtin(Builtin::ToString));
}

/// Internal control-flow channel. `Signal::Return` unwinds through
/// ordinary `?` propagation from an arbitrarily-nested `return` statement
/// up to the enclosing `call_closure`, which is the only place it's
/// caught — everywhere in between (blocks, if/else branches, while
/// bodies) just needs to propagate it via `?`, no special-casing.
enum Signal {
    Error(RuntimeError),
    Return(Value),
}

impl From<RuntimeError> for Signal {
    fn from(e: RuntimeError) -> Self {
        Signal::Error(e)
    }
}

type EvalResult = Result<Value, Signal>;

pub struct Interpreter {
    output: RefCell<String>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            output: RefCell::new(String::new()),
        }
    }

    /// Everything written by `print`/`println` so far.
    pub fn output(&self) -> String {
        self.output.borrow().clone()
    }

    /// Hoists every top-level function (§6.1, extended to the top level —
    /// see LANGUAGE_SPEC.md), then calls `main()`.
    pub fn run(&self, program: &Program) -> Result<Value, RuntimeError> {
        let root = Scope::root();
        register_builtins(&root);
        for item in &program.items {
            let Item::FnDecl { name, fn_lit, .. } = item;
            let closure = Value::Function(Rc::new(Closure {
                fn_lit: Rc::new(fn_lit.clone()),
                env: root.clone(),
            }));
            root.declare(name.clone(), closure);
        }
        let entry_span = Span { line: 1, col: 1 };
        let main = root.get("main").ok_or_else(|| RuntimeError {
            message: "no `main` function defined".to_string(),
            span: entry_span,
        })?;
        match main {
            Value::Function(closure) => self.call_closure(&closure, Vec::new(), entry_span),
            _ => Err(RuntimeError {
                message: "`main` is not callable".to_string(),
                span: entry_span,
            }),
        }
    }

    fn call_closure(
        &self,
        closure: &Closure,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        // Defensive check, not load-bearing for a checked program (the
        // type checker already guarantees matching arity) — kept so a
        // mismatch is a clean RuntimeError rather than silently
        // truncating extra args or leaving trailing params undeclared
        // (which `zip` alone would do) if this is ever reached from
        // unchecked AST.
        if args.len() != closure.fn_lit.params.len() {
            return Err(RuntimeError {
                message: format!(
                    "internal: expected {} argument(s), found {} (should have been caught by the type checker)",
                    closure.fn_lit.params.len(),
                    args.len()
                ),
                span,
            });
        }
        let call_scope = closure.env.child();
        for (param, val) in closure.fn_lit.params.iter().zip(args) {
            call_scope.declare(param.name.clone(), val);
        }
        let body_result = match &closure.fn_lit.body.kind {
            ExprKind::Block(stmts, tail) => {
                self.eval_block_contents(stmts, tail.as_deref(), &call_scope)
            }
            // Parser invariant: a fn_lit's body is always parsed via
            // parse_block, so it is always ExprKind::Block.
            _ => unreachable!("parser guarantees a fn_lit body is always ExprKind::Block"),
        };
        match body_result {
            Ok(v) => Ok(v),
            Err(Signal::Return(v)) => Ok(v),
            Err(Signal::Error(e)) => Err(e),
        }
    }

    /// Evaluates a block's statements and tail expression, assuming the
    /// caller already pushed (or reused, for function bodies) the scope
    /// `env` — mirrors `check_block_contents` in the type checker,
    /// including §6.1's two-pass hoisting: pass 1 creates and binds every
    /// sibling `fn_decl`'s closure over `env` *before* pass 2 executes
    /// any statement, so mutually recursive siblings can call each other
    /// regardless of textual order — and, critically, each such closure's
    /// captured `env` is `env` itself, the very scope pass 1 is about to
    /// insert its own binding into, which is what makes the call
    /// resolvable at all once pass 2 (or a later call) actually runs it.
    fn eval_block_contents(&self, stmts: &[Stmt], tail: Option<&Expr>, env: &Scope) -> EvalResult {
        for stmt in stmts {
            if let StmtKind::FnDecl { name, fn_lit } = &stmt.kind {
                let closure = Value::Function(Rc::new(Closure {
                    fn_lit: Rc::new(fn_lit.clone()),
                    env: env.clone(),
                }));
                env.declare(name.clone(), closure);
            }
        }
        for stmt in stmts {
            self.eval_stmt(stmt, env)?;
        }
        match tail {
            Some(e) => self.eval_expr(e, env),
            None => Ok(Value::Unit),
        }
    }

    fn eval_stmt(&self, stmt: &Stmt, env: &Scope) -> Result<(), Signal> {
        match &stmt.kind {
            StmtKind::Let { name, value, .. } => {
                let v = self.eval_expr(value, env)?;
                env.declare(name.clone(), v);
                Ok(())
            }
            StmtKind::While { cond, body } => {
                loop {
                    let c = self.eval_expr(cond, env)?;
                    match c {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        _ => return Err(internal_type_mismatch(cond.span)),
                    }
                    // `body` is always a Block; a `return` inside it
                    // propagates straight out via `?`, ending the loop.
                    self.eval_expr(body, env)?;
                }
                Ok(())
            }
            StmtKind::Return(value) => {
                let v = match value {
                    Some(e) => self.eval_expr(e, env)?,
                    None => Value::Unit,
                };
                Err(Signal::Return(v))
            }
            // Already bound by this block's own pass 1 (or, for a
            // top-level fn_decl, by `Interpreter::run`'s own hoisting
            // pass) — nothing to do here.
            StmtKind::FnDecl { .. } => Ok(()),
            StmtKind::Expr(e) => {
                self.eval_expr(e, env)?;
                Ok(())
            }
        }
    }

    fn eval_expr(&self, expr: &Expr, env: &Scope) -> EvalResult {
        match &expr.kind {
            ExprKind::IntLit(n) => Ok(Value::Int(*n)),
            ExprKind::FloatLit(x) => Ok(Value::Float(*x)),
            ExprKind::BoolLit(b) => Ok(Value::Bool(*b)),
            ExprKind::StringLit(s) => Ok(Value::String(Rc::from(s.as_str()))),
            ExprKind::ArrayLit(elems) => {
                let mut vals = Vec::with_capacity(elems.len());
                for e in elems {
                    vals.push(self.eval_expr(e, env)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vals))))
            }
            ExprKind::Ident(name) => env.get(name).ok_or_else(|| {
                Signal::Error(RuntimeError {
                    message: format!("undeclared variable '{}'", name),
                    span: expr.span,
                })
            }),
            ExprKind::Unary { op, expr: inner } => self.eval_unary(*op, inner, env),
            ExprKind::Binary { op, lhs, rhs } => self.eval_binary(*op, lhs, rhs, expr.span, env),
            ExprKind::Assign { target, value } => self.eval_assign(target, value, env),
            ExprKind::Call { callee, args } => self.eval_call(callee, args, expr.span, env),
            ExprKind::Index { array, index } => self.eval_index(array, index, env),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
                span,
            } => self.eval_method_call(receiver, method, args, *span, env),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.eval_expr(cond, env)?;
                match c {
                    Value::Bool(true) => self.eval_expr(then_branch, env),
                    Value::Bool(false) => match else_branch {
                        Some(e) => self.eval_expr(e, env),
                        None => Ok(Value::Unit),
                    },
                    _ => Err(internal_type_mismatch(cond.span)),
                }
            }
            ExprKind::Block(stmts, tail) => {
                let child = env.child();
                self.eval_block_contents(stmts, tail.as_deref(), &child)
            }
            ExprKind::FnLit(fn_lit) => Ok(Value::Function(Rc::new(Closure {
                fn_lit: Rc::new(fn_lit.clone()),
                env: env.clone(),
            }))),
        }
    }

    fn eval_unary(&self, op: UnOp, inner: &Expr, env: &Scope) -> EvalResult {
        let v = self.eval_expr(inner, env)?;
        match (op, v) {
            (UnOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
            (UnOp::Neg, Value::Float(x)) => Ok(Value::Float(-x)),
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err(internal_type_mismatch(inner.span)),
        }
    }

    /// Left-to-right evaluation throughout: `lhs` fully evaluates (side
    /// effects and all) before `rhs` begins — except `&&`/`||`, which
    /// short-circuit (the standard, expected behavior): `rhs` is not
    /// evaluated at all when `lhs` alone already determines the result.
    fn eval_binary(
        &self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        env: &Scope,
    ) -> EvalResult {
        if op == BinOp::And || op == BinOp::Or {
            let l = self.eval_expr(lhs, env)?;
            return match (op, l) {
                (BinOp::And, Value::Bool(false)) => Ok(Value::Bool(false)),
                (BinOp::Or, Value::Bool(true)) => Ok(Value::Bool(true)),
                (BinOp::And, Value::Bool(true)) | (BinOp::Or, Value::Bool(false)) => {
                    match self.eval_expr(rhs, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        _ => Err(internal_type_mismatch(rhs.span)),
                    }
                }
                _ => Err(internal_type_mismatch(lhs.span)),
            };
        }

        let l = self.eval_expr(lhs, env)?;
        let r = self.eval_expr(rhs, env)?;
        match op {
            BinOp::Add => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Sub => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Mul => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Div => match (l, r) {
                (Value::Int(a), Value::Int(b)) => {
                    if b == 0 {
                        return Err(Signal::Error(RuntimeError {
                            message: "division by zero".to_string(),
                            span,
                        }));
                    }
                    Ok(Value::Int(a / b))
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Mod => match (l, r) {
                (Value::Int(a), Value::Int(b)) => {
                    if b == 0 {
                        return Err(Signal::Error(RuntimeError {
                            message: "modulo by zero".to_string(),
                            span,
                        }));
                    }
                    Ok(Value::Int(a % b))
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Lt => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Le => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Gt => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Ge => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Eq => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a == b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a == b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a == b)),
                (Value::String(a), Value::String(b)) => Ok(Value::Bool(a == b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::Ne => match (l, r) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a != b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a != b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a != b)),
                (Value::String(a), Value::String(b)) => Ok(Value::Bool(a != b)),
                _ => Err(internal_type_mismatch(span)),
            },
            BinOp::And | BinOp::Or => unreachable!("handled above"),
        }
    }

    /// Evaluation order for a place target: for `xs[i] = v`, `xs` then
    /// `i` then `v`, left to right, matching every other left-to-right
    /// rule in the interpreter.
    fn eval_assign(&self, target: &Expr, value: &Expr, env: &Scope) -> EvalResult {
        match &target.kind {
            ExprKind::Ident(name) => {
                let v = self.eval_expr(value, env)?;
                if !env.assign(name, v.clone()) {
                    return Err(Signal::Error(RuntimeError {
                        message: format!("undeclared variable '{}'", name),
                        span: target.span,
                    }));
                }
                Ok(v)
            }
            ExprKind::Index { array, index } => {
                let arr_val = self.eval_expr(array, env)?;
                let idx_val = self.eval_expr(index, env)?;
                let v = self.eval_expr(value, env)?;
                let idx = match idx_val {
                    Value::Int(n) => n,
                    _ => return Err(internal_type_mismatch(index.span)),
                };
                match arr_val {
                    Value::Array(rc) => {
                        let mut vec = rc.borrow_mut();
                        let len = vec.len();
                        if idx < 0 || (idx as usize) >= len {
                            return Err(Signal::Error(RuntimeError {
                                message: format!(
                                    "array index out of bounds: index {}, length {}",
                                    idx, len
                                ),
                                span: index.span,
                            }));
                        }
                        vec[idx as usize] = v.clone();
                        Ok(v)
                    }
                    _ => Err(internal_type_mismatch(array.span)),
                }
            }
            // Parser/checker invariant: assignment targets are always
            // Ident or Index (see ast::ExprKind::Assign's doc comment).
            _ => unreachable!("parser guarantees assignment targets are Ident or Index"),
        }
    }

    /// Evaluation order: callee first, then arguments strictly
    /// left-to-right.
    fn eval_call(&self, callee: &Expr, args: &[Expr], span: Span, env: &Scope) -> EvalResult {
        let callee_val = self.eval_expr(callee, env)?;
        let mut arg_vals = Vec::with_capacity(args.len());
        for a in args {
            arg_vals.push(self.eval_expr(a, env)?);
        }
        match callee_val {
            Value::Function(closure) => self.call_closure(&closure, arg_vals, span).map_err(Signal::Error),
            Value::Builtin(b) => self.call_builtin(b, arg_vals, span).map_err(Signal::Error),
            _ => Err(Signal::Error(RuntimeError {
                message: "internal: attempted to call a non-function value (should have been caught by the type checker)".to_string(),
                span,
            })),
        }
    }

    fn call_builtin(
        &self,
        b: Builtin,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        let as_string = |v: &Value| -> Result<Rc<str>, RuntimeError> {
            match v {
                Value::String(s) => Ok(s.clone()),
                _ => Err(RuntimeError {
                    message: "internal: expected a string argument".to_string(),
                    span,
                }),
            }
        };
        match b {
            Builtin::Print => {
                let s = as_string(&args[0])?;
                self.output.borrow_mut().push_str(&s);
                Ok(Value::Unit)
            }
            Builtin::Println => {
                let s = as_string(&args[0])?;
                let mut out = self.output.borrow_mut();
                out.push_str(&s);
                out.push('\n');
                Ok(Value::Unit)
            }
            Builtin::Concat => {
                let a = as_string(&args[0])?;
                let b = as_string(&args[1])?;
                Ok(Value::String(Rc::from(format!("{}{}", a, b))))
            }
            Builtin::StrLength => {
                let s = as_string(&args[0])?;
                Ok(Value::Int(s.chars().count() as i64))
            }
            Builtin::IntToFloat => match &args[0] {
                Value::Int(n) => Ok(Value::Float(*n as f64)),
                _ => Err(RuntimeError {
                    message: "internal: expected an int argument".to_string(),
                    span,
                }),
            },
            Builtin::FloatToInt => match &args[0] {
                Value::Float(x) => Ok(Value::Int(*x as i64)),
                _ => Err(RuntimeError {
                    message: "internal: expected a float argument".to_string(),
                    span,
                }),
            },
            Builtin::Panic => {
                let s = as_string(&args[0])?;
                Err(RuntimeError {
                    message: format!("panic: {}", s),
                    span,
                })
            }
            Builtin::ToString => match &args[0] {
                Value::Int(n) => Ok(Value::String(Rc::from(n.to_string()))),
                Value::Float(x) => Ok(Value::String(Rc::from(x.to_string()))),
                Value::Bool(b) => Ok(Value::String(Rc::from(b.to_string()))),
                _ => Err(RuntimeError {
                    message: "internal: to_string called with a non-primitive argument (should have been caught by the type checker)".to_string(),
                    span,
                }),
            },
        }
    }

    fn eval_index(&self, array: &Expr, index: &Expr, env: &Scope) -> EvalResult {
        let arr_val = self.eval_expr(array, env)?;
        let idx_val = self.eval_expr(index, env)?;
        let idx = match idx_val {
            Value::Int(n) => n,
            _ => return Err(internal_type_mismatch(index.span)),
        };
        match arr_val {
            Value::Array(rc) => {
                let vec = rc.borrow();
                if idx < 0 || (idx as usize) >= vec.len() {
                    return Err(Signal::Error(RuntimeError {
                        message: format!(
                            "array index out of bounds: index {}, length {}",
                            idx,
                            vec.len()
                        ),
                        span: index.span,
                    }));
                }
                Ok(vec[idx as usize].clone())
            }
            _ => Err(internal_type_mismatch(array.span)),
        }
    }

    fn eval_method_call(
        &self,
        receiver: &Expr,
        method: &str,
        args: &[Expr],
        span: Span,
        env: &Scope,
    ) -> EvalResult {
        let recv = self.eval_expr(receiver, env)?;
        match recv {
            Value::Array(rc) => match method {
                "length" => Ok(Value::Int(rc.borrow().len() as i64)),
                "push" => {
                    let v = self.eval_expr(&args[0], env)?;
                    rc.borrow_mut().push(v);
                    Ok(Value::Unit)
                }
                _ => Err(internal_type_mismatch(span)),
            },
            _ => Err(internal_type_mismatch(span)),
        }
    }
}

/// Runs `program` (hoisting top-level functions, then calling `main()`)
/// on a dedicated thread with a large stack, so straightforward deep
/// recursion (≥1000 levels, per §6) does not overflow the host process's
/// default thread stack. This is a deliberate choice over a trampoline —
/// see the module docs and README's known-limitations section: Ember
/// makes no tail-call-optimization guarantee, and a trampoline only
/// helps tail-shaped recursion specifically, so it wouldn't even cover
/// the general case a large stack does.
///
/// Returns the captured `print`/`println` output and the run's result.
/// `main`'s return `Value` itself never crosses the thread boundary — it
/// can hold an `Rc`, which is not `Send` — only `Send`-safe data
/// (`String`, `RuntimeError`) does.
pub fn run_program(program: Program) -> (String, Result<(), RuntimeError>) {
    let handle = std::thread::Builder::new()
        .stack_size(INTERPRETER_STACK_SIZE)
        .spawn(move || {
            let interp = Interpreter::new();
            let result = interp.run(&program).map(|_| ());
            (interp.output(), result)
        })
        // Spawning a thread failing is an OS-level resource-exhaustion
        // condition, not something driven by Ember program input — not
        // the kind of failure this project's "never panic on program
        // input" discipline is about.
        .expect("failed to spawn interpreter thread");
    // If the interpreter itself panicked, that's a genuine bug in
    // Ember's own implementation (every user-input-triggered failure
    // path already returns RuntimeError instead of panicking) — letting
    // that panic propagate here, rather than swallowing it, is the
    // correct behavior per the project's own panic-vs-error policy.
    handle.join().expect("interpreter thread panicked")
}
