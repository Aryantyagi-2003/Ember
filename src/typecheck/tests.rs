use super::*;
use crate::lexer::Lexer;

fn check_src(src: &str) -> Vec<TypeError> {
    let tokens = Lexer::new(src)
        .tokenize()
        .unwrap_or_else(|e| panic!("expected lex to succeed for:\n{}\ngot: {}", src, e));
    let prog = crate::parser::parse(tokens)
        .unwrap_or_else(|e| panic!("expected parse to succeed for:\n{}\ngot: {}", src, e));
    check_program(&prog)
}

fn assert_ok(src: &str) {
    let errs = check_src(src);
    assert!(
        errs.is_empty(),
        "expected no type errors for:\n{}\ngot: {:#?}",
        src,
        errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );
}

fn assert_err(src: &str) -> Vec<TypeError> {
    let errs = check_src(src);
    assert!(!errs.is_empty(), "expected type error(s) for:\n{}", src);
    errs
}

fn assert_err_contains(src: &str, needle: &str) {
    let errs = assert_err(src);
    assert!(
        errs.iter().any(|e| e.message.contains(needle)),
        "expected an error containing {:?} for:\n{}\ngot: {:#?}",
        needle,
        src,
        errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );
}

// =======================================================================
// Error-catching: the core required cases from the project brief
// =======================================================================

#[test]
fn wrong_argument_count() {
    assert_err_contains(
        "fn f(x: int) -> int { x } fn main() -> () { f(1, 2); }",
        "argument(s)",
    );
}

#[test]
fn wrong_argument_type() {
    assert_err_contains(
        r#"fn f(x: int) -> int { x } fn main() -> () { f("hi"); }"#,
        "expected type int",
    );
}

#[test]
fn use_of_variable_before_declared() {
    assert_err_contains(
        "fn main() -> () { let y = x; let x = 5; }",
        "undeclared variable 'x'",
    );
}

#[test]
fn use_of_variable_after_scope_ends() {
    // A bare nested `{ }` block statement still requires its own `;`
    // (only `while`/bare-`if` got the optional-semicolon exemption —
    // see LANGUAGE_SPEC.md §7).
    assert_err_contains(
        "fn main() -> () { { let x = 5; }; let y = x; }",
        "undeclared variable 'x'",
    );
}

#[test]
fn assignment_of_wrong_type_to_typed_variable() {
    assert_err_contains(
        r#"fn main() -> () { let mut x = 5; x = "hi"; }"#,
        "cannot assign value of type string to variable 'x' of type int",
    );
}

#[test]
fn non_bool_if_condition() {
    assert_err_contains(
        "fn main() -> () { if 1 { } }",
        "if condition must be type bool, found int",
    );
}

#[test]
fn non_bool_while_condition() {
    assert_err_contains(
        "fn main() -> () { while 1 { } }",
        "while condition must be type bool, found int",
    );
}

#[test]
fn indexing_a_non_array_value() {
    assert_err_contains(
        "fn main() -> () { let x = 5; let y = x[0]; }",
        "cannot index into a value of type int",
    );
}

#[test]
fn assignment_to_non_mut_binding() {
    assert_err_contains(
        "fn main() -> () { let x = 5; x = 6; }",
        "cannot assign to immutable variable 'x'",
    );
}

// ---- ownership boundary: parser owns place-shape, checker owns mutability ----

#[test]
fn checker_enforces_mutability_not_place_shape() {
    // The parser already rejects `5 = x;` at parse time (tested in
    // parser::tests::assign_to_non_place_is_parser_rejected) — it never
    // reaches the checker. The checker's job starts one level in: given
    // a syntactically valid place (Ident/Index), is the *binding*
    // mutable? Confirm the checker doesn't redundantly re-derive the
    // place-shape restriction (it only ever sees Ident/Index targets,
    // by construction) and *does* own the mutability check end to end.
    assert_err_contains("fn main() -> () { let x = 5; x = 6; }", "immutable");
    assert_ok("fn main() -> () { let mut x = 5; x = 6; }");
}

