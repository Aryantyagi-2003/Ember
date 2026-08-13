use super::*;
use crate::lexer::Lexer;

/// Parses, type-checks, and runs `src` on the *current* thread (fine for
/// shallow tests). Panics loudly (with the compiler-stage error) if any
/// earlier stage fails — these are test-harness assumptions, not program
/// behavior under test.
fn eval(src: &str) -> (String, Result<Value, RuntimeError>) {
    let tokens = Lexer::new(src)
        .tokenize()
        .unwrap_or_else(|e| panic!("expected lex to succeed for:\n{}\ngot: {}", src, e));
    let prog = crate::parser::parse(tokens)
        .unwrap_or_else(|e| panic!("expected parse to succeed for:\n{}\ngot: {}", src, e));
    let errs = crate::typecheck::check_program(&prog);
    assert!(
        errs.is_empty(),
        "expected {} to type-check, got: {:#?}",
        src,
        errs.iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );
    let interp = Interpreter::new();
    let result = interp.run(&prog);
    (interp.output(), result)
}

fn eval_ok(src: &str) -> (String, Value) {
    let (output, result) = eval(src);
    match result {
        Ok(v) => (output, v),
        Err(e) => panic!("expected {} to run successfully, got: {}", src, e),
    }
}

/// Runs `f` on a dedicated thread with the same large stack `run_program`
/// uses, re-raising any panic (e.g. a failed `assert_eq!` inside `f`) in
/// the calling (test) thread so failures still report with a legible
/// message. Used only where the test genuinely needs the larger stack
/// (deep recursion) — everything else runs directly.
fn run_on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let handle = std::thread::Builder::new()
        .stack_size(INTERPRETER_STACK_SIZE)
        .spawn(f)
        .expect("failed to spawn test thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

// =======================================================================
// 1. Basic closure capture
// =======================================================================

#[test]
fn closure_reads_variable_from_enclosing_scope() {
    let (_, v) = eval_ok(
        "
        fn make() -> fn() -> int {
            let x = 42;
            fn() -> int { x }
        }
        fn main() -> int {
            let f = make();
            f()
        }
        ",
    );
    assert_eq!(v, Value::Int(42));
}

// =======================================================================
// 2. Mutation-after-capture — the single most important test in the
//    whole project.
// =======================================================================

#[test]
fn mutation_after_capture_is_visible_inside_the_closure() {
    // `shared` is captured by `get`, then mutated *after* `get` is
    // created but *before* it's called. The captured Scope's Rc<RefCell>
    // cell for `shared` is the same one the mutation writes through, so
    // `get()` must observe 20, not the value at capture time (10).
    let (_, v) = eval_ok(
        "
        fn main() -> int {
            let mut shared = 10;
            let get = fn() -> int { shared };
            shared = 20;
            get()
        }
        ",
    );
    assert_eq!(v, Value::Int(20));
}

#[test]
fn mutation_after_capture_via_closure_itself_mutating() {
    // The closure-based counter example: the closure mutates the
    // captured variable itself, and repeated calls see the accumulated
    // mutation.
    let (_, v) = eval_ok(
        "
        fn make_counter(start: int) -> fn() -> int {
            let mut count = start;
            fn() -> int {
                count = count + 1;
                count
            }
        }
        fn main() -> int {
            let counter = make_counter(0);
            counter();
            counter();
            counter()
        }
        ",
    );
    assert_eq!(v, Value::Int(3));
}

// =======================================================================
// 3. Independent closures from two calls to the same factory
// =======================================================================

#[test]
fn independent_closures_from_two_factory_calls_do_not_interfere() {
    let (_, v) = eval_ok(
        "
        fn make_counter(start: int) -> fn() -> int {
            let mut count = start;
            fn() -> int {
                count = count + 1;
                count
            }
        }
        fn main() -> [int] {
            let counter_a = make_counter(0);
            let counter_b = make_counter(100);
            let a1 = counter_a();
            let a2 = counter_a();
            let b1 = counter_b();
            let a3 = counter_a();
            [a1, a2, b1, a3]
        }
        ",
    );
    assert_eq!(
        v,
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(101), // independent from counter_a's sequence
            Value::Int(3),
        ])))
    );
}

// =======================================================================
// 4. Recursive closures: correctness + deep recursion (>=1000 levels)
//    without a Rust stack overflow.
// =======================================================================

#[test]
fn recursive_closure_calls_itself_by_name_correctly() {
    let (_, v) = eval_ok(
        "
        fn factorial(n: int) -> int {
            if n <= 1 { 1 } else { n * factorial(n - 1) }
        }
        fn main() -> int { factorial(6) }
        ",
    );
    assert_eq!(v, Value::Int(720));
}

