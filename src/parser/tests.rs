use super::*;
use crate::lexer::Lexer;

fn tokens(src: &str) -> Vec<Token> {
    Lexer::new(src)
        .tokenize()
        .expect("lexer should succeed on valid test input")
}

/// Parse a single expression in isolation (no surrounding program), for
/// tests that only care about expression-grammar shape (literals,
/// precedence, arrays, calls, assignment). Asserts the whole input was
/// consumed, so trailing garbage after the expression is caught too.
fn parse_expr_only(src: &str) -> Expr {
    let mut p = Parser::new(tokens(src));
    let e = p.parse_expr().expect("expected expression to parse");
    assert!(
        p.is_at_end(),
        "trailing tokens after expression: {:?}",
        p.peek()
    );
    e
}

fn parse_program_ok(src: &str) -> Program {
    parse(tokens(src)).unwrap_or_else(|e| panic!("expected program to parse, got: {}", e))
}

fn parse_program_err(src: &str) -> ParseError {
    parse(tokens(src)).expect_err("expected a parse error")
}

fn main_body(prog: &Program) -> &Vec<Stmt> {
    let Item::FnDecl { fn_lit, .. } = prog
        .items
        .iter()
        .find(|i| matches!(i, Item::FnDecl { name, .. } if name == "main"))
        .expect("expected a `main` function");
    match &fn_lit.body.kind {
        ExprKind::Block(stmts, _) => stmts,
        _ => panic!("function body is not a block"),
    }
}

// ---------------------------------------------------------------------
// Literals
// ---------------------------------------------------------------------

#[test]
fn int_literal() {
    assert_eq!(parse_expr_only("42").kind, ExprKind::IntLit(42));
}

#[test]
fn float_literal() {
    assert_eq!(parse_expr_only("3.5").kind, ExprKind::FloatLit(3.5));
}

