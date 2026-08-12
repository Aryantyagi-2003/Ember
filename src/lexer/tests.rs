use super::*;

fn kinds(src: &str) -> Vec<TokenKind> {
    Lexer::new(src)
        .tokenize()
        .expect("expected successful tokenization")
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

fn lex_err(src: &str) -> LexError {
    Lexer::new(src)
        .tokenize()
        .expect_err("expected a lex error")
}

// ---- literals ----

#[test]
fn ints() {
    assert_eq!(kinds("0"), vec![TokenKind::Int(0), TokenKind::Eof]);
    assert_eq!(kinds("42"), vec![TokenKind::Int(42), TokenKind::Eof]);
    assert_eq!(
        kinds("1234567890"),
        vec![TokenKind::Int(1234567890), TokenKind::Eof]
    );
}

#[test]
fn floats() {
    assert_eq!(kinds("3.25"), vec![TokenKind::Float(3.25), TokenKind::Eof]);
    assert_eq!(kinds("0.5"), vec![TokenKind::Float(0.5), TokenKind::Eof]);
    assert_eq!(kinds("2.0"), vec![TokenKind::Float(2.0), TokenKind::Eof]);
}

#[test]
fn bools() {
    assert_eq!(
        kinds("true false"),
        vec![
            TokenKind::Bool(true),
            TokenKind::Bool(false),
            TokenKind::Eof
        ]
    );
}

#[test]
fn strings_basic() {
    assert_eq!(
        kinds(r#""hello""#),
        vec![TokenKind::Str("hello".to_string()), TokenKind::Eof]
    );
    assert_eq!(
        kinds(r#""""#),
        vec![TokenKind::Str("".to_string()), TokenKind::Eof]
    );
}

#[test]
fn strings_all_four_escapes() {
    assert_eq!(
        kinds(r#""a\nb""#),
        vec![TokenKind::Str("a\nb".to_string()), TokenKind::Eof]
    );
    assert_eq!(
        kinds(r#""a\tb""#),
        vec![TokenKind::Str("a\tb".to_string()), TokenKind::Eof]
    );
    assert_eq!(
        kinds(r#""a\"b""#),
        vec![TokenKind::Str("a\"b".to_string()), TokenKind::Eof]
    );
    assert_eq!(
        kinds(r#""a\\b""#),
        vec![TokenKind::Str("a\\b".to_string()), TokenKind::Eof]
    );
    // all four combined
    assert_eq!(
        kinds(r#""\n\t\"\\""#),
        vec![TokenKind::Str("\n\t\"\\".to_string()), TokenKind::Eof]
    );
}

#[test]
fn identifiers() {
    assert_eq!(
        kinds("foo bar_baz _x9"),
        vec![
            TokenKind::Ident("foo".to_string()),
            TokenKind::Ident("bar_baz".to_string()),
            TokenKind::Ident("_x9".to_string()),
            TokenKind::Eof
        ]
    );
}

#[test]
fn every_keyword() {
    let src = "let mut fn if else while true false return panic int float bool string";
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Let,
            TokenKind::Mut,
            TokenKind::Fn,
            TokenKind::If,
            TokenKind::Else,
            TokenKind::While,
            TokenKind::Bool(true),
            TokenKind::Bool(false),
            TokenKind::Return,
            TokenKind::Panic,
            TokenKind::KwInt,
            TokenKind::KwFloat,
            TokenKind::KwBool,
            TokenKind::KwString,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn every_operator_and_punctuation() {
    let src = "+ - * / % == != < <= > >= && || ! = -> ( ) { } [ ] , : ; .";
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::EqEq,
            TokenKind::NotEq,
            TokenKind::Lt,
            TokenKind::LtEq,
            TokenKind::Gt,
            TokenKind::GtEq,
            TokenKind::AndAnd,
            TokenKind::OrOr,
            TokenKind::Not,
            TokenKind::Eq,
            TokenKind::Arrow,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::LBracket,
            TokenKind::RBracket,
            TokenKind::Comma,
            TokenKind::Colon,
            TokenKind::Semicolon,
            TokenKind::Dot,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn comments_are_skipped() {
    assert_eq!(
        kinds("1 // this is a comment\n2"),
        vec![TokenKind::Int(1), TokenKind::Int(2), TokenKind::Eof]
    );
    assert_eq!(kinds("// only a comment"), vec![TokenKind::Eof]);
}

#[test]
fn ident_vs_keyword_boundary() {
    // "iffy" must lex as one identifier, not `if` + `fy`.
    assert_eq!(
        kinds("iffy letter"),
        vec![
            TokenKind::Ident("iffy".to_string()),
            TokenKind::Ident("letter".to_string()),
            TokenKind::Eof
        ]
    );
}

// ---- line/column tracking ----

#[test]
fn single_line_columns() {
    let tokens = Lexer::new("let x = 5;").tokenize().unwrap();
    let cols: Vec<u32> = tokens.iter().map(|t| t.span.col).collect();
    // "let"(1) " x"(5) "="(7) "5"(9) ";"(10) EOF(11)
    assert_eq!(cols, vec![1, 5, 7, 9, 10, 11]);
    assert!(tokens.iter().all(|t| t.span.line == 1));
}

#[test]
fn multiline_line_tracking() {
    let src = "let x = 1;\nlet y = 2;\nlet z = 3;";
    let tokens = Lexer::new(src).tokenize().unwrap();
    let lines: Vec<u32> = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Let))
        .map(|t| t.span.line)
        .collect();
    assert_eq!(lines, vec![1, 2, 3]);
}

#[test]
fn multiline_column_resets_after_newline() {
    let src = "abc\nde";
    let tokens = Lexer::new(src).tokenize().unwrap();
    // `abc` at line 1 col 1; `de` at line 2 col 1
    assert_eq!(tokens[0].span, Span { line: 1, col: 1 });
    assert_eq!(tokens[1].span, Span { line: 2, col: 1 });
}

#[test]
fn error_locations_are_reported() {
    let err = lex_err("let x = 5\n  @");
    assert_eq!(err.span, Span { line: 2, col: 3 });
}

// ---- malformed input: located errors, not panics ----

#[test]
fn unterminated_string_errors() {
    let err = lex_err(r#""hello"#);
    assert!(err.message.to_lowercase().contains("unterminated"));
    assert_eq!(err.span, Span { line: 1, col: 1 });
}

#[test]
fn unterminated_string_across_newline_errors() {
    let err = lex_err("\"hello\nworld\"");
    assert!(err.message.to_lowercase().contains("unterminated"));
}

#[test]
fn invalid_escape_sequence_errors() {
    let err = lex_err(r#""bad \q escape""#);
    assert!(err.message.to_lowercase().contains("escape"));
}

#[test]
fn dangling_backslash_at_eof_errors() {
    let err = lex_err("\"abc\\");
    assert!(err.message.to_lowercase().contains("unterminated"));
}

#[test]
fn invalid_numeric_literal_trailing_dot_errors() {
    let err = lex_err("3.");
    assert!(err.message.to_lowercase().contains("numeric"));
}

#[test]
fn invalid_numeric_literal_letters_errors() {
    let err = lex_err("123abc");
    assert!(err.message.to_lowercase().contains("numeric"));
}

#[test]
fn unrecognized_character_errors_not_panics() {
    let err = lex_err("let x = 5; @");
    assert!(err.message.contains('@'));
    assert_eq!(err.span, Span { line: 1, col: 12 });
}

#[test]
fn lone_ampersand_errors() {
    let err = lex_err("a & b");
    assert!(err.message.contains('&'));
}

#[test]
fn lone_pipe_errors() {
    let err = lex_err("a | b");
    assert!(err.message.contains('|'));
}

// ---- integration-flavored: a small real snippet ----

#[test]
fn small_function_snippet_tokenizes_cleanly() {
    let src = "fn add(x: int, y: int) -> int {\n    x + y\n}";
    let kinds = kinds(src);
    assert_eq!(
        kinds,
        vec![
            TokenKind::Fn,
            TokenKind::Ident("add".to_string()),
            TokenKind::LParen,
            TokenKind::Ident("x".to_string()),
            TokenKind::Colon,
            TokenKind::KwInt,
            TokenKind::Comma,
            TokenKind::Ident("y".to_string()),
            TokenKind::Colon,
            TokenKind::KwInt,
            TokenKind::RParen,
            TokenKind::Arrow,
            TokenKind::KwInt,
            TokenKind::LBrace,
            TokenKind::Ident("x".to_string()),
            TokenKind::Plus,
            TokenKind::Ident("y".to_string()),
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}
