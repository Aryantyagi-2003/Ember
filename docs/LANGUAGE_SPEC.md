# The Ember Language — Specification (v0.1, draft for review)

Ember is a small, statically-typed, expression-oriented language in the
minimal ML/Rust tradition. This document is the source of truth for the
language design and is written *before* any lexer/parser/interpreter code,
per the project's own discipline: language design is the most expensive
thing to change after the fact.

## 1. Design goals

- Small enough for one person to implement lexer → parser → type checker →
  interpreter by hand, with real test coverage, in a portfolio-scoped
  timeframe.
- Expression-oriented: almost everything (including `if` and blocks)
  produces a value. Statements are a thin, deliberately small subset.
- Statically typed with local type inference, explicit annotations required
  on function signatures. No runtime type errors reachable through a
  program that passed the type checker (modulo the panics below).
- First-class functions and correct lexical-scope closures are the
  centerpiece correctness bar for the interpreter.

## 2. Lexical structure

- Source is UTF-8. Tokens: identifiers, keywords, integer literals, float
  literals, string literals (double-quoted, `\n \t \" \\` escapes), `true`/
  `false`, operators, punctuation, comments.
- Comments: `//` to end of line. No block comments in v1.
- Every token carries `(line, column)` (1-indexed) captured at the token's
  first character. Every later stage (parser, type checker, interpreter)
  threads this location into its own diagnostics — a bare "type mismatch"
  with no location is treated as a bug in Ember's own implementation.
- Keywords (reserved, cannot be used as identifiers):
  `let`, `mut`, `fn`, `if`, `else`, `while`, `true`, `false`, `return`,
  `panic`, `int`, `float`, `bool`, `string`.

## 3. Types

Primitive types:

| Type     | Example literals        |
|----------|--------------------------|
| `int`    | `0`, `42`, `-7`          |
| `float`  | `3.14`, `0.5`, `-2.0`    |
| `bool`   | `true`, `false`          |
| `string` | `"hello"`                |

Compound type — **arrays**, chosen as the v1 baseline over structs/records:

- `[T]` is the type of a homogeneous array of `T`. Literal syntax:
  `[1, 2, 3]` (type `[int]`), `[]` requires a type annotation on the
  binding (`let xs: [int] = []`) since an empty literal carries no element
  type on its own.
- Arrays are chosen over structs because they (a) directly satisfy the
  required stdlib surface (`index`, `length`, `push`), (b) are enough to
  write a non-trivial example program (sorting), and (c) avoid spending the
  type-checker/interpreter budget on nominal-vs-structural typing and field
  resolution, which competes directly with the harder, required bar:
  closures. Structs/records are the documented stretch goal (§10).

Function types: `fn(T1, T2) -> R`, used internally for type-checking
first-class function values; there is no surface syntax to *write* a
function type outside of a `fn` declaration/literal, since Ember does not
yet support passing an anonymous function type annotation independent of a
literal (see §10, function-typed parameters are still supported — the
parameter's annotation is just the literal shape `fn(int) -> int`, reusing
the same grammar production).

There is no implicit numeric coercion: `int` and `float` are distinct
types, and mixing them (e.g. `1 + 1.0`) is a type error. An explicit
`int_to_float(x)` / `float_to_int(x)` stdlib conversion is provided.

## 4. Type inference

- Local `let` bindings infer their type from the initializer expression:
  `let x = 5` infers `x: int`, no annotation needed.
- Function parameters and return types require **explicit annotations** —
  this is the v1 baseline. Full Hindley-Milner-style inference across
  function boundaries is the documented stretch goal (§10); it is not
  attempted in v1 because unifying it correctly with closures and
  first-class functions is a substantially larger project on its own and
  would compete with the closure-correctness bar, which is the harder
  and more load-bearing requirement here.
- Inference is purely local/bidirectional in v1: the checker infers a
  `let`'s type from its RHS, and checks (does not infer) everything else
  against explicit annotations and already-known types.

## 5. Mutability

Ember bindings are **immutable by default**; mutation requires an explicit
`mut` keyword:

