//! Ember type checker — AST -> either nothing (success) or a list of
//! located `TypeError`s (docs/LANGUAGE_SPEC.md's type-system sections).
//!
//! Unlike the lexer/parser, this stage does **not** stop at the first
//! error (§8: "the checker reports every error it finds, not just the
//! first"). `check_expr` therefore never returns `Result` — it always
//! returns *some* `Type`, using the `Type::Error` poison marker when a
//! sub-expression fails, so one root-cause mistake doesn't cascade into a
//! flood of spurious follow-on diagnostics.

use crate::ast::*;
use std::collections::HashMap;
use std::fmt;

#[cfg(test)]
mod tests;

/// The checker's internal semantic type, distinct from `ast::TypeAnn`
/// (the surface syntax the parser produces). Keeping them separate means
/// future stretch goals (HM inference, generics) can grow `Type` without
/// touching the AST.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Unit,
    Int,
    Float,
    Bool,
    String,
    Array(Box<Type>),
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
    },
    /// The declared return type of `panic` (§8/§9: `-> never`). Compatible
    /// with any expected type, same as `Error`, but — unlike `Error` — its
    /// presence is not itself a mistake; it just means "this expression
    /// never actually produces a value; don't require the branches
    /// around it to agree with a value it will never have."
    Never,
    /// Poison marker for error recovery. Never appears in a successfully
    /// checked program. Compatible with everything so that one root-cause
    /// error doesn't cascade into unrelated follow-on diagnostics.
    Error,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "()"),
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Array(t) => write!(f, "[{}]", t),
            Type::Function { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            Type::Never => write!(f, "never"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

impl Type {
    fn from_ann(ann: &TypeAnn) -> Type {
        match ann {
            TypeAnn::Unit => Type::Unit,
            TypeAnn::Int => Type::Int,
            TypeAnn::Float => Type::Float,
            TypeAnn::Bool => Type::Bool,
            TypeAnn::String => Type::String,
            TypeAnn::Array(t) => Type::Array(Box::new(Type::from_ann(t))),
            TypeAnn::Function { params, ret } => Type::Function {
                params: params.iter().map(Type::from_ann).collect(),
                ret: Box::new(Type::from_ann(ret)),
            },
        }
    }
}

fn types_compatible(a: &Type, b: &Type) -> bool {
    a == b
        || matches!(a, Type::Error)
        || matches!(b, Type::Error)
        || matches!(a, Type::Never)
        || matches!(b, Type::Never)
}

fn is_primitive(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Float | Type::Bool | Type::String)
}

fn bin_op_symbol(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn signature_type(fn_lit: &FnLit) -> Type {
    Type::Function {
        params: fn_lit
            .params
            .iter()
            .map(|p| Type::from_ann(&p.ty))
            .collect(),
        ret: Box::new(Type::from_ann(&fn_lit.ret)),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "type error at line {}, column {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

#[derive(Clone)]
struct VarInfo {
    ty: Type,
    mutable: bool,
}

struct TypeEnv {
    scopes: Vec<HashMap<String, VarInfo>>,
}

impl TypeEnv {
    fn new() -> Self {
        // One root scope, holding builtins and top-level function
        // signatures.
        TypeEnv {
            scopes: vec![HashMap::new()],
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: String, ty: Type, mutable: bool) {
        // Internal invariant: `new()` seeds one scope and `push_scope`/
        // `pop_scope` are always balanced within the checker's own code,
        // so at least one scope is always present.
        self.scopes
            .last_mut()
            .expect("TypeEnv always has at least one scope")
            .insert(name, VarInfo { ty, mutable });
    }

    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
}

/// Type-check a complete program, returning every error found (empty
/// means the program type-checks successfully). Top-level function
/// declarations are hoisted the same way §6.1 hoists sibling `fn_decl`s
/// within a block — this is a natural, consistent extension of that rule
/// to the outermost scope, so top-level functions can call each other
/// regardless of declaration order too.
pub fn check_program(program: &Program) -> Vec<TypeError> {
    let mut checker = Checker::new();

    for item in &program.items {
        let Item::FnDecl { name, fn_lit, .. } = item;
        let ty = signature_type(fn_lit);
        checker.env.declare(name.clone(), ty, false);
    }
    for item in &program.items {
        let Item::FnDecl { fn_lit, .. } = item;
        checker.check_fn_lit(fn_lit);
    }

    checker.errors
}

struct Checker {
    env: TypeEnv,
    return_type_stack: Vec<Type>,
    errors: Vec<TypeError>,
}

impl Checker {
    fn new() -> Self {
        let mut checker = Checker {
            env: TypeEnv::new(),
            return_type_stack: Vec::new(),
            errors: Vec::new(),
        };
        checker.register_builtins();
        checker
    }

    fn register_builtins(&mut self) {
        use Type::*;
        let f = |params: Vec<Type>, ret: Type| Function {
            params,
            ret: Box::new(ret),
        };
        self.env
            .declare("print".to_string(), f(vec![String], Unit), false);
        self.env
            .declare("println".to_string(), f(vec![String], Unit), false);
        self.env
            .declare("concat".to_string(), f(vec![String, String], String), false);
        self.env
            .declare("str_length".to_string(), f(vec![String], Int), false);
        self.env
            .declare("int_to_float".to_string(), f(vec![Int], Float), false);
        self.env
            .declare("float_to_int".to_string(), f(vec![Float], Int), false);
        // `panic`'s declared return type is `never` (§8/§9): it's
        // compatible with any expected type, so `panic("msg")` can be
        // used as a branch's tail without forcing the other branch to
        // also produce `()`.
        self.env
            .declare("panic".to_string(), f(vec![String], Never), false);
        // `to_string` is intentionally *not* registered here: it's
        // handled as a compiler-recognized intrinsic in `check_call`
        // (see §5/§9's note on this being the one deliberate exception
        // to "no overloading"), since it needs three different argument
        // types under one name, which this environment model can't
        // otherwise express.
    }

    fn error(&mut self, message: String, span: Span) -> Type {
        self.errors.push(TypeError { message, span });
        Type::Error
    }

    fn is_mut_root(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => self.env.lookup(name).map(|i| i.mutable).unwrap_or(false),
            ExprKind::Index { array, .. } => self.is_mut_root(array),
            _ => false,
        }
    }

    fn check_fn_lit(&mut self, fn_lit: &FnLit) -> Type {
        self.env.push_scope();
        let mut param_types = Vec::new();
        for p in &fn_lit.params {
            let ty = Type::from_ann(&p.ty);
            self.env.declare(p.name.clone(), ty.clone(), p.mutable);
            param_types.push(ty);
        }
        let ret_ty = Type::from_ann(&fn_lit.ret);
        self.return_type_stack.push(ret_ty.clone());

        let body_ty = match &fn_lit.body.kind {
            ExprKind::Block(stmts, tail) => {
                let t = self.check_block_contents(stmts, tail.as_deref(), Some(&ret_ty));
                // Narrow relaxation, not general control-flow
                // reachability analysis (still explicitly out of scope):
                // if the body has no tail expression (structural type
                // `()`) but its last top-level statement is an explicit
                // `return`, the block never actually "falls through" to
                // produce that `()` value — the `return`'s own value
                // already had its type checked against `ret_ty` above by
                // check_stmt. Without this, the common "guard clauses,
                // then a final `return`" idiom would be a false type
                // error despite every runtime path being well-typed.
                // Only the immediate body block's *last* statement is
                // considered — a `return` nested deeper in control flow
                // (e.g. as the only path through an if/else) is not
                // covered by this check and still needs an explicit
                // tail expression.
                if tail.is_none()
                    && matches!(stmts.last().map(|s| &s.kind), Some(StmtKind::Return(_)))
                {
                    ret_ty.clone()
                } else {
                    t
                }
            }
            // Parser invariant: a fn_lit's body is always parsed via
            // parse_block, so it is always ExprKind::Block.
            _ => unreachable!("parser guarantees a fn_lit body is always ExprKind::Block"),
        };
        if !types_compatible(&body_ty, &ret_ty) {
            self.error(
                format!(
                    "function body has type {} but declared return type is {}",
                    body_ty, ret_ty
                ),
                fn_lit.body.span,
            );
        }

        self.return_type_stack.pop();
        self.env.pop_scope();
        Type::Function {
            params: param_types,
            ret: Box::new(ret_ty),
        }
    }

    /// Checks a block's statements and tail expression *without* pushing
    /// its own scope — callers push (function bodies share the params'
    /// scope; every other block gets its own fresh child scope, see
    /// `check_expr`'s `Block` arm). Implements §6.1's two-pass hoisting:
    /// pass 1 registers every sibling `fn_decl`'s signature before pass 2
    /// checks any statement, so mutually recursive siblings can see each
    /// other regardless of textual order.
    fn check_block_contents(
        &mut self,
        stmts: &[Stmt],
        tail: Option<&Expr>,
        expected: Option<&Type>,
    ) -> Type {
        for stmt in stmts {
            if let StmtKind::FnDecl { name, fn_lit } = &stmt.kind {
                let ty = signature_type(fn_lit);
                self.env.declare(name.clone(), ty, false);
            }
        }
        for stmt in stmts {
            self.check_stmt(stmt);
        }
        match tail {
            Some(e) => self.check_expr(e, expected),
            None => Type::Unit,
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let {
                name,
                mutable,
                ty,
                value,
            } => {
                let expected_ty = ty.as_ref().map(Type::from_ann);
                let value_ty = self.check_expr(value, expected_ty.as_ref());
                let final_ty = match &expected_ty {
                    Some(t) => {
                        if !types_compatible(&value_ty, t) {
                            self.error(
                                format!(
                                    "let binding for '{}' declares type {} but initializer has type {}",
                                    name, t, value_ty
                                ),
                                value.span,
                            );
                        }
                        t.clone()
                    }
                    None => value_ty,
                };
                self.env.declare(name.clone(), final_ty, *mutable);
            }
            StmtKind::While { cond, body } => {
                let cond_ty = self.check_expr(cond, None);
                if !types_compatible(&cond_ty, &Type::Bool) {
                    self.error(
                        format!("while condition must be type bool, found {}", cond_ty),
                        cond.span,
                    );
                }
                self.check_expr(body, None);
            }
            StmtKind::Return(value) => {
                let ret_ty = self
                    .return_type_stack
                    .last()
                    .cloned()
                    // Parser/grammar invariant: `return` only ever
                    // appears inside a block, and every block a program
                    // can contain is ultimately a fn_lit body, which
                    // always pushes a return type before checking its
                    // contents.
                    .expect("return statement checked outside any function body");
                match value {
                    Some(e) => {
                        let value_ty = self.check_expr(e, Some(&ret_ty));
                        if !types_compatible(&value_ty, &ret_ty) {
                            self.error(
                                format!(
                                    "return value has type {} but function returns {}",
                                    value_ty, ret_ty
                                ),
                                e.span,
                            );
                        }
                    }
                    None => {
                        if !types_compatible(&Type::Unit, &ret_ty) {
                            self.error(
                                format!(
                                    "`return;` with no value requires the function to return (), but it returns {}",
                                    ret_ty
                                ),
                                stmt.span,
                            );
                        }
                    }
                }
            }
            StmtKind::FnDecl { fn_lit, .. } => {
                // Signature already registered by check_block_contents'
                // pass 1 (or check_program's top-level pass); just check
                // the body now.
                self.check_fn_lit(fn_lit);
            }
            StmtKind::Expr(e) => {
                self.check_expr(e, None);
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr, expected: Option<&Type>) -> Type {
        match &expr.kind {
            ExprKind::IntLit(_) => Type::Int,
            ExprKind::FloatLit(_) => Type::Float,
            ExprKind::BoolLit(_) => Type::Bool,
            ExprKind::StringLit(_) => Type::String,
            ExprKind::ArrayLit(elems) => self.check_array_lit(elems, expected, expr.span),
            ExprKind::Ident(name) => match self.env.lookup(name) {
                Some(info) => info.ty.clone(),
                None => self.error(format!("undeclared variable '{}'", name), expr.span),
            },
            ExprKind::Unary { op, expr: inner } => self.check_unary(*op, inner),
            ExprKind::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, expr.span),
            ExprKind::Assign { target, value } => self.check_assign(target, value),
            ExprKind::Call { callee, args } => self.check_call(callee, args, expr.span),
            ExprKind::Index { array, index } => self.check_index(array, index),
            ExprKind::MethodCall {
                receiver,
                method,
                args,
                span,
            } => self.check_method_call(receiver, method, args, *span),
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.check_if(cond, then_branch, else_branch.as_deref(), expected),
            ExprKind::Block(stmts, tail) => {
                self.env.push_scope();
                let t = self.check_block_contents(stmts, tail.as_deref(), expected);
                self.env.pop_scope();
                t
            }
            ExprKind::FnLit(fn_lit) => self.check_fn_lit(fn_lit),
        }
    }

    fn check_array_lit(&mut self, elems: &[Expr], expected: Option<&Type>, span: Span) -> Type {
        if elems.is_empty() {
            return match expected {
                Some(Type::Array(elem_ty)) => Type::Array(elem_ty.clone()),
                _ => self.error(
                    "cannot infer the type of an empty array literal; add a type annotation \
                     (e.g. `let xs: [int] = []`)"
                        .to_string(),
                    span,
                ),
            };
        }
        let elem_expected = match expected {
            Some(Type::Array(t)) => Some(t.as_ref()),
            _ => None,
        };
        let first_ty = self.check_expr(&elems[0], elem_expected);
        for e in &elems[1..] {
            let t = self.check_expr(e, Some(&first_ty));
            if !types_compatible(&t, &first_ty) {
                self.error(
                    format!(
                        "array elements must all have the same type: expected {}, found {}",
                        first_ty, t
                    ),
                    e.span,
                );
            }
        }
        Type::Array(Box::new(first_ty))
    }

    fn check_unary(&mut self, op: UnOp, inner: &Expr) -> Type {
        let t = self.check_expr(inner, None);
        match op {
            UnOp::Neg => match t {
                Type::Int => Type::Int,
                Type::Float => Type::Float,
                Type::Error => Type::Error,
                other => self.error(
                    format!("unary '-' requires int or float, found {}", other),
                    inner.span,
                ),
            },
            UnOp::Not => match t {
                Type::Bool => Type::Bool,
                Type::Error => Type::Error,
                other => self.error(
                    format!("unary '!' requires bool, found {}", other),
                    inner.span,
                ),
            },
        }
    }

    fn check_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> Type {
        let lt = self.check_expr(lhs, None);
        let rt = self.check_expr(rhs, None);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if lt == Type::Error || rt == Type::Error {
                    Type::Error
                } else if lt == Type::Int && rt == Type::Int {
                    Type::Int
                } else if lt == Type::Float && rt == Type::Float {
                    Type::Float
                } else {
                    self.error(
                        format!(
                            "operator '{}' requires two operands of the same numeric type \
                             (both int or both float), found {} and {}",
                            bin_op_symbol(op),
                            lt,
                            rt
                        ),
                        span,
                    )
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if lt == Type::Error || rt == Type::Error {
                    Type::Error
                } else if (lt == Type::Int && rt == Type::Int)
                    || (lt == Type::Float && rt == Type::Float)
                {
                    Type::Bool
                } else {
                    self.error(
                        format!(
                            "operator '{}' requires two operands of the same numeric type, found {} and {}",
                            bin_op_symbol(op),
                            lt,
                            rt
                        ),
                        span,
                    )
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if lt == Type::Error || rt == Type::Error {
                    Type::Error
                } else if lt != rt {
                    self.error(
                        format!(
                            "cannot compare values of different types: {} and {}",
                            lt, rt
                        ),
                        span,
                    )
                } else if is_primitive(&lt) {
                    Type::Bool
                } else {
                    self.error(
                        format!("type {} does not support equality comparison", lt),
                        span,
                    )
                }
            }
            BinOp::And | BinOp::Or => {
                if lt == Type::Error || rt == Type::Error {
                    Type::Error
                } else if lt == Type::Bool && rt == Type::Bool {
                    Type::Bool
                } else {
                    self.error(
                        format!(
                            "operator '{}' requires two bool operands, found {} and {}",
                            bin_op_symbol(op),
                            lt,
                            rt
                        ),
                        span,
                    )
                }
            }
        }
    }

    fn check_assign(&mut self, target: &Expr, value: &Expr) -> Type {
        match &target.kind {
            ExprKind::Ident(name) => match self.env.lookup(name).cloned() {
                Some(info) => {
                    let value_ty = self.check_expr(value, Some(&info.ty));
                    if !info.mutable {
                        self.error(
                            format!(
                                "cannot assign to immutable variable '{}' (declare it `let mut {}` to allow mutation)",
                                name, name
                            ),
                            target.span,
                        );
                    }
                    if !types_compatible(&value_ty, &info.ty) {
                        self.error(
                            format!(
                                "cannot assign value of type {} to variable '{}' of type {}",
                                value_ty, name, info.ty
                            ),
                            value.span,
                        );
                    }
                    info.ty
                }
                None => {
                    self.check_expr(value, None);
                    self.error(format!("undeclared variable '{}'", name), target.span)
                }
            },
            ExprKind::Index { array, index } => {
                let arr_ty = self.check_expr(array, None);
                let idx_ty = self.check_expr(index, None);
                if !types_compatible(&idx_ty, &Type::Int) {
                    self.error(
                        format!("array index must be of type int, found {}", idx_ty),
                        index.span,
                    );
                }
                let elem_ty = match arr_ty {
                    Type::Array(t) => *t,
                    Type::Error => Type::Error,
                    other => self.error(
                        format!(
                            "cannot index into a value of type {} (expected an array)",
                            other
                        ),
                        array.span,
                    ),
                };
                if !self.is_mut_root(array) {
                    self.error(
                        "cannot assign into an array element unless the array is bound `mut`"
                            .to_string(),
                        array.span,
                    );
                }
                let value_ty = self.check_expr(value, Some(&elem_ty));
                if !types_compatible(&value_ty, &elem_ty) {
                    self.error(
                        format!(
                            "cannot assign value of type {} to array element of type {}",
                            value_ty, elem_ty
                        ),
                        value.span,
                    );
                }
                elem_ty
            }
            // Parser invariant: `assign_expr` structurally restricts
            // assignment targets to Ident or Index (see the doc comment
            // on ExprKind::Assign in ast/mod.rs) — no other target shape
            // can reach the type checker.
            _ => unreachable!("parser guarantees assignment targets are Ident or Index"),
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], call_span: Span) -> Type {
        if let ExprKind::Ident(name) = &callee.kind
            && name == "to_string"
        {
            return self.check_to_string_intrinsic(args, call_span);
        }
        let callee_ty = self.check_expr(callee, None);
        match callee_ty {
            Type::Function { params, ret } => {
                if args.len() != params.len() {
                    self.error(
                        format!(
                            "expected {} argument(s), found {}",
                            params.len(),
                            args.len()
                        ),
                        call_span,
                    );
                }
                for (arg, expected_ty) in args.iter().zip(params.iter()) {
                    let at = self.check_expr(arg, Some(expected_ty));
                    if !types_compatible(&at, expected_ty) {
                        self.error(
                            format!("argument expected type {}, found {}", expected_ty, at),
                            arg.span,
                        );
                    }
                }
                for arg in args.iter().skip(params.len()) {
                    self.check_expr(arg, None);
                }
                *ret
            }
            Type::Error => {
                for arg in args {
                    self.check_expr(arg, None);
                }
                Type::Error
            }
            other => {
                for arg in args {
                    self.check_expr(arg, None);
                }
                self.error(
                    format!(
                        "cannot call a value of type {} (expected a function)",
                        other
                    ),
                    callee.span,
                )
            }
        }
    }

    fn check_to_string_intrinsic(&mut self, args: &[Expr], call_span: Span) -> Type {
        if args.len() != 1 {
            for a in args {
                self.check_expr(a, None);
            }
            return self.error(
                format!("to_string expects 1 argument, found {}", args.len()),
                call_span,
            );
        }
        let t = self.check_expr(&args[0], None);
        match t {
            Type::Int | Type::Float | Type::Bool => Type::String,
            Type::Error => Type::Error,
            other => self.error(
                format!(
                    "to_string expects an int, float, or bool argument, found {}",
                    other
                ),
                args[0].span,
            ),
        }
    }

    fn check_index(&mut self, array: &Expr, index: &Expr) -> Type {
        let at = self.check_expr(array, None);
        let it = self.check_expr(index, None);
        if !types_compatible(&it, &Type::Int) {
            self.error(
                format!("array index must be of type int, found {}", it),
                index.span,
            );
        }
        match at {
            Type::Array(t) => *t,
            Type::Error => Type::Error,
            other => self.error(
                format!(
                    "cannot index into a value of type {} (expected an array)",
                    other
                ),
                array.span,
            ),
        }
    }

    fn check_method_call(
        &mut self,
        receiver: &Expr,
        method: &str,
        args: &[Expr],
        span: Span,
    ) -> Type {
        let rt = self.check_expr(receiver, None);
        match rt {
            Type::Array(elem_ty_box) => {
                let elem_ty = *elem_ty_box;
                match method {
                    "length" => {
                        if !args.is_empty() {
                            for a in args {
                                self.check_expr(a, None);
                            }
                            self.error(
                                format!("'length' takes no arguments, found {}", args.len()),
                                span,
                            );
                        }
                        Type::Int
                    }
                    "push" => {
                        if args.len() != 1 {
                            for a in args {
                                self.check_expr(a, None);
                            }
                            self.error(
                                format!("'push' expects 1 argument, found {}", args.len()),
                                span,
                            );
                        } else {
                            let at = self.check_expr(&args[0], Some(&elem_ty));
                            if !types_compatible(&at, &elem_ty) {
                                self.error(
                                    format!(
                                        "cannot push a value of type {} onto an array of type {}",
                                        at, elem_ty
                                    ),
                                    args[0].span,
                                );
                            }
                        }
                        if !self.is_mut_root(receiver) {
                            self.error(
                                "array must be a `mut` binding (or an index into one) to call `.push()` on it"
                                    .to_string(),
                                receiver.span,
                            );
                        }
                        Type::Unit
                    }
                    other => self.error(format!("unknown array method '{}'", other), span),
                }
            }
            Type::Error => Type::Error,
            other => {
                for a in args {
                    self.check_expr(a, None);
                }
                self.error(
                    format!(
                        "method calls are only supported on arrays, found type {}",
                        other
                    ),
                    receiver.span,
                )
            }
        }
    }

    fn check_if(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        expected: Option<&Type>,
    ) -> Type {
        let ct = self.check_expr(cond, None);
        if !types_compatible(&ct, &Type::Bool) {
            self.error(
                format!("if condition must be type bool, found {}", ct),
                cond.span,
            );
        }
        let tt = self.check_expr(then_branch, expected);
        match else_branch {
            Some(else_e) => {
                let et = self.check_expr(else_e, Some(&tt));
                if !types_compatible(&tt, &et) {
                    self.error(
                        format!(
                            "if/else branches have different types: then is {}, else is {}",
                            tt, et
                        ),
                        else_e.span,
                    );
                    Type::Error
                } else if tt == Type::Error {
                    et
                } else {
                    tt
                }
            }
            None => Type::Unit,
        }
    }
}