#[test]
fn string_literal() {
    assert_eq!(
        parse_expr_only(r#""hello""#).kind,
        ExprKind::StringLit("hello".to_string())
    );
}

#[test]
fn bool_literals() {
    assert_eq!(parse_expr_only("true").kind, ExprKind::BoolLit(true));
    assert_eq!(parse_expr_only("false").kind, ExprKind::BoolLit(false));
}

#[test]
fn identifier() {
    assert_eq!(
        parse_expr_only("some_var").kind,
        ExprKind::Ident("some_var".to_string())
    );
}

// ---------------------------------------------------------------------
// Operator precedence — AST shape, not just "it parsed"
// ---------------------------------------------------------------------

fn int(n: i64) -> Box<Expr> {
    Box::new(Expr {
        kind: ExprKind::IntLit(n),
        span: Span { line: 0, col: 0 },
    })
}

fn bin(op: BinOp, lhs: Expr, rhs: Expr) -> ExprKind {
    ExprKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn strip_spans(e: &Expr) -> Expr {
    // Compare AST shape ignoring spans (spans are checked separately).
    let kind = match &e.kind {
        ExprKind::IntLit(n) => ExprKind::IntLit(*n),
        ExprKind::FloatLit(x) => ExprKind::FloatLit(*x),
        ExprKind::BoolLit(b) => ExprKind::BoolLit(*b),
        ExprKind::StringLit(s) => ExprKind::StringLit(s.clone()),
        ExprKind::Ident(s) => ExprKind::Ident(s.clone()),
        ExprKind::ArrayLit(elems) => ExprKind::ArrayLit(elems.iter().map(strip_spans).collect()),
        ExprKind::Unary { op, expr } => ExprKind::Unary {
            op: *op,
            expr: Box::new(strip_spans(expr)),
        },
        ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
            op: *op,
            lhs: Box::new(strip_spans(lhs)),
            rhs: Box::new(strip_spans(rhs)),
        },
        ExprKind::Assign { target, value } => ExprKind::Assign {
            target: Box::new(strip_spans(target)),
            value: Box::new(strip_spans(value)),
        },
        ExprKind::Call { callee, args } => ExprKind::Call {
            callee: Box::new(strip_spans(callee)),
            args: args.iter().map(strip_spans).collect(),
        },
        ExprKind::Index { array, index } => ExprKind::Index {
            array: Box::new(strip_spans(array)),
            index: Box::new(strip_spans(index)),
        },
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => ExprKind::MethodCall {
            receiver: Box::new(strip_spans(receiver)),
            method: method.clone(),
            args: args.iter().map(strip_spans).collect(),
            span: Span { line: 0, col: 0 },
        },
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => ExprKind::If {
            cond: Box::new(strip_spans(cond)),
            then_branch: Box::new(strip_spans(then_branch)),
            else_branch: else_branch.as_ref().map(|e| Box::new(strip_spans(e))),
        },
        ExprKind::Block(_, tail) => {
            ExprKind::Block(Vec::new(), tail.as_ref().map(|e| Box::new(strip_spans(e))))
        }
        ExprKind::FnLit(f) => ExprKind::FnLit(f.clone()),
    };
    Expr {
        kind,
        span: Span { line: 0, col: 0 },
    }
}

fn assert_expr_shape(src: &str, expected_kind: ExprKind) {
    let expected = Expr {
        kind: expected_kind,
        span: Span { line: 0, col: 0 },
    };
    let actual = strip_spans(&parse_expr_only(src));
    assert_eq!(actual, expected, "unexpected AST shape for `{}`", src);
}

#[test]
fn precedence_mul_binds_tighter_than_add() {
    // 1 + 2 * 3  ==  1 + (2 * 3), not (1 + 2) * 3
    assert_expr_shape(
        "1 + 2 * 3",
        bin(
            BinOp::Add,
            *int(1),
            Expr {
                kind: bin(BinOp::Mul, *int(2), *int(3)),
                span: Span { line: 0, col: 0 },
            },
        ),
    );
}

#[test]
fn parens_override_precedence() {
    // (1 + 2) * 3
    assert_expr_shape(
        "(1 + 2) * 3",
        bin(
            BinOp::Mul,
            Expr {
                kind: bin(BinOp::Add, *int(1), *int(2)),
                span: Span { line: 0, col: 0 },
            },
            *int(3),
        ),
    );
}

#[test]
fn add_is_left_associative() {
    // 1 - 2 - 3 == (1 - 2) - 3
    assert_expr_shape(
        "1 - 2 - 3",
        bin(
            BinOp::Sub,
            Expr {
                kind: bin(BinOp::Sub, *int(1), *int(2)),
                span: Span { line: 0, col: 0 },
            },
            *int(3),
        ),
    );
}

#[test]
fn relational_binds_tighter_than_equality() {
    // 1 < 2 == true  ==  (1 < 2) == true
    let expected = bin(
        BinOp::Eq,
        Expr {
            kind: bin(BinOp::Lt, *int(1), *int(2)),
            span: Span { line: 0, col: 0 },
        },
        Expr {
            kind: ExprKind::BoolLit(true),
            span: Span { line: 0, col: 0 },
        },
    );
    assert_expr_shape("1 < 2 == true", expected);
}

#[test]
fn and_binds_tighter_than_or() {
    // a && b || c  ==  (a && b) || c
    let ident = |s: &str| Expr {
        kind: ExprKind::Ident(s.to_string()),
        span: Span { line: 0, col: 0 },
    };
    let expected = bin(
        BinOp::Or,
        Expr {
            kind: bin(BinOp::And, ident("a"), ident("b")),
            span: Span { line: 0, col: 0 },
        },
        ident("c"),
    );
    assert_expr_shape("a && b || c", expected);
}

#[test]
fn unary_minus_binds_tighter_than_add() {
    // -1 + 2 == (-1) + 2
    let neg_one = Expr {
        kind: ExprKind::Unary {
            op: UnOp::Neg,
            expr: int(1),
        },
        span: Span { line: 0, col: 0 },
    };
    assert_expr_shape("-1 + 2", bin(BinOp::Add, neg_one, *int(2)));
}

#[test]
fn unary_is_right_recursive() {
    // --1 == -(-1) : two separate Minus tokens, nested Unary nodes.
    let inner = Expr {
        kind: ExprKind::Unary {
            op: UnOp::Neg,
            expr: int(1),
        },
        span: Span { line: 0, col: 0 },
    };
    assert_expr_shape(
        "--1",
        ExprKind::Unary {
            op: UnOp::Neg,
            expr: Box::new(inner),
        },
    );
}

#[test]
fn not_unary() {
    let ident_a = Expr {
        kind: ExprKind::Ident("a".to_string()),
        span: Span { line: 0, col: 0 },
    };
    assert_expr_shape(
        "!a",
        ExprKind::Unary {
            op: UnOp::Not,
            expr: Box::new(ident_a),
        },
    );
}

// ---------------------------------------------------------------------
// Arrays, indexing, calls, method calls
// ---------------------------------------------------------------------

#[test]
fn array_literal() {
    assert_expr_shape(
        "[1, 2, 3]",
        ExprKind::ArrayLit(vec![
            Expr {
                kind: ExprKind::IntLit(1),
                span: Span { line: 0, col: 0 },
            },
            Expr {
                kind: ExprKind::IntLit(2),
                span: Span { line: 0, col: 0 },
            },
            Expr {
                kind: ExprKind::IntLit(3),
                span: Span { line: 0, col: 0 },
            },
        ]),
    );
}

#[test]
fn empty_array_literal() {
    assert_expr_shape("[]", ExprKind::ArrayLit(vec![]));
}

#[test]
fn indexing() {
    let expr = parse_expr_only("xs[0]");
    match expr.kind {
        ExprKind::Index { array, index } => {
            assert_eq!(array.kind, ExprKind::Ident("xs".to_string()));
            assert_eq!(index.kind, ExprKind::IntLit(0));
        }
        other => panic!("expected Index, got {:?}", other),
    }
}

#[test]
fn function_call() {
    let expr = parse_expr_only("f(1, 2)");
    match expr.kind {
        ExprKind::Call { callee, args } => {
            assert_eq!(callee.kind, ExprKind::Ident("f".to_string()));
            assert_eq!(args.len(), 2);
            assert_eq!(args[0].kind, ExprKind::IntLit(1));
            assert_eq!(args[1].kind, ExprKind::IntLit(2));
        }
        other => panic!("expected Call, got {:?}", other),
    }
}

#[test]
fn method_call_no_args() {
    let expr = parse_expr_only("xs.length()");
    match expr.kind {
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            assert_eq!(receiver.kind, ExprKind::Ident("xs".to_string()));
            assert_eq!(method, "length");
            assert!(args.is_empty());
        }
        other => panic!("expected MethodCall, got {:?}", other),
    }
}

