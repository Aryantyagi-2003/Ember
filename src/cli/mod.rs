//! Ember's command-line surface: the file runner (`ember run <path>.em`)
//! and the REPL (`ember` with no arguments). Both are thin wiring over
//! the four core pipeline stages — no new language semantics live here,
//! and error formatting is never reinvented: every stage's error type
//! already implements `Display` with a located, readable message, so
//! this module just prints that directly.

use crate::ast::ExprKind;
use crate::interpreter;
use crate::lexer::{Lexer, TokenKind};
use crate::parser;
use crate::typecheck;
use std::fs;
use std::io::{self, Write};

#[cfg(test)]
mod tests;

/// Reads `path`, runs it through the full pipeline (lex -> parse -> check
/// -> run), and returns the process exit code: 0 on success, non-zero on
/// any failure (file I/O, lex, parse, type, or runtime error) — so this
/// is script/CI-friendly. Program output goes to stdout; errors go to
/// stderr, the standard Unix convention.
pub fn run_file(path: &str) -> i32 {
    let src = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read '{}': {}", path, e);
            return 1;
        }
    };

    let tokens = match Lexer::new(&src).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let program = match parser::parse(tokens) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let errs = typecheck::check_program(&program);
    if !errs.is_empty() {
        for e in &errs {
            eprintln!("{}", e);
        }
        return 1;
    }

    let (output, result) = interpreter::run_program(program);
    print!("{}", output);
    io::stdout().flush().ok();
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

/// Counts net bracket/paren/brace depth across a token stream — used to
/// detect whether a REPL turn's input is "complete" (balanced) or needs
/// another line. Working on *tokens*, not raw characters, means this is
/// automatically immune to braces inside string literals or `//`
/// comments — the lexer already stripped/validated those correctly.
fn brace_balance(tokens: &[crate::lexer::Token]) -> i32 {
    let mut depth = 0;
    for t in tokens {
        match t.kind {
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Reads one REPL "turn" — one or more lines, kept open with a `...`
/// continuation prompt until brackets balance (or EOF, or a lex error,
/// either of which ends the turn so its own error/partial input can be
/// reported normally). Returns `None` on EOF with nothing typed yet
/// (Ctrl+D at a fresh prompt).
fn read_one_turn() -> Option<String> {
    let stdin = io::stdin();
    let mut buf = String::new();
    let mut first = true;
    loop {
        print!("{}", if first { "ember> " } else { "...    " });
        io::stdout().flush().ok();

        let mut line = String::new();
        let n = stdin.read_line(&mut line).unwrap_or(0);
        if n == 0 {
            println!();
            return if buf.trim().is_empty() {
                None
            } else {
                Some(buf)
            };
        }

        if first {
            let trimmed = line.trim();
            if trimmed == ":quit" || trimmed == ":q" {
                return Some(":quit".to_string());
            }
        }
        buf.push_str(&line);
        first = false;

        match Lexer::new(&buf).tokenize() {
            Ok(tokens) if brace_balance(&tokens) <= 0 => return Some(buf),
            Ok(_) => continue, // unbalanced — read another line
            // Couldn't tokenize at all (e.g. a genuinely invalid
            // character) — nothing more to wait for; let the normal
            // per-turn error path report it.
            Err(_) => return Some(buf),
        }
    }
}

/// Interactive REPL. Each turn re-evaluates the *entire* session's
/// successfully-committed source, wrapped in `{ ... }`, so a `let`/`fn`
/// from an earlier turn is visible on later ones (the "persisted
/// environment" is the growing source itself, replayed through a fresh
/// `Scope` each turn — simpler and safer than trying to splice live state
/// between turns, and correct because Ember evaluation is deterministic).
/// Only the newly-produced suffix of `print`/`println` output is shown
/// each turn, so replaying old turns' side effects doesn't reprint them.
/// A turn that fails at any stage is *not* committed, so a mistake at the
/// prompt doesn't corrupt the session for later turns.
pub fn repl() {
    println!("Ember REPL. Type an expression or statement; Ctrl+D or :quit to exit.");

    let mut session_source = String::new();
    let mut shown_output_len = 0usize;

    while let Some(turn_input) = read_one_turn() {
        let trimmed = turn_input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ":quit" || trimmed == ":q" {
            break;
        }

        let candidate_source = format!("{}\n{}\n", session_source, turn_input);
        let wrapped = format!("{{\n{}\n}}", candidate_source);

        let tokens = match Lexer::new(&wrapped).tokenize() {
            Ok(t) => t,
            Err(e) => {
                println!("{}", e);
                continue;
            }
        };
        let expr = match parser::parse_expr_program(tokens) {
            Ok(e) => e,
            Err(e) => {
                println!("{}", e);
                continue;
            }
        };
        let errs = typecheck::check_expr_program(&expr);
        if !errs.is_empty() {
            for e in &errs {
                println!("{}", e);
            }
            continue;
        }

        let (output, result) = interpreter::eval_expr_program(&expr);
        if output.len() > shown_output_len {
            print!("{}", &output[shown_output_len..]);
            io::stdout().flush().ok();
        }
        match result {
            Ok(v) => {
                println!("=> {:?}", v);
                // If this turn was evaluated as the wrapping block's
                // *tail* expression (no trailing `;` typed — that's
                // exactly how its value got shown via "=>"), storing it
                // verbatim into the growing session source would leave
                // it unterminated, and the *next* turn's code would
                // parse as an illegal continuation glued onto it (e.g.
                // "expected ';', found 'let'"). Appending `;` here turns
                // it into an ordinary, safely-replayable statement whose
                // value is simply discarded on future replays — which is
                // fine, since we've already shown it once, right above.
                let needs_semicolon = matches!(&expr.kind, ExprKind::Block(_, Some(_)));
                let stored_turn = if needs_semicolon {
                    format!("{};", turn_input.trim_end())
                } else {
                    turn_input
                };
                session_source = format!("{}\n{}\n", session_source, stored_turn);
                shown_output_len = output.len();
            }
            Err(e) => {
                println!("{}", e);
                // Not committed: session_source/shown_output_len stay at
                // their last successful values.
            }
        }
    }
}
