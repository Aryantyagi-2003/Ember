use super::*;

/// The CLI layer is wiring, not new language semantics, so this is a
/// handful of smoke tests, not the exhaustive per-branch coverage the
/// four core pipeline stages have.

#[test]
fn run_file_succeeds_on_a_valid_example() {
    let mut tmp = std::env::temp_dir();
    tmp.push("ember_cli_test_fibonacci.em");
    fs::write(&tmp, "fn main() -> () { println(to_string(1 + 2)); }").unwrap();
    let code = run_file(tmp.to_str().unwrap());
    assert_eq!(code, 0);
    let _ = fs::remove_file(&tmp);
}

#[test]
fn run_file_reports_missing_file_with_nonzero_exit() {
    let code = run_file("/nonexistent/path/does_not_exist.em");
    assert_ne!(code, 0);
}

#[test]
fn run_file_reports_type_error_with_nonzero_exit() {
    let mut tmp = std::env::temp_dir();
    tmp.push("ember_cli_test_bad_types.em");
    fs::write(&tmp, "fn main() -> () { let x: int = \"not an int\"; }").unwrap();
    let code = run_file(tmp.to_str().unwrap());
    assert_ne!(code, 0);
    let _ = fs::remove_file(&tmp);
}

#[test]
fn run_file_reports_parse_error_with_nonzero_exit() {
    let mut tmp = std::env::temp_dir();
    tmp.push("ember_cli_test_bad_syntax.em");
    fs::write(&tmp, "fn main( -> () { }").unwrap();
    let code = run_file(tmp.to_str().unwrap());
    assert_ne!(code, 0);
    let _ = fs::remove_file(&tmp);
}

#[test]
fn run_file_reports_runtime_error_with_nonzero_exit() {
    let mut tmp = std::env::temp_dir();
    tmp.push("ember_cli_test_runtime_error.em");
    fs::write(&tmp, "fn main() -> int { 1 / 0 }").unwrap();
    let code = run_file(tmp.to_str().unwrap());
    assert_ne!(code, 0);
    let _ = fs::remove_file(&tmp);
}

#[test]
fn all_five_example_files_run_successfully() {
    for name in [
        "fibonacci.em",
        "closures.em",
        "array_mutability.em",
        "bubble_sort.em",
        "mutual_recursion.em",
    ] {
        let path = format!("{}/examples/{}", env!("CARGO_MANIFEST_DIR"), name);
        let code = run_file(&path);
        assert_eq!(code, 0, "expected {} to run successfully", name);
    }
}

#[test]
fn brace_balance_counts_all_three_bracket_kinds() {
    let tokens = Lexer::new("{ ( [ ] ) ").tokenize().unwrap();
    assert_eq!(brace_balance(&tokens), 1);
    let tokens = Lexer::new("{ ( [ ] ) }").tokenize().unwrap();
    assert_eq!(brace_balance(&tokens), 0);
}

#[test]
fn brace_balance_ignores_braces_inside_string_literals() {
    // The lexer already turned this into a single Str token, so the '{'
    // inside it never becomes an LBrace token in the first place.
    let tokens = Lexer::new(r#"let s = "{ not a real brace";"#)
        .tokenize()
        .unwrap();
    assert_eq!(brace_balance(&tokens), 0);
}