```
let x = 5;        // immutable — re-assigning x is a type-checker error
let mut y = 5;     // mutable — y = y + 1; is allowed
```

Reasoning: this is the Rust/ML convention, it gives the language a real
opinion worth writing about (rather than defaulting to "everything is
mutable" by accident), and — most importantly for this project — it makes
the closure-capture tests meaningful. Because mutability is opt-in and
visible at the binding site, a test can show precisely that a closure
capturing a `mut` variable observes later mutations, and that this is a
deliberate, narrow capability rather than an accidental consequence of
"everything is a mutable reference." Function parameters are immutable
bindings within the function body unless declared `mut` in the parameter
list (`fn f(mut x: int) -> int`).

Arrays: the *binding* to an array follows the same `let`/`let mut` rule as
any other value, but array *elements* are mutable through `push`/index
assignment only when the binding itself is `mut` (i.e. `let mut xs = [1,2]`
allows `xs.push(3)`; `let xs = [1,2]` does not).

## 6. Closures — semantics (the hard correctness bar)

Ember closures **capture their enclosing environment by reference**, not
by value:

- When a function/closure literal is created, it captures a reference to
  the environment (chain of scopes) active at its definition site — not a
  snapshot/copy of the variables' current values.
- A later mutation to a captured `mut` variable, performed either by the
  enclosing scope or by the closure itself, is visible through that shared
  reference everywhere the environment is reachable.
- Each *call* to a function that returns a new closure allocates a **new**
  environment frame for that call. Two closures created by two separate
  calls to the same factory function are therefore independent — they do
  not share the factory's locals with each other, even though each shares
  its own call's locals with its own closure.
- Variable resolution is standard lexical scoping: a name resolves to the
  nearest enclosing scope that declares it, at the closure's *definition*
  site, not its call site (no dynamic scoping). Shadowing follows normal
  block rules — an inner declaration of the same name hides the outer one
  for the rest of the inner scope.
- Recursion (including mutual recursion between two closures assigned to
  `let`/`let mut` bindings) works via the same environment mechanism: a
  named function's own binding is visible inside its own body. The
  reference tree-walking interpreter must not grow the native call stack
  unboundedly for straightforward recursion depths used in the test suite
  (≥1000); if plain recursive `eval` calls risk a native stack overflow at
  that depth, the interpreter will use an explicit trampoline/work-stack
  for function application rather than relying on Rust's call stack. (This
  is an implementation-detail commitment, not a language-semantics one —
  called out here because it directly gates the closures test suite.)

This is exercised in the test suite by, at minimum: a mutable counter
closure, a closure-factory-produces-independent-closures test, ≥1000-deep
recursive closure test, and a nested-closure shadowing test — see
`ROADMAP.md` / test plan once implementation begins.

### 6.1 Sibling function hoisting (mutual recursion)

**Grammar note (fixed during parser implementation):** this section, as
originally written, presupposed that a named `fn_decl` can appear inside
an arbitrary block — the mutual-recursion example in `EXAMPLES_DRAFT.md`
declares `is_even`/`is_odd` inside `main`'s body — but the §11 grammar as
first drafted only allowed `fn_decl` at the top-level `program := item*`,
with no corresponding `stmt` alternative. That was a real gap between
this section and the grammar, not a stylistic choice; §11 has been
updated to add `fn_decl_stmt` as a `stmt` alternative (see below), and
`StmtKind` in `src/ast/mod.rs` gained a matching `FnDecl` variant so a
block's statement list can actually represent one. With that fix, the
rest of this section holds as originally written:

A named `fn_decl` desugars to `let NAME = fn_lit` (§11), and ordinary
`let` bindings are only visible to code *after* them in the same block —
this is standard lexical order and is unaffected by this section. Named
function declarations are the one deliberate exception:

**All `fn_decl` items appearing directly in a given block are hoisted and
bound — each to a closure over that block's own environment — before any
of the block's statements execute.** This means sibling function
declarations in the same block can call each other regardless of textual
order, which is what makes mutual recursion between two named functions
(e.g. `is_even`/`is_odd`, see `EXAMPLES_DRAFT.md`) work without a forward-
declaration mechanism.