#[test]
fn array_index_assignment_to_non_mut_array() {
    assert_err_contains(
        "fn main() -> () { let xs = [1, 2, 3]; xs[0] = 5; }",
        "cannot assign into an array element unless the array is bound `mut`",
    );
}

#[test]
fn array_index_assignment_to_mut_array_is_ok() {
    assert_ok("fn main() -> () { let mut xs = [1, 2, 3]; xs[0] = 5; }");
}

// =======================================================================
// Every stdlib function — happy path AND misuse
// =======================================================================

#[test]
fn print_and_println_accept_string_only() {
    assert_ok(r#"fn main() -> () { print("hi"); println("hi"); }"#);
    assert_err_contains("fn main() -> () { print(5); }", "expected type string");
    assert_err_contains("fn main() -> () { println(5); }", "expected type string");
}

#[test]
fn concat_happy_path_and_misuse() {
    assert_ok(r#"fn main() -> () { let s = concat("a", "b"); }"#);
    assert_err_contains(
        r#"fn main() -> () { let s = concat(1, "b"); }"#,
        "expected type string",
    );
}

#[test]
fn str_length_happy_path_and_misuse() {
    assert_ok(r#"fn main() -> () { let n = str_length("abc"); }"#);
    assert_err_contains(
        "fn main() -> () { let n = str_length(5); }",
        "expected type string",
    );
}

#[test]
fn int_to_float_and_float_to_int_happy_path_and_misuse() {
    assert_ok("fn main() -> () { let a = int_to_float(5); let b = float_to_int(5.5); }");
    assert_err_contains(
        r#"fn main() -> () { let a = int_to_float("x"); }"#,
        "expected type int",
    );
    assert_err_contains(
        "fn main() -> () { let b = float_to_int(5); }",
        "expected type float",
    );
}

#[test]
fn panic_happy_path_and_misuse() {
    assert_ok(r#"fn main() -> () { panic("boom"); }"#);
    assert_err_contains("fn main() -> () { panic(5); }", "expected type string");
}

#[test]
fn panic_return_type_unifies_with_any_branch() {
    // panic's declared return type is `never` — it must not force the
    // other branch of an if/else to also produce `()`.
    assert_ok(r#"fn main() -> () { let x = if true { 5 } else { panic("no") }; }"#);
}

#[test]
fn to_string_dispatches_on_int_float_bool() {
    assert_ok(
        "fn main() -> () { let a = to_string(1); let b = to_string(1.5); let c = to_string(true); }",
    );
}

#[test]
fn to_string_rejects_a_fourth_type_cleanly() {
    let errs = assert_err("fn main() -> () { let xs = [1, 2, 3]; let s = to_string(xs); }");
    assert!(
        errs.iter().any(|e| e
            .message
            .contains("to_string expects an int, float, or bool argument")),
        "got: {:#?}",
        errs
    );
}

#[test]
fn to_string_wrong_argument_count() {
    assert_err_contains(
        "fn main() -> () { let s = to_string(1, 2); }",
        "to_string expects 1 argument, found 2",
    );
}

#[test]
fn array_length_and_push_happy_path() {
    assert_ok("fn main() -> () { let mut xs = [1, 2, 3]; let n = xs.length(); xs.push(4); }");
}

#[test]
fn array_push_on_non_mut_array_is_rejected() {
    assert_err_contains(
        "fn main() -> () { let xs = [1, 2, 3]; xs.push(4); }",
        "array must be a `mut` binding",
    );
}

#[test]
fn array_push_wrong_element_type() {
    assert_err_contains(
        r#"fn main() -> () { let mut xs = [1, 2, 3]; xs.push("x"); }"#,
        "cannot push a value of type string onto an array of type int",
    );
}

#[test]
fn array_indexing_happy_path() {
    assert_ok("fn main() -> () { let xs = [1, 2, 3]; let x = xs[0]; }");
}

// =======================================================================
// Other required error conditions
// =======================================================================

#[test]
fn empty_array_literal_without_annotation_is_rejected() {
    assert_err_contains(
        "fn main() -> () { let xs = []; }",
        "cannot infer the type of an empty array literal",
    );
}

#[test]
fn empty_array_literal_with_annotation_is_ok() {
    assert_ok("fn main() -> () { let xs: [int] = []; }");
}

#[test]
fn if_else_branch_type_mismatch() {
    assert_err_contains(
        r#"fn main() -> () { let x = if true { 1 } else { "a" }; }"#,
        "if/else branches have different types",
    );
}

#[test]
fn equality_is_rejected_on_arrays() {
    assert_err_contains(
        "fn main() -> () { let a = [1]; let b = [2]; let c = a == b; }",
        "does not support equality comparison",
    );
}

#[test]
fn equality_is_ok_on_primitives() {
    assert_ok("fn main() -> () { let a = 1 == 2; let b = \"x\" == \"y\"; let c = true != false; }");
}

#[test]
fn mixing_int_and_float_in_arithmetic_is_rejected() {
    assert_err_contains(
        "fn main() -> () { let x = 1 + 1.0; }",
        "requires two operands of the same numeric type",
    );
}

#[test]
fn calling_a_non_function_value() {
    assert_err_contains(
        "fn main() -> () { let x = 5; x(1); }",
        "cannot call a value of type int (expected a function)",
    );
}

#[test]
fn return_value_type_mismatch() {
    assert_err_contains(
        r#"fn f() -> int { return "x"; }"#,
        "return value has type string but function returns int",
    );
}

#[test]
fn function_body_tail_type_mismatch() {
    assert_err_contains(
        r#"fn f() -> int { "x" }"#,
        "function body has type string but declared return type is int",
    );
}

#[test]
fn bare_return_with_non_unit_function_is_rejected() {
    assert_err_contains(
        "fn f() -> int { return; }",
        "`return;` with no value requires the function to return ()",
    );
}

#[test]
fn bare_return_with_unit_function_is_ok() {
    assert_ok("fn f() -> () { return; }");
}

#[test]
fn errors_carry_real_spans() {
    let errs = assert_err("fn main() -> () { let x = 5; x = 6; }");
    // `x = 6;` is on line 1; just confirm we got a real, non-degenerate
    // location, not a zeroed-out placeholder.
    assert!(errs.iter().all(|e| e.span.line >= 1 && e.span.col >= 1));
}

// =======================================================================
// Sibling function hoisting (§6.1) — the whole reason this stage's
// design work started
// =======================================================================

#[test]
fn sibling_function_hoisting_allows_forward_reference_in_a_block() {
    // A (declared first) calls B (declared after it, same block) — must
    // type-check via the two-pass hoisting in check_block_contents.
    assert_ok(
        "
        fn main() -> () {
            fn a(n: int) -> bool { if n == 0 { true } else { b(n - 1) } }
            fn b(n: int) -> bool { if n == 0 { false } else { a(n - 1) } }
            let r = a(4);
        }
        ",
    );
}

#[test]
fn example_5_mutual_recursion_type_checks() {
    assert_ok(
        "
        fn main() -> () {
            fn is_even(n: int) -> bool {
                if n == 0 { true } else { is_odd(n - 1) }
            }
            fn is_odd(n: int) -> bool {
                if n == 0 { false } else { is_even(n - 1) }
            }
            println(to_string(is_even(10)));
        }
        ",
    );
}

#[test]
fn top_level_function_hoisting_allows_forward_reference() {
    // Extension of §6.1 to the top level: `main` calls `helper`, which
    // is declared textually *after* `main`.
    assert_ok(
        "
        fn main() -> () { let r = helper(5); }
        fn helper(n: int) -> int { n + 1 }
        ",
    );
}

// =======================================================================
// The "doesn't reject valid programs" bar — 15+ genuinely valid programs
// =======================================================================

#[test]
fn valid_01_example_1_fibonacci() {
    assert_ok(
        r#"
        fn fib(n: int) -> int {
            if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
        }
        fn main() -> () {
            println(to_string(fib(20)));
        }
        "#,
    );
}

#[test]
fn valid_02_example_2_closure_counter_factory() {
    assert_ok(
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
            println(to_string(counter_b()));
        }
        "#,
    );
}

#[test]
fn valid_03_example_3_array_mutability() {
    assert_ok(
        r#"
        fn sum(xs: [int]) -> int {
            fn go(xs: [int], i: int, acc: int) -> int {
                if i >= xs.length() { acc } else { go(xs, i + 1, acc + xs[i]) }
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
fn valid_04_example_4_bubble_sort() {
    assert_ok(
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
fn valid_05_example_5_mutual_recursion() {
    assert_ok(
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

#[test]
fn valid_06_simple_recursion_factorial() {
    assert_ok(
        "
        fn factorial(n: int) -> int {
            if n <= 1 { 1 } else { n * factorial(n - 1) }
        }
        fn main() -> () { let r = factorial(5); }
        ",
    );
}

#[test]
fn valid_07_nested_scope_shadowing() {
    assert_ok(
        r#"
        fn main() -> () {
            let x = 1;
            let s = {
                let x = "shadowed";
                x
            };
            let y = x;
        }
        "#,
    );
}

#[test]
fn valid_08_nested_closures_naming_collision() {
    // Inner-scope `x` should win inside the innermost closure; both
    // levels must independently type-check.
    assert_ok(
        "
        fn outer(x: int) -> fn() -> int {
            fn() -> int {
                let x = x + 1;
                fn() -> int {
                    let x = x + 1;
                    x
                }()
            }
        }
        fn main() -> () { let r = outer(1)(); }
        ",
    );
}

#[test]
fn valid_09_while_loop_mutating_counter() {
    assert_ok(
        "
        fn main() -> () {
            let mut i = 0;
            while i < 10 {
                i = i + 1;
            }
        }
        ",
    );
}

#[test]
fn valid_10_if_else_as_value() {
    assert_ok(r#"fn main() -> () { let x = if 1 < 2 { "yes" } else { "no" }; }"#);
}

#[test]
fn valid_11_bare_if_statement() {
    assert_ok("fn main() -> () { if true { let x = 1; } }");
}

#[test]
fn valid_12_strict_else_if_chain_with_final_else() {
    assert_ok(
        "
        fn classify(n: int) -> string {
            if n < 0 { \"negative\" } else if n == 0 { \"zero\" } else { \"positive\" }
        }
        fn main() -> () { let r = classify(5); }
        ",
    );
}

#[test]
fn valid_13_array_construct_index_length_push() {
    assert_ok(
        "
        fn main() -> () {
            let mut xs = [1, 2, 3];
            xs.push(4);
            let n = xs.length();
            let first = xs[0];
        }
        ",
    );
}

#[test]
fn valid_14_higher_order_function_returning_function() {
    assert_ok(
        "
        fn adder(x: int) -> fn(int) -> int {
            fn(y: int) -> int { x + y }
        }
        fn main() -> () { let add5 = adder(5); let r = add5(3); }
        ",
    );
}

#[test]
fn valid_15_function_taking_a_function_parameter() {
    assert_ok(
        "
        fn apply(f: fn(int) -> int, x: int) -> int { f(x) }
        fn double(x: int) -> int { x * 2 }
        fn main() -> () { let r = apply(double, 21); }
        ",
    );
}

#[test]
fn valid_16_string_stdlib_usage() {
    assert_ok(
        r#"
        fn main() -> () {
            let greeting = concat("hello, ", "world");
            let n = str_length(greeting);
            println(greeting);
        }
        "#,
    );
}

#[test]
fn valid_17_mut_parameter() {
    assert_ok("fn increment(mut x: int) -> int { x = x + 1; x }");
}

#[test]
fn valid_18_top_level_forward_reference() {
    assert_ok(
        "
        fn main() -> () { let r = helper(5); }
        fn helper(n: int) -> int { n + 1 }
        ",
    );
}

#[test]
fn valid_19_recursive_closure_via_let_mut() {
    assert_ok(
        "
        fn main() -> () {
            fn countdown(n: int) -> () {
                if n > 0 {
                    println(to_string(n));
                    countdown(n - 1);
                }
            }
            countdown(3);
        }
        ",
    );
}

#[test]
fn valid_20_unary_operators() {
    assert_ok("fn main() -> () { let a = -5; let b = !true; let c = -1.5; }");
}
