# Example Ember programs (syntax preview — not yet runnable)

These exist to let the language design be judged by how real programs in
it read, before any lexer/parser is written. Once implementation reaches
the interpreter stage, these become the actual files under `/examples`
that double as integration tests.

## 1. Recursive Fibonacci

```
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
```

## 2. Closure-based counter generator (closure factory)

Demonstrates: capture-by-reference of a `mut` variable, and independent
environments per call to the factory.

```
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

    println(to_string(counter_a()));  // 1
    println(to_string(counter_a()));  // 2
    println(to_string(counter_b()));  // 101 -- independent from counter_a
    println(to_string(counter_a()));  // 3
}
```

## 3. Array manipulation + mutability model

Demonstrates: `mut` array bindings, `push`, indexing, and the compile
error you'd get without `mut` (shown as a comment, since it must not
type-check).

```
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

    println(to_string(sum(nums))); // 15

    // let frozen = [1, 2, 3];
    // frozen.push(4); // <- type error: `frozen` is not declared `mut`
}
```

## 4. Non-trivial: bubble sort (array algorithm)

```
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
```

(Resolved: `while` and bare `if`-without-`else` no longer require a
trailing `;`, including when they're the last item in a block — see
LANGUAGE_SPEC.md §7 and §11. The inner `if result[j] > result[j+1] { ... }`
above no longer needs the empty `else {}` workaround either, since a bare
`if` is already `()`-valued on its own.)

## 5. Mutual recursion between two closures (closure correctness bar)

```
fn main() -> () {
    fn is_even(n: int) -> bool {
        if n == 0 { true } else { is_odd(n - 1) }
    }
    fn is_odd(n: int) -> bool {
        if n == 0 { false } else { is_even(n - 1) }
    }

    println(to_string(is_even(10))); // true
}
```

`is_even`'s body references `is_odd`, which is declared *after* it.
Resolved via sibling `fn_decl` hoisting — see LANGUAGE_SPEC.md §6.1: all
`fn_decl` items in a block are bound, over that block's own environment,
before any of the block's statements execute, so `is_even` and `is_odd`
can see each other regardless of declaration order. Ordinary `let`
bindings are not hoisted and still follow normal lexical order.