Non-function `let` bindings are explicitly **not** hoisted — a `let`
binding is only visible in the block after its own declaration, as usual.
Only the `fn_decl` sugar gets this treatment, and only within the single
block it's declared in (hoisting does not cross block boundaries).

This is an environment-model commitment for the interpreter, not merely a
parser detail: when evaluating a block, the interpreter must do a first
pass that creates and binds all of that block's `fn_decl` closures (over
the block's own new environment frame) before evaluating any statement in
program order, then proceed with normal sequential execution for
everything else.

## 7. Control flow

- `if`/`else` is an **expression**: `if cond { e1 } else { e2 }` has a
  value and a type, and both branches must have the same type. An `if`
  used as a statement (no value consumed) with no `else` is permitted and
  has type `()` in that position only. As of the §11 grammar update, `else`
  is *required* structurally by the grammar wherever `if` is reachable as
  a value (`primary`/`if_expr`) — a bare `if` with no `else` is a distinct
  production (`bare_if_stmt`, statement-position only) rather than an
  `if_expr` with an omitted branch, so a missing `else` in value position
  is a **parse error**, not a type error (superseding an earlier draft of
  this section, which deferred the check to the type checker).
  Because the grammar's `if_expr` production is itself recursive
  (`"else" (block | if_expr)`), this requirement is strict at every level
  of an `else if` chain: once a chain contains one `else`, every branch
  down that chain must terminate in an `else` too, whether or not the
  chain's value is ultimately used. `if a { .. } else if b { .. }` with no
  final `else` is a `ParseError` even in throwaway/statement position —
  not just when its value is consumed. This is a deliberate, stricter-
  than-strictly-necessary design opinion: an incomplete `else if` cascade
  is treated as very likely a real bug in the programmer's Ember code,
  not a construct worth accommodating. Confirmed as the intended behavior
  after being flagged during parser implementation; open to revisiting if
  it proves awkward against real example programs.
- `while cond { body }` is a statement-position looping construct; it
  always has type `()`. It is not an expression (unlike `if`) because a
  loop has no well-defined single value in the general case without a
  `break value` mechanism, which is out of scope for v1.
- **Statement-position block-like constructs and semicolons.** `while`
  loops and a bare `if` (no `else`) are always `()`-valued. When either
  appears as a statement — including as the last item in a block — a
  trailing `;` is optional: `while cond { body }` and `if cond { body }`
  are each already complete statements on their own, with or without a
  following `;`. This removes the need for an empty-`else`/trailing-`;`
  workaround purely to satisfy the grammar (see the bubble sort example
  in `EXAMPLES_DRAFT.md`).
  - This is not just a lexical nicety — it requires an explicit
    statement-position restriction to stay unambiguous for a
    single-lookahead, hand-written recursive-descent parser: **a
    block-like construct (`if` with or without `else`, `while`, or a
    bare `{ ... }` block) that begins a statement is parsed as a
    complete, self-contained statement and never continues into a
    trailing binary or postfix operator.** For example,
    `if c { 1 } else { 2 } + 1;` at statement position parses as the
    complete statement `if c { 1 } else { 2 }` followed by a separate
    (here, malformed) statement starting with `+ 1;` — it does *not*
    parse as `(if c {1} else {2}) + 1`. To combine such a construct's
    value into a larger expression, either parenthesize it
    (`(if c { 1 } else { 2 }) + 1`) or use it in a non-statement-initial
    position, e.g. as a `let` initializer's RHS. This is the same
    restriction Rust uses for the same reason, and is a deliberate,
    documented grammar decision, not an incidental one.
- Recursive functions are the primary iteration/"map over a collection"
  story in v1 — there is no `for` loop. This is a deliberate minimalism
  choice consistent with the ML lineage, and it doubles as forcing
  function/closure code paths to be exercised more heavily by example
  programs.
- Blocks `{ ... }` are expressions: the value of a block is the value of
  its last expression if it is not terminated by `;`; a block ending in
  `;` (or empty) has type `()`. Standard ML/Rust block-value convention.

## 8. Error handling