#[test]
fn deep_recursion_1200_levels_does_not_overflow_the_stack() {
    // Confirms the large-stack-thread approach (not a trampoline, per
    // the confirmed design decision) actually clears the ≥1000-level
    // bar. Run via the same big-stack mechanism `run_program` uses.
    run_on_big_stack(|| {
        let (_, v) = eval_ok(
            "
            fn count_down(n: int) -> int {
                if n <= 0 { 0 } else { count_down(n - 1) }
            }
            fn main() -> int { count_down(1200) }
            ",
        );
        assert_eq!(v, Value::Int(0));
    });
}

#[test]
fn deep_recursion_via_run_program_entry_point() {
    // Exercises the actual public `run_program` entry point (the one a
    // future CLI/REPL will use), not just the test helper's own
    // big-stack thread, at the same depth.
    let src = "
        fn count_down(n: int) -> int {
            if n <= 0 { 0 } else { count_down(n - 1) }
        }
        fn main() -> () {
            let r = count_down(1200);
            println(to_string(r));
        }
    ";
    let tokens = Lexer::new(src).tokenize().expect("lex ok");
    let prog = crate::parser::parse(tokens).expect("parse ok");
    let errs = crate::typecheck::check_program(&prog);
    assert!(errs.is_empty(), "got: {:#?}", errs);
    let (output, result) = run_program(prog);
    result.expect("expected deep recursion to complete without error");
    assert_eq!(output, "0\n");
}

// =======================================================================
// 5. Mutual recursion at runtime (Example 5, actually executed)
// =======================================================================

#[test]
fn mutual_recursion_actually_executes_correctly() {
    let (output, v) = eval_ok(
        r#"
        fn main() -> () {
            fn is_even(n: int) -> bool {
                if n == 0 { true } else { is_odd(n - 1) }
            }
            fn is_odd(n: int) -> bool {
                if n == 0 { false } else { is_even(n - 1) }
            }
            println(to_string(is_even(10)));
            println(to_string(is_odd(10)));
        }
        "#,
    );
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "true\nfalse\n");
}

// =======================================================================
// 6. Nested closures, shadowing — inner binding wins, per lexical
//    scoping, proven at runtime.
// =======================================================================

#[test]
fn nested_closures_shadowing_inner_wins() {
    // Two levels of nesting, both introducing a binding named `x`; the
    // innermost closure's own `x` must win over both outer `x`s.
    let (_, v) = eval_ok(
        "
        fn outer(x: int) -> fn() -> int {
            fn() -> int {
                let x = x + 100;
                fn() -> int {
                    let x = x + 1000;
                    x
                }()
            }
        }
        fn main() -> int { outer(1)() }
        ",
    );
    // outer x = 1; middle rebinds x = 1 + 100 = 101; inner rebinds
    // x = 101 + 1000 = 1101. If shadowing were broken (e.g. inner `x`
    // resolved to the outer `x = 1` instead of the middle's 101), this
    // would be 1001 instead.
    assert_eq!(v, Value::Int(1101));
}

#[test]
fn nested_closure_captures_from_both_outer_and_immediate_scope() {
    let (_, v) = eval_ok(
        "
        fn outer(a: int) -> fn(int) -> int {
            let b = 10;
            fn(c: int) -> int { a + b + c }
        }
        fn main() -> int { outer(1)(100) }
        ",
    );
    assert_eq!(v, Value::Int(111));
}

// =======================================================================
// 7. All 5 EXAMPLES_DRAFT.md programs, executed end-to-end with
//    asserted output.
// =======================================================================

#[test]
fn example_1_recursive_fibonacci_runs_with_correct_output() {
    let (output, v) = eval_ok(
        r#"
        fn fib(n: int) -> int {
            if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
        }
        fn main() -> () {
            println(to_string(fib(20)));
        }
        "#,
    );
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "6765\n");
}

#[test]
fn example_2_closure_counter_factory_runs_with_correct_output() {
    let (output, v) = eval_ok(
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
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "1\n2\n101\n3\n");
}

#[test]
fn example_3_array_mutability_runs_with_correct_output() {
    let (output, v) = eval_ok(
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
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "15\n");
}

#[test]
fn example_4_bubble_sort_runs_with_correct_output() {
    let (output, v) = eval_ok(
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
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "1\n2\n3\n5\n8\n9\n");
}