#[test]
fn method_call_with_args() {
    let expr = parse_expr_only("xs.push(4)");
    match expr.kind {
        ExprKind::MethodCall { method, args, .. } => {
            assert_eq!(method, "push");
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].kind, ExprKind::IntLit(4));
        }
        other => panic!("expected MethodCall, got {:?}", other),
    }
}

#[test]
fn chained_postfix_suffixes() {
    // f(x)[0].push(1) -- call, then index, then method call, in order.
    let expr = parse_expr_only("f(x)[0].push(1)");
    match expr.kind {
        ExprKind::MethodCall {
            receiver, method, ..
        } => {
            assert_eq!(method, "push");
            match receiver.kind {
                ExprKind::Index { array, .. } => match array.kind {
                    ExprKind::Call { .. } => {}
                    other => panic!("expected Call as innermost, got {:?}", other),
                },
                other => panic!("expected Index, got {:?}", other),
            }
        }
        other => panic!("expected MethodCall, got {:?}", other),
    }
}

// ---------------------------------------------------------------------
// Assignment
// ---------------------------------------------------------------------

#[test]
fn assign_to_ident() {
    let expr = parse_expr_only("x = 5");
    match expr.kind {
        ExprKind::Assign { target, value } => {
            assert_eq!(target.kind, ExprKind::Ident("x".to_string()));
            assert_eq!(value.kind, ExprKind::IntLit(5));
        }
        other => panic!("expected Assign, got {:?}", other),
    }
}