**Panic-based for v1.** Ember has no `Result`/`Option`/sum types or pattern
matching yet (see §10 for the stretch goal). Two distinct error channels:

1. **Compile-time (type) errors** — anything the type checker rejects
   (wrong arg count/type, undeclared variable, non-bool condition,
   indexing a non-array, assigning wrong type, assigning to a non-`mut`
   binding). These never reach the interpreter; the program simply does
   not run, and the checker reports every error it finds (not just the
   first) with line/column.
2. **Runtime panics** — a builtin `panic(msg: string) -> never` stdlib
   function that unwinds the *Ember program* (not the host Rust process)
   and reports `msg` with the source location of the `panic(...)` call
   site, then halts evaluation of that top-level run/REPL entry cleanly.
   Also triggered by a small fixed set of runtime-only failure modes that
   the static checker cannot rule out: array index out of bounds, integer
   division by zero. These are documented, deliberate, minimal — not a
   general exception system.

Reasoning: a `Result<T, E>` requires generics (or at least one built-in
parametric enum) and, to be ergonomic, some form of `match`/`?`-like
propagation — real language features that are their own design and
implementation project. Given the project's actual hard bar is closures +
a real type checker, `panic` keeps error handling *specified and
consistent* without competing for that budget. `Result<T,E>` + `match` is
the documented stretch goal (§10).

## 9. Standard library (v1 baseline)

All builtins are free functions (no methods/traits in v1), except array
operations which use `.method()` call syntax for readability since arrays
are the one compound type:

| Signature | Description |
|---|---|
| `print(s: string) -> ()` | write `s` to stdout, no trailing newline |
| `println(s: string) -> ()` | write `s` to stdout with a trailing newline |
| `to_string(x: int) -> string` / `(x: float) -> string` / `(x: bool) -> string` | stringify a primitive |
| `concat(a: string, b: string) -> string` | string concatenation |
| `str_length(s: string) -> int` | string length (bytes... actually chars, documented) |
| `int_to_float(x: int) -> float` / `float_to_int(x: float) -> int` | explicit numeric conversion |
| `xs.length() -> int` | array length |
| `xs.push(v: T) -> ()` | append (requires `xs` bound `mut`) |
| indexing `xs[i]` | not a stdlib call — first-class syntax, checked at runtime for bounds |

`print`/`println` overload on argument type at the call site is **not**
supported (no overloading in v1) — `println` takes `string` only; callers
use `to_string` to convert. This keeps the type checker's function-call
rule uniform (one signature per name) rather than needing overload
resolution.

## 10. Explicitly out of scope for v1 (stretch goals, in priority order)

1. Bytecode VM as an alternate execution backend behind the same
   type-checked AST (stated stretch goal).
2. Hindley-Milner inference across function boundaries (infer, don't
   require, parameter/return types).
3. `Result<T, E>` / `Option<T>` as real parametric types, plus `match`.
4. Structs/records as a second compound type.
5. `for`/iterator protocol, `break value` from loops.
6. Alternate parser-generator-based implementation (documented
   alternative, not primary).

## 11. Grammar (informal EBNF)

