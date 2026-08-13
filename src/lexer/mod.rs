//! Ember lexer — source text -> token stream (docs/LANGUAGE_SPEC.md §2).
//!
//! Every token carries a 1-indexed (line, column) captured at its first
//! character. Malformed input (unterminated string, invalid escape,
//! invalid numeric literal, unrecognized character) produces a located
//! `LexError`, never a panic — user input, however broken, must never
//! crash the lexer itself.

use crate::ast::Span;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Ident(String),

    // Keywords
    Let,
    Mut,
    Fn,
    If,
    Else,
    While,
    Return,
    KwInt,
    KwFloat,
    KwBool,
    KwString,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    Not,
    Eq,
    Arrow, // ->

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,
    Dot,

    Eof,
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use TokenKind::*;
        match self {
            Int(n) => write!(f, "integer '{}'", n),
            Float(x) => write!(f, "float '{}'", x),
            Str(s) => write!(f, "string {:?}", s),
            Bool(b) => write!(f, "'{}'", b),
            Ident(s) => write!(f, "identifier '{}'", s),
            Let => write!(f, "'let'"),
            Mut => write!(f, "'mut'"),
            Fn => write!(f, "'fn'"),
            If => write!(f, "'if'"),
            Else => write!(f, "'else'"),
            While => write!(f, "'while'"),
            Return => write!(f, "'return'"),
            KwInt => write!(f, "'int'"),
            KwFloat => write!(f, "'float'"),
            KwBool => write!(f, "'bool'"),
            KwString => write!(f, "'string'"),
            Plus => write!(f, "'+'"),
            Minus => write!(f, "'-'"),
            Star => write!(f, "'*'"),
            Slash => write!(f, "'/'"),
            Percent => write!(f, "'%'"),
            EqEq => write!(f, "'=='"),
            NotEq => write!(f, "'!='"),
            Lt => write!(f, "'<'"),
            LtEq => write!(f, "'<='"),
            Gt => write!(f, "'>'"),
            GtEq => write!(f, "'>='"),
            AndAnd => write!(f, "'&&'"),
            OrOr => write!(f, "'||'"),
            Not => write!(f, "'!'"),
            Eq => write!(f, "'='"),
            Arrow => write!(f, "'->'"),
            LParen => write!(f, "'('"),
            RParen => write!(f, "')'"),
            LBrace => write!(f, "'{{'"),
            RBrace => write!(f, "'}}'"),
            LBracket => write!(f, "'['"),
            RBracket => write!(f, "']'"),
            Comma => write!(f, "','"),
            Colon => write!(f, "':'"),
            Semicolon => write!(f, "';'"),
            Dot => write!(f, "'.'"),
            Eof => write!(f, "end of input"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lex error at line {}, column {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

fn keyword(word: &str) -> Option<TokenKind> {
    Some(match word {
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "fn" => TokenKind::Fn,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "return" => TokenKind::Return,
        "int" => TokenKind::KwInt,
        "float" => TokenKind::KwFloat,
        "bool" => TokenKind::KwBool,
        "string" => TokenKind::KwString,
        "true" => TokenKind::Bool(true),
        "false" => TokenKind::Bool(false),
        _ => return None,
    })
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Tokenize the entire input, stopping at the first error.
    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn here(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.advance();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments();
        let start = self.here();

        let c = match self.peek() {
            None => {
                return Ok(Token {
                    kind: TokenKind::Eof,
                    span: start,
                });
            }
            Some(c) => c,
        };

        if c.is_ascii_digit() {
            return self.lex_number(start);
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            return Ok(self.lex_ident_or_keyword(start));
        }
        if c == b'"' {
            return self.lex_string(start);
        }

        // Operators and punctuation.
        macro_rules! two_char {
            ($second:expr, $two:expr, $one:expr) => {{
                self.advance();
                if self.peek() == Some($second) {
                    self.advance();
                    $two
                } else {
                    $one
                }
            }};
        }

        let kind = match c {
            b'+' => {
                self.advance();
                TokenKind::Plus
            }
            b'-' => {
                self.advance();
                if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            b'*' => {
                self.advance();
                TokenKind::Star
            }
            b'/' => {
                self.advance();
                TokenKind::Slash
            }
            b'%' => {
                self.advance();
                TokenKind::Percent
            }
            b'=' => two_char!(b'=', TokenKind::EqEq, TokenKind::Eq),
            b'!' => two_char!(b'=', TokenKind::NotEq, TokenKind::Not),
            b'<' => two_char!(b'=', TokenKind::LtEq, TokenKind::Lt),
            b'>' => two_char!(b'=', TokenKind::GtEq, TokenKind::Gt),
            b'&' => {
                self.advance();
                if self.peek() == Some(b'&') {
                    self.advance();
                    TokenKind::AndAnd
                } else {
                    return Err(LexError {
                        message: "unexpected character '&' (did you mean '&&'?)".to_string(),
                        span: start,
                    });
                }
            }
            b'|' => {
                self.advance();
                if self.peek() == Some(b'|') {
                    self.advance();
                    TokenKind::OrOr
                } else {
                    return Err(LexError {
                        message: "unexpected character '|' (did you mean '||'?)".to_string(),
                        span: start,
                    });
                }
            }
            b'(' => {
                self.advance();
                TokenKind::LParen
            }
            b')' => {
                self.advance();
                TokenKind::RParen
            }
            b'{' => {
                self.advance();
                TokenKind::LBrace
            }
            b'}' => {
                self.advance();
                TokenKind::RBrace
            }
            b'[' => {
                self.advance();
                TokenKind::LBracket
            }
            b']' => {
                self.advance();
                TokenKind::RBracket
            }
            b',' => {
                self.advance();
                TokenKind::Comma
            }
            b':' => {
                self.advance();
                TokenKind::Colon
            }
            b';' => {
                self.advance();
                TokenKind::Semicolon
            }
            b'.' => {
                self.advance();
                TokenKind::Dot
            }
            other => {
                self.advance();
                return Err(LexError {
                    message: format!("unexpected character '{}'", other as char),
                    span: start,
                });
            }
        };

        Ok(Token { kind, span: start })
    }

    fn lex_ident_or_keyword(&mut self, start: Span) -> Token {
        let begin = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        // Internal invariant, not user-input-dependent: every byte in this
        // range was admitted by `is_ascii_alphanumeric() || c == b'_'`
        // above, so it is guaranteed valid ASCII/UTF-8.
        let word = std::str::from_utf8(&self.src[begin..self.pos]).unwrap();
        let kind = keyword(word).unwrap_or_else(|| TokenKind::Ident(word.to_string()));
        Token { kind, span: start }
    }

    fn lex_number(&mut self, start: Span) -> Result<Token, LexError> {
        let begin = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.advance();
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // consume '.'
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.advance();
            }
        } else if self.peek() == Some(b'.') {
            // A digit run followed by '.' with no digit after it is a
            // malformed numeric literal (e.g. "3."), not a valid int
            // followed by a separate '.' token — reject it explicitly
            // rather than silently splitting, since that's almost always
            // a typo, not intentional field access on an int literal.
            let dot_span = Span {
                line: self.line,
                col: self.col,
            };
            return Err(LexError {
                message: "invalid numeric literal: expected a digit after '.'".to_string(),
                span: dot_span,
            });
        }

        // Reject a numeric literal immediately followed by an identifier
        // character, e.g. "123abc" or "1.5e10" (no exponent support) —
        // this is never valid and should error rather than silently
        // tokenizing as two adjacent tokens.
        if matches!(self.peek(), Some(c) if c.is_ascii_alphabetic() || c == b'_') {
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_') {
                self.advance();
            }
            // Internal invariant: range is digits/'.'/alphanumeric/'_' only.
            let text = std::str::from_utf8(&self.src[begin..self.pos]).unwrap();
            return Err(LexError {
                message: format!("invalid numeric literal '{}'", text),
                span: start,
            });
        }

        // Internal invariant: range is digits and at most one '.' only.
        let text = std::str::from_utf8(&self.src[begin..self.pos]).unwrap();
        if is_float {
            let v: f64 = text.parse().map_err(|_| LexError {
                message: format!("invalid float literal '{}'", text),
                span: start,
            })?;
            Ok(Token {
                kind: TokenKind::Float(v),
                span: start,
            })
        } else {
            let v: i64 = text.parse().map_err(|_| LexError {
                message: format!("invalid int literal '{}' (out of range?)", text),
                span: start,
            })?;
            Ok(Token {
                kind: TokenKind::Int(v),
                span: start,
            })
        }
    }

    fn lex_string(&mut self, start: Span) -> Result<Token, LexError> {
        self.advance(); // consume opening '"'
        let mut value = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(LexError {
                        message: "unterminated string literal".to_string(),
                        span: start,
                    });
                }
                Some(b'\n') => {
                    return Err(LexError {
                        message: "unterminated string literal (newline before closing '\"')"
                            .to_string(),
                        span: start,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    let esc_span = self.here();
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some(b'n') => {
                            value.push('\n');
                            self.advance();
                        }
                        Some(b't') => {
                            value.push('\t');
                            self.advance();
                        }
                        Some(b'"') => {
                            value.push('"');
                            self.advance();
                        }
                        Some(b'\\') => {
                            value.push('\\');
                            self.advance();
                        }
                        Some(other) => {
                            return Err(LexError {
                                message: format!("invalid escape sequence '\\{}'", other as char),
                                span: esc_span,
                            });
                        }
                        None => {
                            return Err(LexError {
                                message: "unterminated string literal (dangling '\\')".to_string(),
                                span: start,
                            });
                        }
                    }
                }
                Some(_) => {
                    // Collect a full UTF-8 scalar, not just one byte, so
                    // multi-byte characters in string literals survive.
                    let ch_start = self.pos;
                    let first = self.src[ch_start];
                    let len = utf8_len(first);
                    for _ in 0..len {
                        self.advance();
                    }
                    let s =
                        std::str::from_utf8(&self.src[ch_start..self.pos]).unwrap_or("\u{FFFD}");
                    value.push_str(s);
                }
            }
        }
        Ok(Token {
            kind: TokenKind::Str(value),
            span: start,
        })
    }
}

fn utf8_len(first_byte: u8) -> usize {
    if first_byte & 0b1000_0000 == 0 {
        1
    } else if first_byte & 0b1110_0000 == 0b1100_0000 {
        2
    } else if first_byte & 0b1111_0000 == 0b1110_0000 {
        3
    } else if first_byte & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    }
}