#[test]
fn assign_to_index() {
    let expr = parse_expr_only("xs[0] = 5");
    match expr.kind {
        ExprKind::Assign { target, .. } => {
            assert!(matches!(target.kind, ExprKind::Index { .. }));
        }
        other => panic!("expected Assign, got {:?}", other),
    }
}

#[test]
fn assign_is_right_associative() {
    // x = y = 5  ==  x = (y = 5)
    let expr = parse_expr_only("x = y = 5");
    match expr.kind {
        ExprKind::Assign { target, value } => {
            assert_eq!(target.kind, ExprKind::Ident("x".to_string()));
            match value.kind {
                ExprKind::Assign { target, value } => {
                    assert_eq!(target.kind, ExprKind::Ident("y".to_string()));
                    assert_eq!(value.kind, ExprKind::IntLit(5));
                }
                other => panic!("expected nested Assign, got {:?}", other),
            }
        }
        other => panic!("expected Assign, got {:?}", other),
    }
}

#[test]
fn assign_to_non_place_is_parser_rejected() {
    let err = parse_program_err("fn main() -> () { 5 = x; }");
    assert!(
        err.message.contains("invalid assignment target"),
        "got: {}",
        err.message
    );
}

// ---------------------------------------------------------------------
// if/else — value position and statement position
// ---------------------------------------------------------------------

#[test]
fn if_else_as_value() {
    let expr = parse_expr_only("if a { 1 } else { 2 }");
    match expr.kind {
        ExprKind::If {
            cond, else_branch, ..
        } => {
            assert_eq!(cond.kind, ExprKind::Ident("a".to_string()));
            assert!(else_branch.is_some());
        }
        other => panic!("expected If, got {:?}", other),
    }
}

#[test]
fn if_expr_missing_else_in_value_position_is_parse_error() {
    let err = parse_program_err("fn main() -> () { let x = if true { 1 }; }");
    assert!(
        err.message.to_lowercase().contains("else"),
        "got: {}",
        err.message
    );
}

#[test]
fn bare_if_statement_no_else_allowed() {
    let prog = parse_program_ok("fn main() -> () { if true { 1 } }");
    let stmts = main_body(&prog);
    assert_eq!(stmts.len(), 1);
    match &stmts[0].kind {
        StmtKind::Expr(e) => match &e.kind {
            ExprKind::If { else_branch, .. } => assert!(else_branch.is_none()),
            other => panic!("expected If, got {:?}", other),
        },
        other => panic!("expected Stmt::Expr, got {:?}", other),
    }
}

#[test]
fn bare_if_with_optional_trailing_semicolon() {
    parse_program_ok("fn main() -> () { if true { 1 }; }");
}

#[test]
fn while_with_and_without_trailing_semicolon() {
    parse_program_ok("fn main() -> () { while true { 1; } }");
    parse_program_ok("fn main() -> () { while true { 1; }; }");
}

#[test]
fn else_if_chain_requires_final_else() {
    // Strict literal grammar reading (confirmed): missing final `else`
    // in an else-if chain is a ParseError even in throwaway/statement
    // position, not just value position.
    let err = parse_program_err("fn main() -> () { if a { 1 } else if b { 2 } }");
    assert!(
        err.message.to_lowercase().contains("else"),
        "got: {}",
        err.message
    );
}

