//! Ember parser — hand-written recursive descent, token stream -> AST
//! (docs/LANGUAGE_SPEC.md §11). No parser-generator, per the project's
//! constraint. Malformed token streams produce a located `ParseError`,
//! never a panic — same discipline as the lexer.

use crate::ast::*;
use crate::lexer::{Token, TokenKind};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parse error at line {}, column {}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

/// Parse a complete token stream (as produced by `Lexer::tokenize`) into a
/// `Program`.
pub fn parse(tokens: Vec<Token>) -> Result<Program, ParseError> {
    Parser::new(tokens).parse_program()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }
        Ok(Program { items })
    }

    // ---- token-stream helpers ----

    fn peek(&self) -> &Token {
        // `tokens` is always Eof-terminated and `advance` never steps past
        // Eof, so `pos` is always in bounds.
        &self.tokens[self.pos]
    }

    fn is_at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    /// One token of lookahead past the current one. `tokens` is always
    /// non-empty (Eof-terminated by construction from `Lexer::tokenize`),
    /// so `.last()` is guaranteed `Some` — internal invariant, not
    /// user-input-dependent.
    fn peek_next(&self) -> &Token {
        self.tokens
            .get(self.pos + 1)
            .unwrap_or_else(|| self.tokens.last().unwrap())
    }

    fn check(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if !matches!(tok.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        tok
    }

    fn describe_current(&self) -> String {
        format!("{}", self.peek().kind)
    }

    fn error_here(&self, message: String) -> ParseError {
        ParseError {
            message,
            span: self.peek().span,
        }
    }

    fn expect(&mut self, kind: TokenKind, what: &str) -> Result<Token, ParseError> {
        if self.check(&kind) {
            Ok(self.advance())
        } else {
            Err(self.error_here(format!(
                "expected {}, found {}",
                what,
                self.describe_current()
            )))
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match &self.peek().kind {
            TokenKind::Ident(name) => {
                let name = name.clone();
                let span = self.peek().span;
                self.advance();
                Ok((name, span))
            }
            _ => Err(self.error_here(format!(
                "expected {}, found {}",
                what,
                self.describe_current()
            ))),
        }
    }

    // ---- items (top level) ----

    fn parse_item(&mut self) -> Result<Item, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::Fn, "'fn'")?;
        let (name, _) = self.expect_ident("a function name")?;
        let fn_lit = self.parse_fn_signature_and_body(start)?;
        Ok(Item::FnDecl {
            name,
            fn_lit,
            span: start,
        })
    }

    /// Shared by `item` fn_decl, `stmt` fn_decl, and the anonymous
    /// `fn_lit` primary (§11 note: same underlying production in all
    /// three positions). Caller has already consumed `fn` (and, for the
    /// named forms, the function's name) and supplies `start` as the
    /// span to attach to the resulting `FnLit`.
    fn parse_fn_signature_and_body(&mut self, start: Span) -> Result<FnLit, ParseError> {
        self.expect(TokenKind::LParen, "'(' after 'fn'")?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen, "')' to close parameter list")?;
        self.expect(TokenKind::Arrow, "'->' before return type")?;
        let ret = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(FnLit {
            params,
            ret,
            body: Box::new(body),
            span: start,
        })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let start = self.peek().span;
            let mutable = if self.check(&TokenKind::Mut) {
                self.advance();
                true
            } else {
                false
            };
            let (name, _) = self.expect_ident("a parameter name")?;
            self.expect(TokenKind::Colon, "':' before parameter type")?;
            let ty = self.parse_type()?;
            params.push(Param {
                name,
                mutable,
                ty,
                span: start,
            });
            if self.check(&TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<TypeAnn, ParseError> {
        match &self.peek().kind {
            TokenKind::LParen if matches!(self.peek_next().kind, TokenKind::RParen) => {
                self.advance(); // '('
                self.advance(); // ')'
                Ok(TypeAnn::Unit)
            }
            TokenKind::KwInt => {
                self.advance();
                Ok(TypeAnn::Int)
            }
            TokenKind::KwFloat => {
                self.advance();
                Ok(TypeAnn::Float)
            }
            TokenKind::KwBool => {
                self.advance();
                Ok(TypeAnn::Bool)
            }
            TokenKind::KwString => {
                self.advance();
                Ok(TypeAnn::String)
            }
            TokenKind::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                self.expect(TokenKind::RBracket, "']' to close array type")?;
                Ok(TypeAnn::Array(Box::new(inner)))
            }
            TokenKind::Fn => {
                self.advance();
                self.expect(TokenKind::LParen, "'(' in function type")?;
                let mut params = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        params.push(self.parse_type()?);
                        if self.check(&TokenKind::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.expect(
                    TokenKind::RParen,
                    "')' to close function type parameter list",
                )?;
                self.expect(TokenKind::Arrow, "'->' in function type")?;
                let ret = self.parse_type()?;
                Ok(TypeAnn::Function {
                    params,
                    ret: Box::new(ret),
                })
            }
            _ => Err(self.error_here(format!(
                "expected a type, found {}",
                self.describe_current()
            ))),
        }
    }

    // ---- blocks / statements ----

    fn parse_block(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::LBrace, "'{' to start a block")?;

        let mut stmts = Vec::new();
        let mut tail: Option<Box<Expr>> = None;

        loop {
            if self.check(&TokenKind::RBrace) {
                break;
            }
            if self.is_at_end() {
                return Err(self.error_here("unexpected end of input, expected '}'".to_string()));
            }

            match &self.peek().kind {
                TokenKind::Let => stmts.push(self.parse_let_stmt()?),
                TokenKind::While => stmts.push(self.parse_while_stmt()?),
                TokenKind::Return => stmts.push(self.parse_return_stmt()?),
                // Only a *named* `fn` (i.e. `fn IDENT ...`) is the
                // fn_decl_stmt production; a bare `fn(...)` here is an
                // anonymous closure literal used as an ordinary
                // expression (e.g. a block's tail expression, as in the
                // closure-factory example) and must fall through to the
                // general expression-statement path below.
                TokenKind::Fn if matches!(self.peek_next().kind, TokenKind::Ident(_)) => {
                    stmts.push(self.parse_fn_decl_stmt()?)
                }
                TokenKind::If => {
                    let (expr, has_else) = self.parse_if_stmt_head()?;
                    if self.check(&TokenKind::Semicolon) {
                        self.advance();
                        stmts.push(Stmt {
                            span: expr.span,
                            kind: StmtKind::Expr(expr),
                        });
                    } else if !has_else {
                        // Bare `if` (no else): always a statement, `;`
                        // optional, never assigned to `tail` — bare `if`
                        // is not reachable as a value per §11.
                        stmts.push(Stmt {
                            span: expr.span,
                            kind: StmtKind::Expr(expr),
                        });
                    } else if self.check(&TokenKind::RBrace) {
                        tail = Some(Box::new(expr));
                        break;
                    } else {
                        return Err(self.error_here(format!(
                            "expected ';' after if/else statement, found {}",
                            self.describe_current()
                        )));
                    }
                }
                TokenKind::LBrace => {
                    let blk = self.parse_block()?;
                    if self.check(&TokenKind::Semicolon) {
                        self.advance();
                        stmts.push(Stmt {
                            span: blk.span,
                            kind: StmtKind::Expr(blk),
                        });
                    } else if self.check(&TokenKind::RBrace) {
                        tail = Some(Box::new(blk));
                        break;
                    } else {
                        return Err(self.error_here(format!(
                            "expected ';' after block statement, found {}",
                            self.describe_current()
                        )));
                    }
                }
                _ => {
                    let expr = self.parse_expr()?;
                    if self.check(&TokenKind::Semicolon) {
                        self.advance();
                        stmts.push(Stmt {
                            span: expr.span,
                            kind: StmtKind::Expr(expr),
                        });
                    } else if self.check(&TokenKind::RBrace) {
                        tail = Some(Box::new(expr));
                        break;
                    } else {
                        return Err(self.error_here(format!(
                            "expected ';' after expression, found {}",
                            self.describe_current()
                        )));
                    }
                }
            }
        }

        self.expect(TokenKind::RBrace, "'}' to close block")?;
        Ok(Expr {
            kind: ExprKind::Block(stmts, tail),
            span: start,
        })
    }

    fn parse_let_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::Let, "'let'")?;
        let mutable = if self.check(&TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (name, _) = self.expect_ident("a variable name")?;
        let ty = if self.check(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(TokenKind::Eq, "'=' in let binding")?;
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon, "';' after let binding")?;
        Ok(Stmt {
            kind: StmtKind::Let {
                name,
                mutable,
                ty,
                value,
            },
            span: start,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::While, "'while'")?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }
        Ok(Stmt {
            kind: StmtKind::While { cond, body },
            span: start,
        })
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::Return, "'return'")?;
        let value = if self.check(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::Semicolon, "';' after return")?;
        Ok(Stmt {
            kind: StmtKind::Return(value),
            span: start,
        })
    }

    fn parse_fn_decl_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::Fn, "'fn'")?;
        let (name, _) = self.expect_ident("a function name")?;
        let fn_lit = self.parse_fn_signature_and_body(start)?;
        Ok(Stmt {
            kind: StmtKind::FnDecl { name, fn_lit },
            span: start,
        })
    }

    // ---- if-construct: shared head, two callers with different
    // else-requirements (see §7/§11: strict chain once `else` appears) ----

    fn parse_if_head(&mut self) -> Result<(Span, Expr, Expr), ParseError> {
        let start = self.peek().span;
        self.expect(TokenKind::If, "'if'")?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        Ok((start, cond, then_branch))
    }

    fn parse_else_branch_strict(&mut self) -> Result<Expr, ParseError> {
        if self.check(&TokenKind::If) {
            self.parse_if_expr()
        } else {
            self.parse_block()
        }
    }

    /// `if` reached via `primary` (value position): `else` is required,
    /// and required recursively down any `else if` chain.
    fn parse_if_expr(&mut self) -> Result<Expr, ParseError> {
        let (start, cond, then_branch) = self.parse_if_head()?;
        self.expect(
            TokenKind::Else,
            "'else' (an if used as a value must have one, and every branch \
             of an else-if chain must end in else)",
        )?;
        let else_branch = self.parse_else_branch_strict()?;
        Ok(Expr {
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Some(Box::new(else_branch)),
            },
            span: start,
        })
    }

    /// `if` reached at statement position: `else` is optional. If present,
    /// the chain from that point on is strict (same as `parse_if_expr`).
    fn parse_if_stmt_head(&mut self) -> Result<(Expr, bool), ParseError> {
        let (start, cond, then_branch) = self.parse_if_head()?;
        if self.check(&TokenKind::Else) {
            self.advance();
            let else_branch = self.parse_else_branch_strict()?;
            Ok((
                Expr {
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(then_branch),
                        else_branch: Some(Box::new(else_branch)),
                    },
                    span: start,
                },
                true,
            ))
        } else {
            Ok((
                Expr {
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(then_branch),
                        else_branch: None,
                    },
                    span: start,
                },
                false,
            ))
        }
    }

    // ---- expression precedence chain (§11, levels not collapsed) ----

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_assign_expr()
    }

    fn parse_assign_expr(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_or_expr()?;
        if self.check(&TokenKind::Eq) {
            let is_place = matches!(lhs.kind, ExprKind::Ident(_) | ExprKind::Index { .. });
            if !is_place {
                return Err(ParseError {
                    message: "invalid assignment target: only a variable or an array index \
                              (e.g. `x`, `xs[i]`) may appear on the left of '='"
                        .to_string(),
                    span: lhs.span,
                });
            }
            self.advance(); // consume '='
            let value = self.parse_assign_expr()?; // right-associative
            let span = lhs.span;
            Ok(Expr {
                kind: ExprKind::Assign {
                    target: Box::new(lhs),
                    value: Box::new(value),
                },
                span,
            })
        } else {
            Ok(lhs)
        }
    }

    /// Shared left-associative binary-operator level: parse one operand
    /// via `sub`, then fold in `(op operand)*` for any of `ops` that
    /// match. Used for all six precedence levels between `or_expr` and
    /// `mul_expr` — genuinely repeated structure, not a premature
    /// abstraction.
    fn parse_left_assoc_binop(
        &mut self,
        sub: fn(&mut Self) -> Result<Expr, ParseError>,
        ops: &[(TokenKind, BinOp)],
    ) -> Result<Expr, ParseError> {
        let mut lhs = sub(self)?;
        loop {
            let matched = ops
                .iter()
                .find(|(tok, _)| self.check(tok))
                .map(|(_, op)| *op);
            match matched {
                Some(op) => {
                    self.advance();
                    let rhs = sub(self)?;
                    let span = lhs.span;
                    lhs = Expr {
                        kind: ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        span,
                    };
                }
                None => break,
            }
        }
        Ok(lhs)
    }

    fn parse_or_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(Self::parse_and_expr, &[(TokenKind::OrOr, BinOp::Or)])
    }

    fn parse_and_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(Self::parse_eq_expr, &[(TokenKind::AndAnd, BinOp::And)])
    }

    fn parse_eq_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(
            Self::parse_rel_expr,
            &[(TokenKind::EqEq, BinOp::Eq), (TokenKind::NotEq, BinOp::Ne)],
        )
    }

    fn parse_rel_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(
            Self::parse_add_expr,
            &[
                (TokenKind::Lt, BinOp::Lt),
                (TokenKind::LtEq, BinOp::Le),
                (TokenKind::Gt, BinOp::Gt),
                (TokenKind::GtEq, BinOp::Ge),
            ],
        )
    }

    fn parse_add_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(
            Self::parse_mul_expr,
            &[
                (TokenKind::Plus, BinOp::Add),
                (TokenKind::Minus, BinOp::Sub),
            ],
        )
    }

    fn parse_mul_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_left_assoc_binop(
            Self::parse_unary_expr,
            &[
                (TokenKind::Star, BinOp::Mul),
                (TokenKind::Slash, BinOp::Div),
                (TokenKind::Percent, BinOp::Mod),
            ],
        )
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek().span;
        if self.check(&TokenKind::Minus) {
            self.advance();
            let expr = self.parse_unary_expr()?;
            Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expr),
                },
                span: start,
            })
        } else if self.check(&TokenKind::Not) {
            self.advance();
            let expr = self.parse_unary_expr()?;
            Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Not,
                    expr: Box::new(expr),
                },
                span: start,
            })
        } else {
            self.parse_call_expr()
        }
    }

    fn parse_call_expr(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.check(&TokenKind::LParen) {
                self.advance();
                let args = self.parse_args(&TokenKind::RParen)?;
                self.expect(TokenKind::RParen, "')' to close argument list")?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                };
            } else if self.check(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket, "']' to close index expression")?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::Index {
                        array: Box::new(expr),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.check(&TokenKind::Dot) {
                let dot_span = self.peek().span;
                self.advance();
                let (method, _) = self.expect_ident("a method name after '.'")?;
                self.expect(TokenKind::LParen, "'(' to start method call arguments")?;
                let args = self.parse_args(&TokenKind::RParen)?;
                self.expect(TokenKind::RParen, "')' to close method call arguments")?;
                let span = expr.span;
                expr = Expr {
                    kind: ExprKind::MethodCall {
                        receiver: Box::new(expr),
                        method,
                        args,
                        span: dot_span,
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// Comma-separated expressions up to (not consuming) `terminator`.
    fn parse_args(&mut self, terminator: &TokenKind) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.check(terminator) {
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            if self.check(&TokenKind::Comma) {
                self.advance();
                continue;
            }
            break;
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.peek().span;
        match self.peek().kind.clone() {
            TokenKind::Int(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::IntLit(n),
                    span: start,
                })
            }
            TokenKind::Float(x) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::FloatLit(x),
                    span: start,
                })
            }
            TokenKind::Str(s) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::StringLit(s),
                    span: start,
                })
            }
            TokenKind::Bool(b) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLit(b),
                    span: start,
                })
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Ident(name),
                    span: start,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen, "')' to close parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let elems = self.parse_args(&TokenKind::RBracket)?;
                self.expect(TokenKind::RBracket, "']' to close array literal")?;
                Ok(Expr {
                    kind: ExprKind::ArrayLit(elems),
                    span: start,
                })
            }
            TokenKind::If => self.parse_if_expr(),
            TokenKind::Fn => {
                self.advance();
                let fn_lit = self.parse_fn_signature_and_body(start)?;
                Ok(Expr {
                    kind: ExprKind::FnLit(fn_lit),
                    span: start,
                })
            }
            TokenKind::LBrace => self.parse_block(),
            _ => Err(self.error_here(format!(
                "expected an expression, found {}",
                self.describe_current()
            ))),
        }
    }
}