```ebnf
program        := item* ;
item           := fn_decl ;

fn_decl        := "fn" IDENT "(" params? ")" "->" type block ;
params         := param ("," param)* ;
param          := "mut"? IDENT ":" type ;

type           := "int" | "float" | "bool" | "string"
                 | "(" ")"                                      // unit type — see note below
                 | "[" type "]"
                 | "fn" "(" (type ("," type)*)? ")" "->" type ;

block          := "{" stmt* expr? "}" ;

stmt           := let_stmt
                 | while_stmt
                 | bare_if_stmt
                 | return_stmt
                 | fn_decl_stmt
                 | expr_stmt ;

let_stmt       := "let" "mut"? IDENT (":" type)? "=" expr ";" ;
while_stmt     := "while" expr block ";"? ;
bare_if_stmt   := "if" expr block ";"? ;        // no `else` — statement position only, always `()`
return_stmt    := "return" expr? ";" ;
fn_decl_stmt   := fn_decl ;                     // same production as the item form, §6.1; no trailing `;` needed — block-terminated like while/bare-if
expr_stmt      := expr ";" ;

expr           := assign_expr ;
assign_expr    := IDENT "=" assign_expr
                 | array_index "=" assign_expr
                 | or_expr ;
or_expr        := and_expr ("||" and_expr)* ;
and_expr       := eq_expr ("&&" eq_expr)* ;
eq_expr        := rel_expr (("==" | "!=") rel_expr)* ;
rel_expr       := add_expr (("<" | "<=" | ">" | ">=") add_expr)* ;
add_expr       := mul_expr (("+" | "-") mul_expr)* ;
mul_expr       := unary_expr (("*" | "/" | "%") unary_expr)* ;
unary_expr     := ("-" | "!") unary_expr | call_expr ;
call_expr      := primary (call_suffix)* ;
call_suffix    := "(" args? ")" | "[" expr "]" | "." IDENT "(" args? ")" ;
args           := expr ("," expr)* ;

primary        := INT_LIT | FLOAT_LIT | STRING_LIT | "true" | "false"
                 | IDENT
                 | "(" expr ")"
                 | array_lit
                 | if_expr
                 | fn_lit
                 | block ;

array_lit      := "[" args? "]" ;
if_expr        := "if" expr block "else" (block | if_expr) ;
fn_lit         := "fn" "(" params? ")" "->" type block ;   // anonymous closure literal
```

Notes:
- **`()` (unit type), fixed during parser implementation.** Every example
  and §9's stdlib table writes `-> ()` for a function with no meaningful
  return value, but the original `type` production never actually
  admitted `()` as a type — a genuine grammar gap (caught the same way as
  the `fn_decl`-in-block gap above: the parser wouldn't parse `fn main()
  -> () { .. }` at all). Fixed by adding `"(" ")"` as a `type` alternative
  and a corresponding `TypeAnn::Unit` AST variant. `()` is only a type
  annotation here — it is not (yet) a general unit *value* literal
  distinct from "a block with no tail expression"; blocks/`while`/bare-
  `if` already evaluate to "no value" without needing a spelled-out `()`
  literal in expression position (§7), which is why this gap went
  unnoticed until the parser tried to parse real example programs.
- `if` without `else` is only legal as `bare_if_stmt`, a dedicated
  statement production (not reachable through `primary`/`if_expr`, which
  always requires `else`) — enforced structurally by the grammar itself
  now, not deferred to the type checker as an earlier draft of this spec
  proposed.
- **Statement-initial block-like constructs don't continue into trailing
  operators.** When `if`, `while`, or a bare `{ ... }` block begins a
  statement, the parser commits to it as a complete statement at its
  closing `}` and does not attempt to parse a following binary or postfix
  operator as a continuation of it (see §7 for the full rationale and an
  example). Concretely: inside `stmt*`, a `stmt` beginning with `if` is
  dispatched to `bare_if_stmt` or the `if_expr`-as-`expr_stmt` case purely
  by which of `if_expr`'s own productions matches (presence of `else`),
  and in neither case does parsing continue past the construct's closing
  brace looking for `+`, `-`, `(`, `[`, `.`, etc. To use such a
  construct's value inside a larger expression, parenthesize it or place
  it in a non-statement-initial position (e.g. a `let` initializer). This
  keeps the grammar single-token-lookahead and unambiguous for the
  hand-written recursive-descent parser, at the cost of requiring parens
  in the (rare) case where a block-like construct's value needs to feed
  directly into an operator from statement-initial position.
- Named `fn` declarations (`item`) and anonymous `fn` literals (`fn_lit`,
  used for closures assigned to `let`) share the same parameter/body
  grammar; a named `fn_decl` is sugar for `let NAME = fn_lit`, which also
  gives named top-level functions the same first-class-value treatment
  closures get (this is what makes recursion via self-reference and
  mutual recursion work uniformly).
- `fn_decl` is reachable both as a top-level `item` and, via
  `fn_decl_stmt`, inside any `block` (§6.1) — the same underlying
  production (`"fn" IDENT "(" params? ")" "->" type block`) in both
  positions, which is why one shared parser routine builds both.