#[test]
fn example_5_mutual_recursion_runs_with_correct_output() {
    let (output, v) = eval_ok(
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
    assert_eq!(v, Value::Unit);
    assert_eq!(output, "true\n");
}

// =======================================================================
// Runtime-only error conditions (the checker cannot catch these)
// =======================================================================

#[test]
fn array_index_out_of_bounds_is_a_clean_runtime_error() {
    let (_, result) = eval("fn main() -> int { let xs = [1, 2, 3]; xs[5] }");
    let err = result.expect_err("expected a runtime error");
    assert!(
        err.message.contains("index out of bounds"),
        "got: {}",
        err.message
    );
    assert!(err.span.line >= 1 && err.span.col >= 1);
}

#[test]
fn negative_array_index_is_a_clean_runtime_error() {
    let (_, result) = eval("fn main() -> int { let xs = [1, 2, 3]; let i = 0 - 1; xs[i] }");
    let err = result.expect_err("expected a runtime error");
    assert!(
        err.message.contains("index out of bounds"),
        "got: {}",
        err.message
    );
}

#[test]
fn integer_division_by_zero_is_a_clean_runtime_error() {
    let (_, result) = eval("fn main() -> int { 1 / 0 }");
    let err = result.expect_err("expected a runtime error");
    assert_eq!(err.message, "division by zero");
}

#[test]
fn integer_modulo_by_zero_is_a_clean_runtime_error() {
    let (_, result) = eval("fn main() -> int { 1 % 0 }");
    let err = result.expect_err("expected a runtime error");
    assert_eq!(err.message, "modulo by zero");
}

#[test]
fn panic_produces_a_clean_runtime_error_not_a_process_crash() {
    let (_, result) = eval(r#"fn main() -> () { panic("something went wrong"); }"#);
    let err = result.expect_err("expected panic to produce a runtime error");
    assert!(
        err.message.contains("something went wrong"),
        "got: {}",
        err.message
    );
}

// =======================================================================
// Evaluation order: left-to-right, observable via side effects
// =======================================================================

#[test]
fn binary_operands_evaluate_left_to_right() {
    // Each call appends to `log` before returning its value; the final
    // log order proves lhs ran before rhs.
    let (_, v) = eval_ok(
        "
        fn main() -> [int] {
            let mut log: [int] = [];
            fn left() -> int { log.push(1); 10 }
            fn right() -> int { log.push(2); 20 }
            let sum = left() + right();
            log
        }
        ",
    );
    assert_eq!(
        v,
        Value::Array(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])))
    );
}

#[test]
fn call_arguments_evaluate_left_to_right() {
    let (_, v) = eval_ok(
        "
        fn main() -> [int] {
            let mut log: [int] = [];
            fn f(a: int, b: int, c: int) -> int { a + b + c }
            fn side(n: int) -> int { log.push(n); n }
            f(side(1), side(2), side(3));
            log
        }
        ",
    );
    assert_eq!(
        v,
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3)
        ])))
    );
}

#[test]
fn and_short_circuits_and_does_not_evaluate_rhs() {
    let (_, v) = eval_ok(
        "
        fn main() -> bool {
            let mut ran = false;
            fn side() -> bool { ran = true; true }
            let r = false && side();
            ran
        }
        ",
    );
    assert_eq!(v, Value::Bool(false));
}

#[test]
fn or_short_circuits_and_does_not_evaluate_rhs() {
    let (_, v) = eval_ok(
        "
        fn main() -> bool {
            let mut ran = false;
            fn side() -> bool { ran = true; true }
            let r = true || side();
            ran
        }
        ",
    );
    assert_eq!(v, Value::Bool(false));
}

// =======================================================================
// A few more general correctness checks (mutability model, stdlib, etc.)
// =======================================================================

#[test]
fn while_loop_mutates_correctly() {
    let (_, v) = eval_ok(
        "
        fn main() -> int {
            let mut i = 0;
            let mut sum = 0;
            while i < 5 {
                sum = sum + i;
                i = i + 1;
            }
            sum
        }
        ",
    );
    assert_eq!(v, Value::Int(1 + 2 + 3 + 4));
}

#[test]
fn early_return_from_nested_control_flow_propagates_correctly() {
    let (_, v) = eval_ok(
        "
        fn find_first_even(xs: [int]) -> int {
            let mut i = 0;
            while i < xs.length() {
                if xs[i] % 2 == 0 {
                    return xs[i];
                }
                i = i + 1;
            }
            return -1;
        }
        fn main() -> int { find_first_even([1, 3, 5, 8, 9]) }
        ",
    );
    assert_eq!(v, Value::Int(8));
}

#[test]
fn stdlib_functions_produce_correct_values() {
    let (_, v) = eval_ok(
        r#"
        fn main() -> [string] {
            let a = concat("foo", "bar");
            let n = to_string(str_length(a));
            let f = to_string(int_to_float(5));
            let i = to_string(float_to_int(5.9));
            [a, n, f, i]
        }
        "#,
    );
    assert_eq!(
        v,
        Value::Array(Rc::new(RefCell::new(vec![
            Value::String(Rc::from("foobar")),
            Value::String(Rc::from("6")),
            Value::String(Rc::from("5")),
            Value::String(Rc::from("5")),
        ])))
    );
}