#[test]
fn else_if_chain_with_final_else_parses() {
    parse_program_ok("fn main() -> () { if a { 1 } else if b { 2 } else { 3 }; }");
}

// ---------------------------------------------------------------------
// The statement-position restriction — the single most important
// correctness property from this session's spec update.
// ---------------------------------------------------------------------

#[test]
fn if_else_statement_does_not_combine_with_trailing_operator() {
    let err = parse_program_err("fn main() -> () { if c { 1 } else { 2 } + 1; }");
    assert!(
        err.message.contains("expected ';'"),
        "expected a dangling '+' to be a separate, malformed statement, got: {}",
        err.message
    );
}

#[test]
fn parenthesized_if_else_does_combine_with_trailing_operator() {
    let prog = parse_program_ok("fn main() -> () { let x = (if c { 1 } else { 2 }) + 1; }");
    let stmts = main_body(&prog);
    match &stmts[0].kind {
        StmtKind::Let { value, .. } => match &value.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                assert_eq!(*op, BinOp::Add);
                assert!(matches!(lhs.kind, ExprKind::If { .. }));
                assert_eq!(rhs.kind, ExprKind::IntLit(1));
            }
            other => panic!("expected Binary(Add, If, 1), got {:?}", other),
        },
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn while_statement_does_not_combine_with_trailing_operator() {
    // `while` is never reachable via `primary`, so `+ 1` can never be
    // parsed as a continuation of it. Concretely: `parse_while_stmt`
    // completes and returns without ever looking at `+`; the block loop
    // then tries to start a *new* statement there, which fails because
    // Ember has no unary `+` — a different message than the if/else
    // case (which has its own explicit "expected ';'" check), but proof
    // of the same underlying property: the while statement was never
    // combined with the trailing `+ 1` into one expression.
    let err = parse_program_err("fn main() -> () { while c { 1; } + 1; }");
    assert!(
        err.message.contains("expected an expression"),
        "got: {}",
        err.message
    );
}

// ---------------------------------------------------------------------
// fn declarations, fn literals, closures, params, types
// ---------------------------------------------------------------------

#[test]
fn top_level_fn_decl() {
    let prog = parse_program_ok("fn add(x: int, y: int) -> int { x + y }");
    match &prog.items[0] {
        Item::FnDecl { name, fn_lit, .. } => {
            assert_eq!(name, "add");
            assert_eq!(fn_lit.params.len(), 2);
            assert_eq!(fn_lit.params[0].name, "x");
            assert!(!fn_lit.params[0].mutable);
            assert_eq!(fn_lit.ret, TypeAnn::Int);
        }
    }
}

#[test]
fn mut_param() {
    let prog = parse_program_ok("fn f(mut x: int) -> int { x }");
    match &prog.items[0] {
        Item::FnDecl { fn_lit, .. } => assert!(fn_lit.params[0].mutable),
    }
}

#[test]
fn anonymous_fn_lit_as_tail_expression() {
    // The closure-factory case: a nested, unnamed `fn(...)->T{...}` used
    // as a block's tail expression must NOT be mistaken for a named
    // fn_decl_stmt (see the lookahead fix this uncovered).
    let prog = parse_program_ok(
        "fn make_counter(start: int) -> fn() -> int {
            let mut count = start;
            fn() -> int {
                count = count + 1;
                count
            }
        }",
    );
    let fn_lit = match &prog.items[0] {
        Item::FnDecl { fn_lit, .. } => fn_lit,
    };
    assert_eq!(
        fn_lit.ret,
        TypeAnn::Function {
            params: vec![],
            ret: Box::new(TypeAnn::Int)
        }
    );
    match &fn_lit.body.kind {
        ExprKind::Block(stmts, tail) => {
            assert_eq!(stmts.len(), 1); // just the `let`
            assert!(matches!(
                tail.as_ref().map(|e| &e.kind),
                Some(ExprKind::FnLit(_))
            ));
        }
        other => panic!("expected Block, got {:?}", other),
    }
}

#[test]
fn nested_named_fn_decls_parse_regardless_of_call_order() {
    // Mutual recursion (EXAMPLES_DRAFT.md example 5): is_even calls
    // is_odd, declared after it. The parser doesn't need to resolve
    // this (that's §6.1's interpreter-level hoisting), but it must at
    // least parse both declarations as StmtKind::FnDecl without
    // requiring forward-declaration syntax.
    let prog = parse_program_ok(
        "fn main() -> () {
            fn is_even(n: int) -> bool {
                if n == 0 { true } else { is_odd(n - 1) }
            }
            fn is_odd(n: int) -> bool {
                if n == 0 { false } else { is_even(n - 1) }
            }
            println(to_string(is_even(10)));
        }",
    );
    let stmts = main_body(&prog);
    assert!(matches!(stmts[0].kind, StmtKind::FnDecl { .. }));
    assert!(matches!(stmts[1].kind, StmtKind::FnDecl { .. }));
    if let StmtKind::FnDecl { name, .. } = &stmts[0].kind {
        assert_eq!(name, "is_even");
    }
    if let StmtKind::FnDecl { name, .. } = &stmts[1].kind {
        assert_eq!(name, "is_odd");
    }
}

#[test]
fn array_and_function_types() {
    let prog = parse_program_ok("fn f(xs: [int], g: fn(int, int) -> bool) -> [int] { xs }");
    match &prog.items[0] {
        Item::FnDecl { fn_lit, .. } => {
            assert_eq!(fn_lit.params[0].ty, TypeAnn::Array(Box::new(TypeAnn::Int)));
            assert_eq!(
                fn_lit.params[1].ty,
                TypeAnn::Function {
                    params: vec![TypeAnn::Int, TypeAnn::Int],
                    ret: Box::new(TypeAnn::Bool)
                }
            );
            assert_eq!(fn_lit.ret, TypeAnn::Array(Box::new(TypeAnn::Int)));
        }
    }
}

#[test]
fn let_with_explicit_type_annotation() {
    let prog = parse_program_ok("fn main() -> () { let xs: [int] = []; }");
    let stmts = main_body(&prog);
    match &stmts[0].kind {
        StmtKind::Let { ty, .. } => assert_eq!(*ty, Some(TypeAnn::Array(Box::new(TypeAnn::Int)))),
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn let_mut_binding() {
    let prog = parse_program_ok("fn main() -> () { let mut x = 5; }");
    let stmts = main_body(&prog);
    match &stmts[0].kind {
        StmtKind::Let { mutable, .. } => assert!(mutable),
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn return_with_and_without_value() {
    parse_program_ok("fn f() -> int { return 5; }");
    parse_program_ok("fn f() -> () { return; }");
}

// ---------------------------------------------------------------------
// Malformed input — clean ParseErrors, never a panic
// ---------------------------------------------------------------------

#[test]
fn mismatched_braces_error() {
    let err = parse_program_err("fn main() -> () { let x = 5;");
    assert!(err.message.contains("end of input"));
}

#[test]
fn mismatched_parens_error() {
    let err = parse_program_err("fn f(x: int -> int { x }");
    // missing ')' before '->'
    assert!(err.message.contains("')'") || err.message.contains("','"));
}

#[test]
fn missing_arrow_in_fn_signature_error() {
    let err = parse_program_err("fn f(x: int) { x }");
    assert!(err.message.contains("'->'"), "got: {}", err.message);
}

#[test]
fn if_with_missing_condition_error() {
    // `{` is itself a valid primary (a bare block expression), so
    // `if { 1 } }` parses `{ 1 }` as the *condition* and then correctly
    // complains there's no then-block left to parse — a clean, located
    // ParseError either way, just not literally "expected an expression".
    let err = parse_program_err("fn main() -> () { if { 1 } }");
    assert!(err.message.contains("'{'"), "got: {}", err.message);
}

#[test]
fn if_with_non_expression_condition_error() {
    let err = parse_program_err("fn main() -> () { if , { 1 } else { 2 } }");
    assert!(
        err.message.contains("expected an expression"),
        "got: {}",
        err.message
    );
}

#[test]
fn dangling_operator_no_rhs_error() {
    let err = parse_program_err("fn main() -> () { let x = 1 + ; }");
    assert!(
        err.message.contains("expected an expression"),
        "got: {}",
        err.message
    );
}

#[test]
fn unclosed_array_literal_error() {
    let err = parse_program_err("fn main() -> () { let x = [1, 2; }");
    assert!(err.message.contains("']'"), "got: {}", err.message);
}

#[test]
fn errors_carry_a_real_span_not_a_panic() {
    // A representative sample: none of these should panic, and each
    // must carry a non-degenerate (line, col).
    for src in [
        "fn main() -> (",
        "fn () -> int { 1 }",
        "fn main() -> () { let = 5; }",
        "fn main() -> () { 1 + + 2; }",
    ] {
        let err = parse_program_err(src);
        assert!(err.span.line >= 1 && err.span.col >= 1);
    }
}

// ---------------------------------------------------------------------
// Integration: all 5 EXAMPLES_DRAFT.md programs must parse successfully
// ---------------------------------------------------------------------

#[test]
fn example_1_recursive_fibonacci_parses() {
    parse_program_ok(
        r#"
        fn fib(n: int) -> int {
            if n < 2 {
                n
            } else {
                fib(n - 1) + fib(n - 2)
            }
        }

        fn main() -> () {
            println(to_string(fib(20)));
        }
        "#,
    );
}

#[test]
fn example_2_closure_counter_factory_parses() {
    parse_program_ok(
        r#"
        fn make_counter(start: int) -> fn() -> int {
            let mut count = start;
            fn() -> int {
                count = count + 1;
                count
            }
        }

        fn main() -> () {
            let counter_a = make_counter(0);
            let counter_b = make_counter(100);

            println(to_string(counter_a()));
            println(to_string(counter_a()));
            println(to_string(counter_b()));
            println(to_string(counter_a()));
        }
        "#,
    );
}

#[test]
fn example_3_array_mutability_parses() {
    parse_program_ok(
        r#"
        fn sum(xs: [int]) -> int {
            fn go(xs: [int], i: int, acc: int) -> int {
                if i >= xs.length() {
                    acc
                } else {
                    go(xs, i + 1, acc + xs[i])
                }
            }
            go(xs, 0, 0)
        }

        fn main() -> () {
            let mut nums = [1, 2, 3];
            nums.push(4);
            nums.push(5);

            println(to_string(sum(nums)));
        }
        "#,
    );
}

#[test]
fn example_4_bubble_sort_parses() {
    parse_program_ok(
        r#"
        fn bubble_sort(xs: [int]) -> [int] {
            let mut result = xs;
            let mut i = 0;
            while i < result.length() {
                let mut j = 0;
                while j < result.length() - 1 - i {
                    if result[j] > result[j + 1] {
                        let tmp = result[j];
                        result[j] = result[j + 1];
                        result[j + 1] = tmp;
                    }
                    j = j + 1;
                }
                i = i + 1;
            }
            result
        }

        fn main() -> () {
            let sorted = bubble_sort([5, 3, 8, 1, 9, 2]);
            let mut i = 0;
            while i < sorted.length() {
                println(to_string(sorted[i]));
                i = i + 1;
            }
        }
        "#,
    );
}

#[test]
fn example_5_mutual_recursion_parses() {
    parse_program_ok(
        r#"
        fn main() -> () {
            fn is_even(n: int) -> bool {
                if n == 0 { true } else { is_odd(n - 1) }
            }
            fn is_odd(n: int) -> bool {
                if n == 0 { false } else { is_even(n - 1) }
            }

            println(to_string(is_even(10)));
        }
        "#,
    );
}
