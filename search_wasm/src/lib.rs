/**
 * Copyright (c) 2024 Kuldeep Singh
 * Licensed under the MIT License
 *
 * A zero-copy WASM search engine for Salesforce Lightning Web Components (LWC)
 *
 * Features:
 * - Direct in-memory search over JSON array (no data copying during query)
 * - Full-text boolean search with AND, OR, NOT operators
 * - Query operators: =, !=, >, >=, <, <=, LIKE, FUZZY, REGEX, IN, NOT IN, BETWEEN
 * - Text matching: CONTAINS, STARTS WITH, ENDS WITH
 * - Built-in pagination with accurate total match count
 * - Adaptive result caching for fast repeat queries
 * - Designed for Salesforce LWC deployment via IIFE bundle
 *
 * Usage:
 *   const engineId = init_engine(jsonData);
 *   const result = search(engineId, 'country = "India" AND category = "software"');
 *   const paginated = search(engineId, 'name LIKE "%test%"', 10, 20);
 */
use js_sys::Date;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use strsim::damerau_levenshtein;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone)]
enum TokenKind {
    Select,
    Where,
    And,
    Or,
    Not,
    Fuzzy,
    In,
    Like,
    Regex,
    Order,
    By,
    Limit,
    Offset,
    Asc,
    Desc,
    Nulls,
    First,
    Last,
    Score,
    Case,
    Sensitive,
    Insensitive,
    Strict,
    Between,
    Contains,
    Starts,
    Ends,
    Exists,
    Is,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    LParen,
    RParen,
    Comma,
    Ident(String),
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    pos: usize,
}

#[derive(Debug, Clone)]
enum Expr {
    Or(Vec<Expr>),
    And(Vec<Expr>),
    Not(Box<Expr>),
    Term(String),
    FuzzyTerm(String),
    Predicate(Predicate),
    All,
}

#[derive(Debug, Clone)]
struct Query {
    expr: Expr,
    projection: Option<Vec<String>>,
    order_by: Vec<OrderBy>,
    limit: Option<usize>,
    offset: Option<usize>,
    case_sensitive: bool,
    strict: bool,
    score_needed: bool,
}

#[derive(Debug, Clone)]
struct OrderBy {
    field: String,
    desc: bool,
    nulls_first: Option<bool>,
}

#[derive(Debug, Clone)]
struct Predicate {
    field: String,
    op: Op,
    values: Vec<ValueLit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    In,
    NotIn,
    Like,
    NotLike,
    Regex,
    NotRegex,
    Fuzzy,
    NotFuzzy,
    Between,
    Contains,
    StartsWith,
    EndsWith,
    Exists,
    IsNull,
    IsNotNull,
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone)]
enum ValueLit {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Field(String),
    Regex(String),
    RegexCompiled(regex::Regex),
}

#[derive(Debug, Clone, Copy)]
struct EvalOptions {
    case_sensitive: bool,
    strict: bool,
}

#[derive(Debug)]
struct ParseError {
    message: String,
    pos: usize,
}

impl ParseError {
    fn new(message: impl Into<String>, pos: usize) -> Self {
        Self {
            message: message.into(),
            pos,
        }
    }
}

/// Convert the raw query string into a stream of tokens.
fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                tokens.push(Token {
                    kind: TokenKind::LParen,
                    pos: i,
                });
                i += 1;
            }
            ')' => {
                tokens.push(Token {
                    kind: TokenKind::RParen,
                    pos: i,
                });
                i += 1;
            }
            ',' => {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    pos: i,
                });
                i += 1;
            }
            '\'' | '"' => {
                let quote = c;
                let start = i;
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                        s.push(chars[i]);
                        i += 1;
                        continue;
                    }
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(ParseError::new("Unterminated string literal", start));
                }
                i += 1;
                tokens.push(Token {
                    kind: TokenKind::String(s),
                    pos: start,
                });
            }
            '=' => {
                tokens.push(Token {
                    kind: TokenKind::Eq,
                    pos: i,
                });
                i += 1;
            }
            '!' => {
                let start = i;
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Neq,
                        pos: start,
                    });
                    i += 2;
                } else {
                    return Err(ParseError::new("Unexpected character: !", start));
                }
            }
            '>' => {
                let start = i;
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Gte,
                        pos: start,
                    });
                    i += 2;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Gt,
                        pos: start,
                    });
                    i += 1;
                }
            }
            '<' => {
                let start = i;
                if i + 1 < chars.len() && chars[i + 1] == '=' {
                    tokens.push(Token {
                        kind: TokenKind::Lte,
                        pos: start,
                    });
                    i += 2;
                } else {
                    tokens.push(Token {
                        kind: TokenKind::Lt,
                        pos: start,
                    });
                    i += 1;
                }
            }
            _ => {
                if c.is_ascii_digit()
                    || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
                {
                    let start = i;
                    let mut s = String::new();
                    s.push(c);
                    i += 1;
                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                        s.push(chars[i]);
                        i += 1;
                    }
                    let num = s
                        .parse::<f64>()
                        .map_err(|_| ParseError::new("Invalid number", start))?;
                    tokens.push(Token {
                        kind: TokenKind::Number(num),
                        pos: start,
                    });
                } else if c.is_ascii_alphanumeric() || c == '_' {
                    let start = i;
                    let mut s = String::new();
                    s.push(c);
                    i += 1;
                    while i < chars.len()
                        && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                    {
                        s.push(chars[i]);
                        i += 1;
                    }
                    let upper = s.to_ascii_uppercase();
                    let kind = match upper.as_str() {
                        "SELECT" => TokenKind::Select,
                        "WHERE" => TokenKind::Where,
                        "AND" => TokenKind::And,
                        "OR" => TokenKind::Or,
                        "NOT" => TokenKind::Not,
                        "FUZZY" => TokenKind::Fuzzy,
                        "IN" => TokenKind::In,
                        "LIKE" => TokenKind::Like,
                        "REGEX" => TokenKind::Regex,
                        "ORDER" => TokenKind::Order,
                        "BY" => TokenKind::By,
                        "LIMIT" => TokenKind::Limit,
                        "OFFSET" => TokenKind::Offset,
                        "ASC" => TokenKind::Asc,
                        "DESC" => TokenKind::Desc,
                        "NULLS" => TokenKind::Nulls,
                        "FIRST" => TokenKind::First,
                        "LAST" => TokenKind::Last,
                        "SCORE" => TokenKind::Score,
                        "CASE" => TokenKind::Case,
                        "SENSITIVE" => TokenKind::Sensitive,
                        "INSENSITIVE" => TokenKind::Insensitive,
                        "STRICT" => TokenKind::Strict,
                        "BETWEEN" => TokenKind::Between,
                        "CONTAINS" => TokenKind::Contains,
                        "STARTS" => TokenKind::Starts,
                        "ENDS" => TokenKind::Ends,
                        "EXISTS" => TokenKind::Exists,
                        "IS" => TokenKind::Is,
                        "TRUE" => TokenKind::Bool(true),
                        "FALSE" => TokenKind::Bool(false),
                        "NULL" => TokenKind::Null,
                        _ => TokenKind::Ident(s),
                    };
                    tokens.push(Token { kind, pos: start });
                } else {
                    return Err(ParseError::new(format!("Unexpected character: {}", c), i));
                }
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        pos: input.len(),
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    /// Create a parser over a token stream.
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            depth: 0,
        }
    }

    /// Peek at the current token without consuming it.
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    /// Consume and return the current token.
    fn next(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        self.pos += 1;
        tok
    }

    /// Expect a token of a given kind (by discriminant).
    fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<(), ParseError> {
        let tok = self.peek();
        if std::mem::discriminant(&tok.kind) == std::mem::discriminant(kind) {
            self.next();
            Ok(())
        } else {
            Err(ParseError::new(msg, tok.pos))
        }
    }

    /// Parse full expression (OR-precedence).
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    /// Parse OR chains.
    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut parts = vec![self.parse_and()?];
        while matches!(self.peek().kind, TokenKind::Or) {
            self.next();
            parts.push(self.parse_and()?);
        }
        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(Expr::Or(parts))
        }
    }

    /// Parse AND chains.
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut parts = vec![self.parse_not()?];
        while matches!(self.peek().kind, TokenKind::And) {
            self.next();
            parts.push(self.parse_not()?);
        }
        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(Expr::And(parts))
        }
    }

    /// Parse NOT prefix.
    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek().kind, TokenKind::Not) {
            self.next();
            self.enter_depth()?;
            let expr = self.parse_not()?;
            self.exit_depth();
            Ok(Expr::Not(Box::new(expr)))
        } else {
            self.parse_primary()
        }
    }

    /// Parse parentheses, predicates, or free terms.
    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match &self.peek().kind {
            TokenKind::LParen => {
                self.next();
                self.enter_depth()?;
                let expr = self.parse_expr()?;
                self.exit_depth();
                self.expect(&TokenKind::RParen, "Expected ')'")?;
                Ok(expr)
            }
            TokenKind::Eof => Ok(Expr::All),
            TokenKind::Ident(_) => {
                if self.is_predicate_start() {
                    let pred = self.parse_predicate()?;
                    Ok(Expr::Predicate(pred))
                } else {
                    let term = self.parse_term()?;
                    Ok(Expr::Term(term))
                }
            }
            TokenKind::Fuzzy => {
                self.next();
                let term = self.parse_term()?;
                Ok(Expr::FuzzyTerm(term))
            }
            TokenKind::String(_) => {
                let term = self.parse_term()?;
                Ok(Expr::Term(term))
            }
            _ => {
                let tok = self.peek();
                Err(ParseError::new("Expected term, predicate, or '('", tok.pos))
            }
        }
    }

    /// Lookahead to decide whether an identifier starts a predicate.
    fn is_predicate_start(&self) -> bool {
        if let TokenKind::Ident(_) = self.peek().kind {
            if let Some(next) = self.tokens.get(self.pos + 1) {
                matches!(
                    next.kind,
                    TokenKind::In
                        | TokenKind::Like
                        | TokenKind::Regex
                        | TokenKind::Fuzzy
                        | TokenKind::Not
                        | TokenKind::Between
                        | TokenKind::Contains
                        | TokenKind::Starts
                        | TokenKind::Ends
                        | TokenKind::Exists
                        | TokenKind::Is
                        | TokenKind::Eq
                        | TokenKind::Neq
                        | TokenKind::Gt
                        | TokenKind::Gte
                        | TokenKind::Lt
                        | TokenKind::Lte
                )
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Parse a free-text term.
    fn parse_term(&mut self) -> Result<String, ParseError> {
        let tok = self.next();
        match &tok.kind {
            TokenKind::Ident(s) => Ok(s.clone()),
            TokenKind::String(s) => Ok(s.clone()),
            _ => Err(ParseError::new("Expected term", tok.pos)),
        }
    }

    fn enter_depth(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(ParseError::new(
                format!("Max nesting depth exceeded ({})", MAX_NESTING_DEPTH),
                self.peek().pos,
            ));
        }
        Ok(())
    }

    fn exit_depth(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// Parse a predicate like `field OP value`.
    fn parse_predicate(&mut self) -> Result<Predicate, ParseError> {
        let field_tok = self.next();
        let field = match &field_tok.kind {
            TokenKind::Ident(s) => s.clone(),
            _ => return Err(ParseError::new("Expected field name", field_tok.pos)),
        };
        if !is_valid_field_name(&field) {
            return Err(ParseError::new("Invalid field name", field_tok.pos));
        }

        let op = match &self.peek().kind {
            TokenKind::In => {
                self.next();
                Op::In
            }
            TokenKind::Like => {
                self.next();
                Op::Like
            }
            TokenKind::Regex => {
                self.next();
                Op::Regex
            }
            TokenKind::Fuzzy => {
                self.next();
                Op::Fuzzy
            }
            TokenKind::Between => {
                self.next();
                Op::Between
            }
            TokenKind::Contains => {
                self.next();
                Op::Contains
            }
            TokenKind::Starts => {
                self.next();
                self.expect_with_keyword("WITH", "Expected WITH after STARTS")?;
                Op::StartsWith
            }
            TokenKind::Ends => {
                self.next();
                self.expect_with_keyword("WITH", "Expected WITH after ENDS")?;
                Op::EndsWith
            }
            TokenKind::Exists => {
                self.next();
                Op::Exists
            }
            TokenKind::Is => {
                self.next();
                if matches!(self.peek().kind, TokenKind::Not) {
                    self.next();
                    self.expect(&TokenKind::Null, "Expected NULL after IS NOT")?;
                    Op::IsNotNull
                } else {
                    self.expect(&TokenKind::Null, "Expected NULL after IS")?;
                    Op::IsNull
                }
            }
            TokenKind::Eq => {
                self.next();
                Op::Eq
            }
            TokenKind::Neq => {
                self.next();
                Op::Neq
            }
            TokenKind::Gt => {
                self.next();
                Op::Gt
            }
            TokenKind::Gte => {
                self.next();
                Op::Gte
            }
            TokenKind::Lt => {
                self.next();
                Op::Lt
            }
            TokenKind::Lte => {
                self.next();
                Op::Lte
            }
            TokenKind::Not => {
                self.next();
                if matches!(self.peek().kind, TokenKind::Like) {
                    self.next();
                    Op::NotLike
                } else if matches!(self.peek().kind, TokenKind::Regex) {
                    self.next();
                    Op::NotRegex
                } else if matches!(self.peek().kind, TokenKind::Fuzzy) {
                    self.next();
                    Op::NotFuzzy
                } else {
                    self.expect(&TokenKind::In, "Expected IN after NOT")?;
                    Op::NotIn
                }
            }
            _ => {
                let tok = self.peek();
                return Err(ParseError::new("Expected operator", tok.pos));
            }
        };

        let values = match op {
            Op::Like => vec![self.parse_value()?],
            Op::NotLike => vec![self.parse_value()?],
            Op::Regex | Op::NotRegex => {
                let lit = self.parse_value()?;
                let pattern = match lit {
                    ValueLit::Str(s) => s,
                    _ => {
                        return Err(ParseError::new(
                            "REGEX expects a string literal",
                            self.peek().pos,
                        ))
                    }
                };
                validate_regex_pattern(&pattern, self.peek().pos)?;
                vec![ValueLit::Regex(pattern)]
            }
            Op::Between => self.parse_between()?,
            Op::Contains | Op::StartsWith | Op::EndsWith | Op::Fuzzy | Op::NotFuzzy => {
                vec![self.parse_value()?]
            }
            Op::Exists | Op::IsNull | Op::IsNotNull => Vec::new(),
            Op::Eq | Op::Neq | Op::Gt | Op::Gte | Op::Lt | Op::Lte => {
                vec![self.parse_value_fieldable()?]
            }
            Op::In | Op::NotIn => self.parse_list()?,
        };

        Ok(Predicate { field, op, values })
    }

    /// Parse an IN list: `(a, b, c)`.
    fn parse_list(&mut self) -> Result<Vec<ValueLit>, ParseError> {
        self.expect(&TokenKind::LParen, "Expected '(' after IN")?;
        let mut values = Vec::new();
        if matches!(self.peek().kind, TokenKind::RParen) {
            self.next();
            return Ok(values);
        }
        loop {
            values.push(self.parse_value()?);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.next();
                continue;
            }
            if matches!(self.peek().kind, TokenKind::RParen) {
                self.next();
                break;
            }
            let tok = self.peek();
            return Err(ParseError::new("Expected ',' or ')'", tok.pos));
        }
        Ok(values)
    }

    /// Parse projection fields after SELECT.
    fn parse_projection_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut fields = Vec::new();
        loop {
            let field = match &self.peek().kind {
                TokenKind::Ident(s) => {
                    let f = s.clone();
                    self.next();
                    f
                }
                TokenKind::Score => {
                    self.next();
                    "SCORE".to_string()
                }
                _ => return Err(ParseError::new("Expected field in SELECT", self.peek().pos)),
            };
            fields.push(field);
            if matches!(self.peek().kind, TokenKind::Comma) {
                self.next();
                continue;
            }
            break;
        }
        Ok(fields)
    }

    /// Expect an identifier with a specific keyword value.
    fn expect_with_keyword(&mut self, expected: &str, msg: &str) -> Result<(), ParseError> {
        let tok = self.peek();
        match &tok.kind {
            TokenKind::Ident(s) if s.eq_ignore_ascii_case(expected) => {
                self.next();
                Ok(())
            }
            _ => Err(ParseError::new(msg, tok.pos)),
        }
    }

    /// Parse BETWEEN with either `(a, b)` or `a AND b`.
    fn parse_between(&mut self) -> Result<Vec<ValueLit>, ParseError> {
        if matches!(self.peek().kind, TokenKind::LParen) {
            self.next();
            let first = self.parse_value_fieldable()?;
            self.expect(&TokenKind::Comma, "Expected ',' in BETWEEN")?;
            let second = self.parse_value_fieldable()?;
            self.expect(&TokenKind::RParen, "Expected ')' after BETWEEN")?;
            return Ok(vec![first, second]);
        }
        let first = self.parse_value_fieldable()?;
        self.expect(&TokenKind::And, "Expected AND in BETWEEN")?;
        let second = self.parse_value_fieldable()?;
        Ok(vec![first, second])
    }

    /// Parse a literal value.
    fn parse_value(&mut self) -> Result<ValueLit, ParseError> {
        let tok = self.next();
        match &tok.kind {
            TokenKind::String(s) => Ok(ValueLit::Str(s.clone())),
            TokenKind::Ident(s) => Ok(ValueLit::Str(s.clone())),
            TokenKind::Number(n) => Ok(ValueLit::Num(*n)),
            TokenKind::Bool(b) => Ok(ValueLit::Bool(*b)),
            TokenKind::Null => Ok(ValueLit::Null),
            _ => Err(ParseError::new("Expected value", tok.pos)),
        }
    }

    /// Parse a value that can also be a field reference.
    fn parse_value_fieldable(&mut self) -> Result<ValueLit, ParseError> {
        let tok = self.next();
        match &tok.kind {
            TokenKind::String(s) => Ok(ValueLit::Str(s.clone())),
            TokenKind::Ident(s) => Ok(ValueLit::Field(s.clone())),
            TokenKind::Number(n) => Ok(ValueLit::Num(*n)),
            TokenKind::Bool(b) => Ok(ValueLit::Bool(*b)),
            TokenKind::Null => Ok(ValueLit::Null),
            _ => Err(ParseError::new("Expected value", tok.pos)),
        }
    }
}

/// Parse a query string into a full Query (filters, ordering, flags, projection).
fn parse_query(query: &str) -> Result<Query, ParseError> {
    if query.len() > MAX_QUERY_LEN {
        return Err(ParseError::new(
            format!("Query length exceeds {} characters", MAX_QUERY_LEN),
            0,
        ));
    }
    let tokens = tokenize(query)?;
    let mut parser = Parser::new(tokens);
    let mut projection: Option<Vec<String>> = None;
    let expr = if matches!(parser.peek().kind, TokenKind::Select) {
        parser.next();
        projection = Some(parser.parse_projection_list()?);
        if matches!(parser.peek().kind, TokenKind::Where) {
            parser.next();
            parser.parse_expr()?
        } else {
            Expr::All
        }
    } else if matches!(
        parser.peek().kind,
        TokenKind::Order
            | TokenKind::Limit
            | TokenKind::Offset
            | TokenKind::Case
            | TokenKind::Strict
            | TokenKind::Eof
    ) {
        Expr::All
    } else {
        parser.parse_expr()?
    };

    let mut order_by: Vec<OrderBy> = Vec::new();
    let mut limit: Option<usize> = None;
    let mut offset: Option<usize> = None;
    let mut case_sensitive = false;
    let mut strict = false;

    loop {
        match &parser.peek().kind {
            TokenKind::Case => {
                parser.next();
                match &parser.peek().kind {
                    TokenKind::Sensitive => {
                        parser.next();
                        case_sensitive = true;
                    }
                    TokenKind::Insensitive => {
                        parser.next();
                        case_sensitive = false;
                    }
                    _ => {
                        return Err(ParseError::new(
                            "Expected SENSITIVE or INSENSITIVE after CASE",
                            parser.peek().pos,
                        ));
                    }
                }
            }
            TokenKind::Strict => {
                parser.next();
                strict = true;
            }
            TokenKind::Order => {
                parser.next();
                parser.expect(&TokenKind::By, "Expected BY after ORDER")?;
                loop {
                    let field = match &parser.peek().kind {
                        TokenKind::Ident(s) => {
                            let f = s.clone();
                            parser.next();
                            f
                        }
                        TokenKind::Score => {
                            parser.next();
                            "SCORE".to_string()
                        }
                        _ => {
                            return Err(ParseError::new(
                                "Expected field after ORDER BY",
                                parser.peek().pos,
                            ))
                        }
                    };
                    let desc = match &parser.peek().kind {
                        TokenKind::Desc => {
                            parser.next();
                            true
                        }
                        TokenKind::Asc => {
                            parser.next();
                            false
                        }
                        _ => false,
                    };
                    let mut nulls_first: Option<bool> = None;
                    if matches!(parser.peek().kind, TokenKind::Nulls) {
                        parser.next();
                        match &parser.peek().kind {
                            TokenKind::First => {
                                parser.next();
                                nulls_first = Some(true);
                            }
                            TokenKind::Last => {
                                parser.next();
                                nulls_first = Some(false);
                            }
                            _ => {
                                return Err(ParseError::new(
                                    "Expected FIRST or LAST after NULLS",
                                    parser.peek().pos,
                                ));
                            }
                        }
                    }
                    order_by.push(OrderBy {
                        field,
                        desc,
                        nulls_first,
                    });
                    if matches!(parser.peek().kind, TokenKind::Comma) {
                        parser.next();
                        continue;
                    }
                    break;
                }
            }
            TokenKind::Limit => {
                parser.next();
                let val = match &parser.peek().kind {
                    TokenKind::Number(n) => {
                        let v = *n;
                        parser.next();
                        v
                    }
                    _ => {
                        return Err(ParseError::new(
                            "Expected number after LIMIT",
                            parser.peek().pos,
                        ))
                    }
                };
                if val < 0.0 {
                    return Err(ParseError::new(
                        "LIMIT must be non-negative",
                        parser.peek().pos,
                    ));
                }
                limit = Some(val as usize);
            }
            TokenKind::Offset => {
                parser.next();
                let val = match &parser.peek().kind {
                    TokenKind::Number(n) => {
                        let v = *n;
                        parser.next();
                        v
                    }
                    _ => {
                        return Err(ParseError::new(
                            "Expected number after OFFSET",
                            parser.peek().pos,
                        ))
                    }
                };
                if val < 0.0 {
                    return Err(ParseError::new(
                        "OFFSET must be non-negative",
                        parser.peek().pos,
                    ));
                }
                offset = Some(val as usize);
            }
            TokenKind::Eof => break,
            _ => {
                return Err(ParseError::new(
                    "Unexpected trailing input",
                    parser.peek().pos,
                ));
            }
        }
    }
    let mut expr = expr;
    compile_regexes(&mut expr, case_sensitive)?;
    let projection_has_score = projection
        .as_ref()
        .map(|p| p.iter().any(|f| f.eq_ignore_ascii_case("SCORE")))
        .unwrap_or(false);
    let order_has_score = order_by
        .iter()
        .any(|o| o.field.eq_ignore_ascii_case("SCORE"));
    let score_needed = projection_has_score || order_has_score;

    Ok(Query {
        expr,
        projection,
        order_by,
        limit,
        offset,
        case_sensitive,
        strict,
        score_needed,
    })
}

/// Compile REGEX patterns inside the AST for fast execution.
fn compile_regexes(expr: &mut Expr, case_sensitive: bool) -> Result<(), ParseError> {
    match expr {
        Expr::Or(parts) | Expr::And(parts) => {
            for part in parts.iter_mut() {
                compile_regexes(part, case_sensitive)?;
            }
        }
        Expr::Not(inner) => compile_regexes(inner, case_sensitive)?,
        Expr::Predicate(pred) => {
            if matches!(pred.op, Op::Regex | Op::NotRegex) {
                if let Some(first) = pred.values.get_mut(0) {
                    if let ValueLit::Regex(pattern) = first {
                        validate_regex_pattern(pattern, 0)?;
                        let mut builder = regex::RegexBuilder::new(pattern);
                        builder.case_insensitive(!case_sensitive);
                        let compiled = builder
                            .build()
                            .map_err(|_| ParseError::new("Invalid REGEX pattern", 0))?;
                        *first = ValueLit::RegexCompiled(compiled);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Normalize whitespace (outside quotes) for query cache keys.
fn normalize_query(query: &str) -> String {
    let mut out = String::new();
    let mut last_space = false;
    let mut in_quote: Option<char> = None;
    for ch in query.chars() {
        if let Some(q) = in_quote {
            out.push(ch);
            if ch == q {
                in_quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
            out.push(ch);
            last_space = false;
            continue;
        }
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

fn is_valid_field_name(field: &str) -> bool {
    if field.is_empty() {
        return false;
    }
    field
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn validate_regex_pattern(pattern: &str, pos: usize) -> Result<(), ParseError> {
    if pattern.len() > MAX_REGEX_LEN {
        return Err(ParseError::new(
            format!("REGEX pattern too long (max {})", MAX_REGEX_LEN),
            pos,
        ));
    }
    let lowered = pattern.to_string();
    let banned = [")+", ")*", "){", ")+?", ")*?"];
    for b in banned.iter() {
        if lowered.contains(b) {
            return Err(ParseError::new(
                "REGEX pattern rejected (potential ReDoS)",
                pos,
            ));
        }
    }
    if lowered.contains("++") || lowered.contains("**") || lowered.contains("{,") {
        return Err(ParseError::new(
            "REGEX pattern rejected (potential ReDoS)",
            pos,
        ));
    }
    Ok(())
}

/// Parse a query with LRU caching of the AST.
fn parse_query_cached(query: &str) -> Result<Query, ParseError> {
    let key = normalize_query(query);
    QUERY_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(found) = cache.get(&key) {
            return Ok(found);
        }
        let parsed = parse_query(&key)?;
        cache.put(key, parsed.clone());
        Ok(parsed)
    })
}

#[derive(Debug, Clone)]
struct ColumnarView {
    fields: HashMap<String, Vec<Value>>,
}

enum ColumnarRef<'a> {
    Borrowed(&'a ColumnarView),
    Owned(ColumnarView),
    None,
}

impl<'a> ColumnarRef<'a> {
    fn as_ref(&'a self) -> Option<&'a ColumnarView> {
        match self {
            ColumnarRef::Borrowed(v) => Some(*v),
            ColumnarRef::Owned(v) => Some(v),
            ColumnarRef::None => None,
        }
    }
}

fn get_path_with_columnar<'a>(
    item: &'a Value,
    field: &str,
    columnar: Option<&'a ColumnarView>,
    idx: usize,
) -> Option<&'a Value> {
    if let Some(view) = columnar {
        if let Some(col) = view.fields.get(field) {
            return col.get(idx);
        }
    }
    get_path(item, field)
}

fn collect_expr_fields(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::And(parts) | Expr::Or(parts) => {
            for p in parts {
                collect_expr_fields(p, out);
            }
        }
        Expr::Not(inner) => collect_expr_fields(inner, out),
        Expr::Predicate(pred) => {
            if !out.iter().any(|f| f == &pred.field) {
                out.push(pred.field.clone());
            }
            for v in pred.values.iter() {
                if let ValueLit::Field(f) = v {
                    if !out.iter().any(|x| x == f) {
                        out.push(f.clone());
                    }
                }
            }
        }
        _ => {}
    }
}

fn build_columnar_view(items: &[Value], fields: &[String]) -> ColumnarView {
    let mut map: HashMap<String, Vec<Value>> = HashMap::new();
    for field in fields.iter() {
        let mut col = Vec::with_capacity(items.len());
        for item in items.iter() {
            let value = get_path(item, field).cloned().unwrap_or(Value::Null);
            col.push(value);
        }
        map.insert(field.clone(), col);
    }
    ColumnarView { fields: map }
}

/// Evaluate an expression against a single JSON record.
fn eval_expr(
    expr: &Expr,
    item: &Value,
    options: EvalOptions,
    columnar: Option<&ColumnarView>,
    idx: usize,
) -> bool {
    match expr {
        Expr::Or(parts) => parts
            .iter()
            .any(|e| eval_expr(e, item, options, columnar, idx)),
        Expr::And(parts) => parts
            .iter()
            .all(|e| eval_expr(e, item, options, columnar, idx)),
        Expr::Not(inner) => !eval_expr(inner, item, options, columnar, idx),
        Expr::Term(term) => contains_text(item, term, options),
        Expr::FuzzyTerm(term) => fuzzy_contains_text(item, term, options),
        Expr::Predicate(pred) => eval_predicate(pred, item, options, columnar, idx),
        Expr::All => true,
    }
}

/// Evaluate a single predicate (field op value) against one record.
fn eval_predicate(
    pred: &Predicate,
    item: &Value,
    options: EvalOptions,
    columnar: Option<&ColumnarView>,
    idx: usize,
) -> bool {
    let target = get_path_with_columnar(item, &pred.field, columnar, idx);
    if target.is_none() {
        return matches!(pred.op, Op::IsNull);
    }
    let target = target.unwrap();
    match pred.op {
        Op::Like => {
            if let Some(ValueLit::Str(pattern)) = pred.values.get(0) {
                match target {
                    Value::String(s) => like_match(s, pattern, options.case_sensitive),
                    _ => false,
                }
            } else {
                false
            }
        }
        Op::NotLike => {
            if let Some(ValueLit::Str(pattern)) = pred.values.get(0) {
                match target {
                    Value::String(s) => !like_match(s, pattern, options.case_sensitive),
                    _ => false,
                }
            } else {
                false
            }
        }
        Op::Regex => {
            if let Some(ValueLit::RegexCompiled(re)) = pred.values.get(0) {
                match target {
                    Value::String(s) => re.is_match(s),
                    _ => false,
                }
            } else {
                false
            }
        }
        Op::NotRegex => {
            if let Some(ValueLit::RegexCompiled(re)) = pred.values.get(0) {
                match target {
                    Value::String(s) => !re.is_match(s),
                    _ => false,
                }
            } else {
                false
            }
        }
        Op::In => matches_in(target, &pred.values, item, options),
        Op::NotIn => !matches_in(target, &pred.values, item, options),
        Op::Between => between_match(target, &pred.values, item, options),
        Op::Contains => contains_match(target, pred.values.get(0), item, options),
        Op::Fuzzy => fuzzy_match(target, pred.values.get(0), item, options),
        Op::NotFuzzy => !fuzzy_match(target, pred.values.get(0), item, options),
        Op::StartsWith => starts_with_match(target, pred.values.get(0), options.case_sensitive),
        Op::EndsWith => ends_with_match(target, pred.values.get(0), options.case_sensitive),
        Op::Exists => true,
        Op::IsNull => matches!(target, Value::Null),
        Op::IsNotNull => !matches!(target, Value::Null),
        Op::Eq => compare_any(target, pred.values.get(0), CmpOp::Eq, item, options),
        Op::Neq => compare_any(target, pred.values.get(0), CmpOp::Neq, item, options),
        Op::Gt => compare_any(target, pred.values.get(0), CmpOp::Gt, item, options),
        Op::Gte => compare_any(target, pred.values.get(0), CmpOp::Gte, item, options),
        Op::Lt => compare_any(target, pred.values.get(0), CmpOp::Lt, item, options),
        Op::Lte => compare_any(target, pred.values.get(0), CmpOp::Lte, item, options),
    }
}

/// IN/NOT IN evaluation with array support.
fn matches_in(target: &Value, values: &[ValueLit], item: &Value, options: EvalOptions) -> bool {
    match target {
        Value::Array(arr) => arr.iter().any(|v| value_in_list(v, values, item, options)),
        _ => value_in_list(target, values, item, options),
    }
}

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
}

/// Compare a field value to a literal/field with numeric ops.
fn compare_any(
    target: &Value,
    value: Option<&ValueLit>,
    op: CmpOp,
    item: &Value,
    options: EvalOptions,
) -> bool {
    let value = match value {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::Array(arr) => arr
            .iter()
            .any(|v| compare_value(v, value, op, item, options)),
        _ => compare_value(target, value, op, item, options),
    }
}

/// Compare a single target value to a literal/field.
fn compare_value(
    target: &Value,
    value: &ValueLit,
    op: CmpOp,
    item: &Value,
    options: EvalOptions,
) -> bool {
    match op {
        CmpOp::Eq => value_matches(target, value, item, options),
        CmpOp::Neq => !value_matches(target, value, item, options),
        CmpOp::Gt | CmpOp::Gte | CmpOp::Lt | CmpOp::Lte => {
            let left = match target {
                Value::Number(n) => n.as_f64(),
                _ => None,
            };
            let right = resolve_number(value, item, options);
            match (left, right) {
                (Some(l), Some(r)) => match op {
                    CmpOp::Gt => l > r,
                    CmpOp::Gte => l >= r,
                    CmpOp::Lt => l < r,
                    CmpOp::Lte => l <= r,
                    _ => false,
                },
                _ => false,
            }
        }
    }
}

/// Check if a target value matches any value in a list.
fn value_in_list(target: &Value, values: &[ValueLit], item: &Value, options: EvalOptions) -> bool {
    values
        .iter()
        .any(|v| value_matches(target, v, item, options))
}

/// Evaluate BETWEEN on a numeric field (supports field-to-field bounds).
fn between_match(target: &Value, values: &[ValueLit], item: &Value, options: EvalOptions) -> bool {
    if values.len() != 2 {
        return false;
    }
    let low = match resolve_number(&values[0], item, options) {
        Some(v) => v,
        None => return false,
    };
    let high = match resolve_number(&values[1], item, options) {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::Number(n) => n.as_f64().map(|x| x >= low && x <= high).unwrap_or(false),
        Value::Array(arr) => arr.iter().any(|v| between_match(v, values, item, options)),
        _ => false,
    }
}

/// CONTAINS evaluation for strings/arrays.
fn contains_match(
    target: &Value,
    value: Option<&ValueLit>,
    item: &Value,
    options: EvalOptions,
) -> bool {
    let value = match value {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::String(s) => match value {
            ValueLit::Str(v) => {
                if options.case_sensitive {
                    s.contains(v)
                } else {
                    s.to_ascii_lowercase().contains(&v.to_ascii_lowercase())
                }
            }
            _ => false,
        },
        Value::Array(arr) => arr.iter().any(|v| value_matches(v, value, item, options)),
        _ => false,
    }
}

/// STARTS WITH evaluation for strings.
fn starts_with_match(target: &Value, value: Option<&ValueLit>, case_sensitive: bool) -> bool {
    let value = match value {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::String(s) => match value {
            ValueLit::Str(v) => {
                if case_sensitive {
                    s.starts_with(v)
                } else {
                    s.to_ascii_lowercase().starts_with(&v.to_ascii_lowercase())
                }
            }
            _ => false,
        },
        _ => false,
    }
}

/// ENDS WITH evaluation for strings.
fn ends_with_match(target: &Value, value: Option<&ValueLit>, case_sensitive: bool) -> bool {
    let value = match value {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::String(s) => match value {
            ValueLit::Str(v) => {
                if case_sensitive {
                    s.ends_with(v)
                } else {
                    s.to_ascii_lowercase().ends_with(&v.to_ascii_lowercase())
                }
            }
            _ => false,
        },
        _ => false,
    }
}

/// Equality match with optional field-to-field comparison.
fn value_matches(target: &Value, value: &ValueLit, item: &Value, options: EvalOptions) -> bool {
    match value {
        ValueLit::Field(field) => {
            if let Some(other) = get_path(item, field) {
                return compare_json_values(target, other, options);
            }
            false
        }
        ValueLit::Str(v) => match target {
            Value::String(s) => {
                if options.case_sensitive {
                    s == v
                } else {
                    s.eq_ignore_ascii_case(v)
                }
            }
            Value::Number(n) if !options.strict => v
                .parse::<f64>()
                .ok()
                .zip(n.as_f64())
                .map(|(a, b)| a == b)
                .unwrap_or(false),
            _ => false,
        },
        ValueLit::Num(v) => match target {
            Value::Number(n) => n.as_f64().map(|x| x == *v).unwrap_or(false),
            Value::String(s) if !options.strict => {
                s.parse::<f64>().ok().map(|x| x == *v).unwrap_or(false)
            }
            _ => false,
        },
        ValueLit::Bool(v) => matches!(target, Value::Bool(b) if b == v),
        ValueLit::Null => matches!(target, Value::Null),
        ValueLit::Regex(_) | ValueLit::RegexCompiled(_) => false,
    }
}

/// Resolve a numeric literal or field reference to a number.
fn resolve_number(value: &ValueLit, item: &Value, options: EvalOptions) -> Option<f64> {
    match value {
        ValueLit::Num(n) => Some(*n),
        ValueLit::Field(field) => {
            let v = get_path(item, field)?;
            match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) if !options.strict => s.parse::<f64>().ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Compare two JSON values with strict/case rules.
fn compare_json_values(a: &Value, b: &Value, options: EvalOptions) -> bool {
    match (a, b) {
        (Value::String(x), Value::String(y)) => {
            if options.case_sensitive {
                x == y
            } else {
                x.eq_ignore_ascii_case(y)
            }
        }
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        (Value::String(x), Value::Number(y)) if !options.strict => x
            .parse::<f64>()
            .ok()
            .zip(y.as_f64())
            .map(|(a, b)| a == b)
            .unwrap_or(false),
        (Value::Number(x), Value::String(y)) if !options.strict => y
            .parse::<f64>()
            .ok()
            .zip(x.as_f64())
            .map(|(a, b)| a == b)
            .unwrap_or(false),
        _ => false,
    }
}

/// LIKE matcher with % and _ wildcards.
fn like_match(text: &str, pattern: &str, case_sensitive: bool) -> bool {
    let (text, pattern) = if case_sensitive {
        (text.to_string(), pattern.to_string())
    } else {
        (text.to_ascii_lowercase(), pattern.to_ascii_lowercase())
    };
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    let mut dp = vec![vec![false; p.len() + 1]; t.len() + 1];
    dp[0][0] = true;
    for j in 1..=p.len() {
        if p[j - 1] == '%' {
            dp[0][j] = dp[0][j - 1];
        }
    }
    for i in 1..=t.len() {
        for j in 1..=p.len() {
            match p[j - 1] {
                '%' => dp[i][j] = dp[i][j - 1] || dp[i - 1][j],
                '_' => dp[i][j] = dp[i - 1][j - 1],
                c => dp[i][j] = dp[i - 1][j - 1] && t[i - 1] == c,
            }
        }
    }
    dp[t.len()][p.len()]
}

/// Full record text search for simple terms.
fn contains_text(value: &Value, term: &str, options: EvalOptions) -> bool {
    match value {
        Value::String(s) => {
            if options.case_sensitive {
                s.contains(term)
            } else {
                s.to_ascii_lowercase().contains(&term.to_ascii_lowercase())
            }
        }
        Value::Number(n) => {
            if options.case_sensitive {
                n.to_string().contains(term)
            } else {
                n.to_string()
                    .to_ascii_lowercase()
                    .contains(&term.to_ascii_lowercase())
            }
        }
        Value::Bool(b) => {
            if options.case_sensitive {
                b.to_string().contains(term)
            } else {
                b.to_string()
                    .to_ascii_lowercase()
                    .contains(&term.to_ascii_lowercase())
            }
        }
        Value::Array(arr) => arr.iter().any(|v| contains_text(v, term, options)),
        Value::Object(map) => map.iter().any(|(k, v)| {
            if options.case_sensitive {
                k.contains(term) || contains_text(v, term, options)
            } else {
                k.to_ascii_lowercase().contains(&term.to_ascii_lowercase())
                    || contains_text(v, term, options)
            }
        }),
        Value::Null => false,
    }
}

/// FUZZY match on a field value (string/array).
fn fuzzy_match(
    target: &Value,
    value: Option<&ValueLit>,
    item: &Value,
    options: EvalOptions,
) -> bool {
    let value = match value {
        Some(v) => v,
        None => return false,
    };
    match target {
        Value::String(s) => fuzzy_match_text_value(s, value, options),
        Value::Array(arr) => arr
            .iter()
            .any(|v| fuzzy_match(v, Some(value), item, options)),
        _ => false,
    }
}

/// FUZZY match over a full record (term search).
fn fuzzy_contains_text(value: &Value, term: &str, options: EvalOptions) -> bool {
    match value {
        Value::String(s) => fuzzy_match_text_str(s, term, options),
        Value::Number(n) => fuzzy_match_text_str(&n.to_string(), term, options),
        Value::Bool(b) => fuzzy_match_text_str(&b.to_string(), term, options),
        Value::Array(arr) => arr.iter().any(|v| fuzzy_contains_text(v, term, options)),
        Value::Object(map) => map.iter().any(|(k, v)| {
            fuzzy_match_text_str(k, term, options) || fuzzy_contains_text(v, term, options)
        }),
        Value::Null => false,
    }
}

fn fuzzy_match_text_value(text: &str, lit: &ValueLit, options: EvalOptions) -> bool {
    match lit {
        ValueLit::Str(s) => fuzzy_match_text_str(text, s, options),
        _ => false,
    }
}

fn fuzzy_match_text_str(text: &str, term: &str, options: EvalOptions) -> bool {
    let t = normalize_match_text(term, options);
    if t.is_empty() {
        return false;
    }
    let c = normalize_match_text(text, options);
    if c.contains(&t) {
        return true;
    }
    for token in tokenize_text(&c) {
        if fuzzy_distance_ok(&t, &token) {
            return true;
        }
    }
    false
}

fn normalize_match_text(text: &str, options: EvalOptions) -> String {
    if options.case_sensitive {
        text.to_string()
    } else {
        text.to_ascii_lowercase()
    }
}

fn fuzzy_distance_ok(term: &str, token: &str) -> bool {
    if term.len() < 3 || token.len() < 3 {
        return term == token;
    }
    let max_dist = if term.len() <= 4 {
        1
    } else if term.len() <= 7 {
        2
    } else {
        3
    };
    damerau_levenshtein(term, token) <= max_dist
}

/// Resolve dotted paths like `meta.region` or `tags.0`.
fn get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        if let Ok(idx) = part.parse::<usize>() {
            if let Value::Array(arr) = current {
                current = arr.get(idx)?;
            } else {
                return None;
            }
        } else if let Value::Object(map) = current {
            current = map.get(part)?;
        } else {
            return None;
        }
    }
    Some(current)
}

const DEFAULT_INDEX_FIELDS: [&str; 3] = ["category", "country", "active"];
const DEFAULT_RESULT_CACHE_CAP: usize = 128;
const DEFAULT_RESULT_CACHE_MIN_HITS: usize = 2;
const MAX_QUERY_LEN: usize = 8192;
const MAX_NESTING_DEPTH: usize = 100;
const MAX_REGEX_LEN: usize = 256;
const MAX_RESULT_ROWS: usize = 1_000_000; // Increased to support large datasets
#[allow(dead_code)]
const PAGE_SIZE_DEFAULT: usize = 25;

fn normalize_index_fields(fields: Option<Vec<String>>) -> Vec<String> {
    match fields {
        Some(list) if !list.is_empty() => list,
        _ => DEFAULT_INDEX_FIELDS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Build default indexes for common fields to speed filtering.
fn build_indexes_for_fields(
    data: &[Value],
    fields: &[String],
) -> HashMap<String, HashMap<String, Vec<usize>>> {
    let mut indexes = HashMap::new();
    for field in fields.iter() {
        indexes.insert(field.to_string(), build_index_for_field(data, field));
    }
    indexes
}

/// Build a single-field inverted index.
fn build_index_for_field(data: &[Value], field: &str) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, item) in data.iter().enumerate() {
        if let Some(value) = get_path(item, field) {
            match value {
                Value::Array(arr) => {
                    for v in arr.iter() {
                        if let Some(key) = index_key_from_value(v, false) {
                            map.entry(key).or_default().push(idx);
                        }
                    }
                }
                _ => {
                    if let Some(key) = index_key_from_value(value, false) {
                        map.entry(key).or_default().push(idx);
                    }
                }
            }
        }
    }
    map
}

#[derive(serde::Serialize)]
struct IndexStats {
    field: String,
    keys: usize,
    entries: usize,
}

/// Normalize a JSON value into an index key.
fn index_key_from_value(value: &Value, case_sensitive: bool) -> Option<String> {
    match value {
        Value::String(s) => {
            let v = if case_sensitive {
                s.clone()
            } else {
                s.to_ascii_lowercase()
            };
            Some(format!("s:{}", v))
        }
        Value::Number(n) => n.as_f64().map(|v| format!("n:{}", v)),
        Value::Bool(b) => Some(format!("b:{}", b)),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct IndexFilter {
    field: String,
    values: Vec<ValueLit>,
}

/// Extract index-eligible filters from the AST.
fn collect_index_filters(expr: &Expr) -> Vec<IndexFilter> {
    match expr {
        Expr::And(parts) => parts.iter().flat_map(collect_index_filters).collect(),
        Expr::Predicate(pred) => match pred.op {
            Op::Eq | Op::In => vec![IndexFilter {
                field: pred.field.clone(),
                values: pred.values.clone(),
            }],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn predicate_is_indexed(pred: &Predicate, engine: Option<&Engine>) -> bool {
    match pred.op {
        Op::Eq | Op::In => {
            if let Some(eng) = engine {
                if !eng.indexes.contains_key(&pred.field) {
                    return false;
                }
                return pred.values.iter().all(|v| {
                    matches!(
                        v,
                        ValueLit::Str(_) | ValueLit::Num(_) | ValueLit::Bool(_) | ValueLit::Null
                    )
                });
            }
            false
        }
        _ => false,
    }
}

fn predicate_cost(pred: &Predicate, engine: Option<&Engine>) -> usize {
    if predicate_is_indexed(pred, engine) {
        return 1;
    }
    match pred.op {
        Op::Exists | Op::IsNull | Op::IsNotNull => 2,
        Op::Eq | Op::Neq | Op::Gt | Op::Gte | Op::Lt | Op::Lte => 3,
        Op::Between => 4,
        Op::In | Op::NotIn => 5,
        Op::Contains | Op::StartsWith | Op::EndsWith | Op::Like | Op::NotLike => 7,
        Op::Regex | Op::NotRegex | Op::Fuzzy | Op::NotFuzzy => 9,
    }
}

fn expr_cost(expr: &Expr, engine: Option<&Engine>) -> usize {
    match expr {
        Expr::Predicate(pred) => predicate_cost(pred, engine),
        Expr::Term(_) | Expr::FuzzyTerm(_) => 10,
        Expr::And(_) => 6,
        Expr::Or(_) => 12,
        Expr::Not(inner) => expr_cost(inner, engine) + 1,
        Expr::All => 0,
    }
}

fn optimize_expr(expr: &Expr, engine: Option<&Engine>) -> Expr {
    match expr {
        Expr::And(parts) => {
            let mut optimized: Vec<Expr> = parts.iter().map(|p| optimize_expr(p, engine)).collect();
            optimized.sort_by_key(|e| expr_cost(e, engine));
            Expr::And(optimized)
        }
        Expr::Or(parts) => Expr::Or(parts.iter().map(|p| optimize_expr(p, engine)).collect()),
        Expr::Not(inner) => Expr::Not(Box::new(optimize_expr(inner, engine))),
        _ => expr.clone(),
    }
}

/// Use indexes to derive a candidate list for evaluation.
fn candidate_indices(engine: &Engine, expr: &Expr, options: EvalOptions) -> Option<Vec<usize>> {
    if options.case_sensitive || !options.strict {
        return None;
    }
    match expr {
        Expr::Or(parts) => {
            let mut union: Vec<usize> = Vec::new();
            for part in parts {
                let part = candidate_indices(engine, part, options)?;
                union.extend(part);
            }
            if union.is_empty() {
                return None;
            }
            union.sort_unstable();
            union.dedup();
            Some(union)
        }
        _ => {
            let filters = collect_index_filters(expr);
            if filters.is_empty() {
                return None;
            }
            let mut sets: Vec<Vec<usize>> = Vec::new();
            for filter in filters {
                let index = engine.indexes.get(&filter.field)?;
                let mut idxs: Vec<usize> = Vec::new();
                for v in filter.values.iter() {
                    let key = match v {
                        ValueLit::Str(s) => Some(format!("s:{}", s.to_ascii_lowercase())),
                        ValueLit::Num(n) => Some(format!("n:{}", n)),
                        ValueLit::Bool(b) => Some(format!("b:{}", b)),
                        ValueLit::Null => Some("null".to_string()),
                        _ => None,
                    };
                    if let Some(k) = key {
                        if let Some(list) = index.get(&k) {
                            idxs.extend(list.iter().copied());
                        }
                    }
                }
                if !idxs.is_empty() {
                    idxs.sort_unstable();
                    idxs.dedup();
                    sets.push(idxs);
                }
            }
            if sets.is_empty() {
                return None;
            }
            sets.sort_by_key(|s| s.len());
            let mut result = sets.remove(0);
            for set in sets {
                let mut inter = Vec::new();
                let mut i = 0usize;
                let mut j = 0usize;
                while i < result.len() && j < set.len() {
                    if result[i] == set[j] {
                        inter.push(result[i]);
                        i += 1;
                        j += 1;
                    } else if result[i] < set[j] {
                        i += 1;
                    } else {
                        j += 1;
                    }
                }
                result = inter;
                if result.is_empty() {
                    break;
                }
            }
            Some(result)
        }
    }
}

thread_local! {
    static ENGINES: RefCell<HashMap<u32, Engine>> = RefCell::new(HashMap::new());
    static NEXT_ENGINE_ID: RefCell<u32> = RefCell::new(1);
    static QUERY_CACHE: RefCell<QueryCache> = RefCell::new(QueryCache::new(512));
}

/**
 * WASM: Configure global query cache size.
 *
 * Sets the LRU cache capacity for parsed queries.
 *
 * # Arguments
 * * `cap` - Maximum number of queries to cache
 *
 * # Example
 * ```js
 * set_query_cache_size(1024);
 * ```
 */
#[wasm_bindgen]
pub fn set_query_cache_size(cap: usize) {
    QUERY_CACHE.with(|cache| cache.borrow_mut().set_cap(cap));
}

#[derive(Debug, Clone)]
struct Engine {
    data: Vec<Value>,
    indexes: HashMap<String, HashMap<String, Vec<usize>>>,
    index_fields: Vec<String>,
    text_index: Option<TextIndex>,
    columnar_enabled: bool,
    columnar_fields: Vec<String>,
    columnar_store: Option<ColumnarView>,
    result_cache: ResultCache,
    metrics: EngineMetrics,
    approx_bytes: usize,
}

#[derive(Debug, Clone)]
struct TextIndex {
    doc_len: Vec<usize>,
    avg_len: f64,
    df: HashMap<String, usize>,
    tf: Vec<HashMap<String, usize>>,
}

struct QueryCache {
    map: HashMap<String, Query>,
    order: VecDeque<String>,
    cap: usize,
}

#[derive(Debug, Clone)]
struct ResultCacheEntry {
    hits: usize,
    data: Option<Vec<usize>>,
}

#[derive(Debug, Clone)]
struct ResultCache {
    map: HashMap<String, ResultCacheEntry>,
    order: VecDeque<String>,
    cap: usize,
    min_hits: usize,
    hits_served: usize,
    misses: usize,
}

impl ResultCache {
    fn new(cap: usize, min_hits: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
            min_hits: min_hits.max(1),
            hits_served: 0,
            misses: 0,
        }
    }

    fn set_min_hits(&mut self, min_hits: usize) {
        self.min_hits = min_hits.max(1);
    }

    fn get(&mut self, key: &str) -> Option<Vec<usize>> {
        let hit = self.map.get(key).and_then(|entry| entry.data.clone());
        if hit.is_some() {
            self.hits_served += 1;
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.to_string());
            return hit;
        }
        self.misses += 1;
        None
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits_served,
            misses: self.misses,
            entries: self.order.len(),
            cap: self.cap,
        }
    }

    fn record(&mut self, key: &str, data: &Vec<usize>) {
        let entry = self.map.entry(key.to_string()).or_insert(ResultCacheEntry {
            hits: 0,
            data: None,
        });
        entry.hits += 1;
        if entry.data.is_some() {
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.to_string());
            return;
        }
        if entry.hits >= self.min_hits {
            entry.data = Some(data.clone());
            self.order.push_back(key.to_string());
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    if let Some(old_entry) = self.map.get_mut(&old) {
                        old_entry.data = None;
                    }
                }
            }
        }
    }
}

#[derive(serde::Serialize)]
struct CacheStats {
    hits: usize,
    misses: usize,
    entries: usize,
    cap: usize,
}

#[derive(Debug, Clone)]
struct EngineMetrics {
    query_count: usize,
    total_ms: f64,
    latency_samples: VecDeque<f64>,
    rows_scanned: usize,
    cache_hits: usize,
    cache_misses: usize,
}

impl EngineMetrics {
    fn new() -> Self {
        Self {
            query_count: 0,
            total_ms: 0.0,
            latency_samples: VecDeque::new(),
            rows_scanned: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    fn record(&mut self, elapsed_ms: f64, scanned: usize, cache_hit: bool) {
        self.query_count += 1;
        self.total_ms += elapsed_ms;
        self.rows_scanned += scanned;
        if cache_hit {
            self.cache_hits += 1;
        } else {
            self.cache_misses += 1;
        }
        self.latency_samples.push_back(elapsed_ms);
        if self.latency_samples.len() > 200 {
            self.latency_samples.pop_front();
        }
    }

    fn avg_latency(&self) -> f64 {
        if self.query_count == 0 {
            0.0
        } else {
            self.total_ms / self.query_count as f64
        }
    }

    fn p95_latency(&self) -> f64 {
        if self.latency_samples.is_empty() {
            return 0.0;
        }
        let mut list: Vec<f64> = self.latency_samples.iter().copied().collect();
        list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((list.len() as f64) * 0.95).ceil() as usize - 1;
        list[idx.clamp(0, list.len() - 1)]
    }
}

#[derive(serde::Serialize)]
struct EngineMetricsSnapshot {
    query_count: usize,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
    rows_scanned: usize,
    cache_hit_rate: f64,
}

#[derive(serde::Serialize)]
struct ValidationErrorInfo {
    message: String,
    pos: usize,
}

#[derive(serde::Serialize)]
struct ValidationResult {
    ok: bool,
    normalized: Option<String>,
    error: Option<ValidationErrorInfo>,
}

#[derive(serde::Deserialize)]
struct AggSpec {
    #[serde(default)]
    group_by: Vec<String>,
    #[serde(default)]
    aggs: Vec<AggDef>,
    #[serde(default)]
    distinct_fields: Vec<String>,
    #[serde(default)]
    filter: Option<String>,
}

#[derive(serde::Deserialize)]
struct AggDef {
    op: String,
    field: Option<String>,
    #[serde(default)]
    alias: Option<String>,
}

#[derive(Default, Clone)]
struct AggState {
    count: usize,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
}

impl QueryCache {
    fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    fn get(&mut self, key: &str) -> Option<Query> {
        if self.map.contains_key(key) {
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.to_string());
            return self.map.get(key).cloned();
        }
        None
    }

    fn put(&mut self, key: String, value: Query) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
            self.order.push_back(key);
            return;
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct IndexHit {
    idx: usize,
    score: f64,
}

/// One-shot search over a slice of JSON objects (no handle).
pub fn search_items(items: &[Value], query: &str) -> Result<Vec<Value>, String> {
    let parsed = parse_query_cached(query)
        .map_err(|e| format!("Parse error at {}: {}", e.pos, e.message))?;
    execute_search(items, &parsed, None)
}

/// Execute a parsed query over data with optional engine context.
fn execute_search(
    items: &[Value],
    parsed: &Query,
    engine: Option<&Engine>,
) -> Result<Vec<Value>, String> {
    let mut text_index: Option<TextIndex> = None;
    if parsed.score_needed {
        if let Some(eng) = engine {
            if let Some(idx) = &eng.text_index {
                text_index = Some(idx.clone());
            }
        }
        if text_index.is_none() {
            text_index = Some(build_text_index(items));
        }
    }
    let score_terms = if parsed.score_needed {
        collect_query_terms(&parsed.expr)
    } else {
        Vec::new()
    };

    let (indices, _scanned) = execute_search_indices(items, parsed, engine)?;
    Ok(project_from_indices(
        &indices,
        items,
        parsed,
        text_index.as_ref(),
        &score_terms,
    ))
}

/// Execute a parsed query and return matched row indexes.
fn execute_search_indices(
    items: &[Value],
    parsed: &Query,
    engine: Option<&Engine>,
) -> Result<(Vec<usize>, usize), String> {
    let options = EvalOptions {
        case_sensitive: parsed.case_sensitive,
        strict: parsed.strict,
    };

    let optimized_expr = optimize_expr(&parsed.expr, engine);

    let candidates = if options.strict && !options.case_sensitive {
        engine.and_then(|eng| candidate_indices(eng, &optimized_expr, options))
    } else {
        None
    };

    let columnar_view = if let Some(eng) = engine {
        if let Some(store) = eng.columnar_store.as_ref() {
            ColumnarRef::Borrowed(store)
        } else if eng.columnar_enabled {
            let mut fields = Vec::new();
            collect_expr_fields(&optimized_expr, &mut fields);
            for order in parsed.order_by.iter() {
                if !fields.iter().any(|f| f == &order.field) {
                    fields.push(order.field.clone());
                }
            }
            let allowed = if eng.columnar_fields.is_empty() {
                fields
            } else {
                fields
                    .into_iter()
                    .filter(|f| eng.columnar_fields.iter().any(|af| af == f))
                    .collect()
            };
            if allowed.is_empty() {
                ColumnarRef::None
            } else {
                ColumnarRef::Owned(build_columnar_view(items, &allowed))
            }
        } else {
            ColumnarRef::None
        }
    } else {
        ColumnarRef::None
    };

    let mut text_index: Option<TextIndex> = None;
    if parsed.score_needed {
        if let Some(eng) = engine {
            if let Some(idx) = &eng.text_index {
                text_index = Some(idx.clone());
            }
        }
        if text_index.is_none() {
            text_index = Some(build_text_index(items));
        }
    }
    let score_terms = if parsed.score_needed {
        collect_query_terms(&parsed.expr)
    } else {
        Vec::new()
    };

    let iter: Box<dyn Iterator<Item = usize>> = if let Some(list) = candidates {
        Box::new(list.into_iter())
    } else {
        Box::new(0..items.len())
    };

    if parsed.order_by.is_empty() {
        let mut results: Vec<usize> = Vec::new();
        let mut total_matched: usize = 0;
        let offset = parsed.offset.unwrap_or(0);
        // Don't cap limit - we need to scan enough to handle large offsets (e.g., 500K records)
        let limit = parsed.limit.unwrap_or(usize::MAX).min(MAX_RESULT_ROWS);

        for idx in iter {
            let item = &items[idx];
            if eval_expr(&optimized_expr, item, options, columnar_view.as_ref(), idx) {
                total_matched += 1;
                // Only collect if past offset and under limit
                if total_matched > offset && results.len() < limit {
                    results.push(idx);
                }
            }
        }
        return Ok((results, total_matched));
    }

    let mut hits: Vec<IndexHit> = Vec::new();
    let mut matched = 0usize;
    for idx in iter {
        let item = &items[idx];
        if eval_expr(&optimized_expr, item, options, columnar_view.as_ref(), idx) {
            matched += 1;
            let score = if let Some(ti) = &text_index {
                score_doc(idx, ti, &score_terms)
            } else {
                0.0
            };
            hits.push(IndexHit { idx, score });
        }
    }

    let order_by = &parsed.order_by;
    hits.sort_by(|a, b| compare_for_sort_idx(a, b, items, order_by, options));

    let mut results: Vec<IndexHit> = hits;
    if let Some(offset) = parsed.offset {
        results = results.into_iter().skip(offset).collect();
    }
    if let Some(limit) = parsed.limit {
        results = results
            .into_iter()
            .take(limit.min(MAX_RESULT_ROWS))
            .collect();
    } else if results.len() > MAX_RESULT_ROWS {
        results.truncate(MAX_RESULT_ROWS);
    }

    Ok((results.into_iter().map(|hit| hit.idx).collect(), matched))
}

fn project_from_indices(
    indices: &[usize],
    items: &[Value],
    parsed: &Query,
    text_index: Option<&TextIndex>,
    score_terms: &[String],
) -> Vec<Value> {
    indices
        .iter()
        .map(|idx| {
            let item = &items[*idx];
            let score = if parsed.score_needed {
                if let Some(ti) = text_index {
                    score_doc(*idx, ti, score_terms)
                } else {
                    0.0
                }
            } else {
                0.0
            };
            if let Some(projection) = &parsed.projection {
                project_value(item, projection, score)
            } else {
                item.clone()
            }
        })
        .collect()
}

#[allow(dead_code)]
fn build_distinct(items: &[Value], fields: &[String]) -> Result<Vec<Value>, String> {
    if fields.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items.iter() {
        let mut key_parts = Vec::new();
        let mut obj = serde_json::Map::new();
        for f in fields.iter() {
            let v = get_path(item, f).cloned().unwrap_or(Value::Null);
            key_parts.push(serde_json::to_string(&v).unwrap_or_default());
            obj.insert(f.clone(), v);
        }
        let key = key_parts.join("|");
        if seen.insert(key) {
            out.push(Value::Object(obj));
        }
    }
    Ok(out)
}

fn aggregate_items_indices(
    items: &[Value],
    indices: impl Iterator<Item = usize>,
    spec: &AggSpec,
) -> Result<Vec<Value>, String> {
    if !spec.distinct_fields.is_empty() && spec.aggs.is_empty() && spec.group_by.is_empty() {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let fields = spec.distinct_fields.clone();
        for idx in indices {
            let item = &items[idx];
            let mut key_parts = Vec::new();
            let mut obj = serde_json::Map::new();
            for f in fields.iter() {
                let v = get_path(item, f).cloned().unwrap_or(Value::Null);
                key_parts.push(serde_json::to_string(&v).unwrap_or_default());
                obj.insert(f.clone(), v);
            }
            let key = key_parts.join("|");
            if seen.insert(key) {
                out.push(Value::Object(obj));
            }
        }
        return Ok(out);
    }

    let mut groups: HashMap<String, serde_json::Map<String, Value>> = HashMap::new();
    let mut states: HashMap<String, AggState> = HashMap::new();

    for idx in indices {
        let item = &items[idx];
        let mut key_parts = Vec::new();
        let mut group_obj = serde_json::Map::new();
        for field in spec.group_by.iter() {
            let v = get_path(item, field).cloned().unwrap_or(Value::Null);
            key_parts.push(serde_json::to_string(&v).unwrap_or_default());
            group_obj.insert(field.clone(), v);
        }
        let key = key_parts.join("|");
        groups.entry(key.clone()).or_insert_with(|| group_obj);
        let state = states.entry(key).or_default();

        for agg in spec.aggs.iter() {
            let op = agg.op.to_ascii_uppercase();
            match op.as_str() {
                "COUNT" => {
                    if let Some(field) = &agg.field {
                        if field == "*" {
                            state.count += 1;
                        } else if get_path(item, field).is_some() {
                            state.count += 1;
                        }
                    } else {
                        state.count += 1;
                    }
                }
                "SUM" | "AVG" | "MIN" | "MAX" => {
                    let field = agg.field.as_ref().ok_or("Aggregate field required")?;
                    if let Some(Value::Number(n)) = get_path(item, field) {
                        if let Some(v) = n.as_f64() {
                            state.sum += v;
                            state.min = Some(state.min.map(|m| m.min(v)).unwrap_or(v));
                            state.max = Some(state.max.map(|m| m.max(v)).unwrap_or(v));
                            state.count += 1;
                        }
                    }
                }
                _ => return Err(format!("Unknown aggregate op: {}", agg.op)),
            }
        }
    }

    let mut out = Vec::new();
    for (key, mut row) in groups {
        let state = states.get(&key).cloned().unwrap_or_default();
        for agg in spec.aggs.iter() {
            let op = agg.op.to_ascii_uppercase();
            let name = agg.alias.clone().unwrap_or_else(|| {
                format!(
                    "{}_{}",
                    op,
                    agg.field.clone().unwrap_or_else(|| "*".to_string())
                )
            });
            let value = match op.as_str() {
                "COUNT" => Value::Number(serde_json::Number::from(state.count as i64)),
                "SUM" => serde_json::Number::from_f64(state.sum)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                "AVG" => {
                    if state.count == 0 {
                        Value::Null
                    } else {
                        serde_json::Number::from_f64(state.sum / state.count as f64)
                            .map(Value::Number)
                            .unwrap_or(Value::Null)
                    }
                }
                "MIN" => state
                    .min
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                "MAX" => state
                    .max
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            };
            row.insert(name, value);
        }
        out.push(Value::Object(row));
    }
    Ok(out)
}

fn aggregate_items(items: &[Value], spec: &AggSpec) -> Result<Vec<Value>, String> {
    aggregate_items_indices(items, 0..items.len(), spec)
}

/**
 * WASM: Search from JsValue array; returns JsValue array.
 *
 * # Arguments
 * * `items` - JSON array as JsValue (e.g., from JavaScript array)
 * * `query` - Search query string (e.g., 'country = "India" AND category = "software"')
 *
 * # Returns
 * Array of matching JSON objects
 *
 * # Example
 * ```js
 * const results = search_json(dataArray, 'name LIKE "%test%"');
 * ```
 */
#[wasm_bindgen]
pub fn search_json(items: JsValue, query: String) -> Result<JsValue, JsValue> {
    let data: Vec<Value> = serde_wasm_bindgen::from_value(items)
        .map_err(|e| JsValue::from_str(&format!("Invalid input: {}", e)))?;
    let results = search_items(&data, &query).map_err(|e| JsValue::from_str(&e))?;
    serde_wasm_bindgen::to_value(&results)
        .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
}

/**
 * WASM: Search from JSON string; returns JsValue array.
 *
 * # Arguments
 * * `items_json` - JSON string representation of array
 * * `query` - Search query string
 *
 * # Returns
 * Array of matching JSON objects
 *
 * # Example
 * ```js
 * const results = search_json_string('[{"name":"test"}]', 'name = "test"');
 * ```
 */
#[wasm_bindgen]
pub fn search_json_string(items_json: String, query: String) -> Result<JsValue, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let results = search_items(&data, &query).map_err(|e| JsValue::from_str(&e))?;
    serde_wasm_bindgen::to_value(&results)
        .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
}

/**
 * WASM: Search from JSON string; returns JSON string result.
 *
 * # Arguments
 * * `items_json` - JSON string representation of array
 * * `query` - Search query string
 *
 * # Returns
 * JSON string of matching objects
 *
 * # Example
 * ```js
 * const results = execute_query_json('[{"name":"test"}]', 'name = "test"');
 * ```
 */
#[wasm_bindgen]
pub fn execute_query_json(items_json: String, query: String) -> Result<String, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let parsed = parse_query_cached(&query)
        .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;
    let results = execute_search(&data, &parsed, None).map_err(|e| JsValue::from_str(&e))?;
    serde_json::to_string(&results)
        .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
}

/**
 * WASM: Initialize a search engine from JSON string.
 *
 * Creates an engine handle for repeated queries without re-parsing data.
 *
 * # Arguments
 * * `items_json` - JSON string of array of objects to search
 *
 * # Returns
 * Engine handle (u32) for use with execute_query functions
 *
 * # Example
 * ```js
 * const handle = init_engine('[{"name":"test"}, {"name":"demo"}]');
 * const results = execute_query(handle, 'name = "test"');
 * ```
 */
#[wasm_bindgen]
pub fn init_engine(items_json: String) -> Result<u32, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let index_fields = normalize_index_fields(None);
    let indexes = build_indexes_for_fields(&data, &index_fields);

    // Use JSON string length - O(1)
    let approx_bytes = items_json.len();

    let id = NEXT_ENGINE_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.wrapping_add(1);
        id
    });
    ENGINES.with(|engines| {
        engines.borrow_mut().insert(
            id,
            Engine {
                data,
                indexes,
                index_fields,
                text_index: None,
                columnar_enabled: false,
                columnar_fields: Vec::new(),
                columnar_store: None,
                result_cache: ResultCache::new(
                    DEFAULT_RESULT_CACHE_CAP,
                    DEFAULT_RESULT_CACHE_MIN_HITS,
                ),
                metrics: EngineMetrics::new(),
                approx_bytes,
            },
        );
    });
    Ok(id)
}

/**
 * WASM: Initialize search engine with custom index fields.
 *
 * # Arguments
 * * `items_json` - JSON string of array to search
 * * `indexes_json` - Array of field names to pre-index for faster lookups
 *
 * # Returns
 * Engine handle (u32)
 *
 * # Example
 * ```js
 * const handle = init_engine_with_indexes(data, '["country", "category"]');
 * ```
 */
#[wasm_bindgen]
pub fn init_engine_with_indexes(items_json: String, indexes_json: JsValue) -> Result<u32, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let fields: Vec<String> = serde_wasm_bindgen::from_value(indexes_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid index list: {}", e)))?;
    let index_fields = normalize_index_fields(Some(fields));
    let indexes = build_indexes_for_fields(&data, &index_fields);

    // Use JSON string length - O(1)
    let approx_bytes = items_json.len();

    let id = NEXT_ENGINE_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.wrapping_add(1);
        id
    });
    ENGINES.with(|engines| {
        engines.borrow_mut().insert(
            id,
            Engine {
                data,
                indexes,
                index_fields,
                text_index: None,
                columnar_enabled: false,
                columnar_fields: Vec::new(),
                columnar_store: None,
                result_cache: ResultCache::new(
                    DEFAULT_RESULT_CACHE_CAP,
                    DEFAULT_RESULT_CACHE_MIN_HITS,
                ),
                metrics: EngineMetrics::new(),
                approx_bytes,
            },
        );
    });
    Ok(id)
}

#[derive(serde::Deserialize)]
struct EngineOptions {
    #[serde(default)]
    indexes: Option<Vec<String>>,
    #[serde(default)]
    query_cache_cap: Option<usize>,
    #[serde(default)]
    columnar: Option<bool>,
    #[serde(default)]
    columnar_fields: Option<Vec<String>>,
}

/**
 * WASM: Initialize search engine with options JSON (future-proof).
 *
 * # Arguments
 * * `items_json` - JSON string of array to search
 * * `options_json` - JSON object with optional settings:
 *   - indexes: Array of field names to index
 *   - query_cache_cap: Maximum number of cached queries
 *   - columnar: Enable columnar storage
 *   - columnar_fields: Fields to store in columnar format
 *
 * # Returns
 * Engine handle (u32)
 *
 * # Example
 * ```js
 * const handle = init_engine_with_options(data, JSON.stringify({
 *   indexes: ["country", "category"],
 *   query_cache_cap: 100
 * }));
 * ```
 */
#[wasm_bindgen]
pub fn init_engine_with_options(items_json: String, options_json: String) -> Result<u32, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let options: EngineOptions = serde_json::from_str(&options_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid options JSON: {}", e)))?;
    if let Some(cap) = options.query_cache_cap {
        QUERY_CACHE.with(|cache| cache.borrow_mut().set_cap(cap));
    }
    let index_fields = normalize_index_fields(options.indexes);
    let indexes = build_indexes_for_fields(&data, &index_fields);
    let columnar_enabled = options.columnar.unwrap_or(false);
    let columnar_fields = options.columnar_fields.unwrap_or_default();
    let columnar_store = if columnar_enabled && !columnar_fields.is_empty() {
        Some(build_columnar_view(&data, &columnar_fields))
    } else {
        None
    };

    // Use JSON string length - O(1) instead of re-serializing each record
    let approx_bytes = items_json.len();

    let id = NEXT_ENGINE_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.wrapping_add(1);
        id
    });
    ENGINES.with(|engines| {
        engines.borrow_mut().insert(
            id,
            Engine {
                data,
                indexes,
                index_fields,
                text_index: None,
                columnar_enabled,
                columnar_fields,
                columnar_store,
                result_cache: ResultCache::new(
                    DEFAULT_RESULT_CACHE_CAP,
                    DEFAULT_RESULT_CACHE_MIN_HITS,
                ),
                metrics: EngineMetrics::new(),
                approx_bytes,
            },
        );
    });
    Ok(id)
}

/**
 * WASM: Initialize search engine from raw UTF-8 bytes.
 *
 * Zero-copy:接收解压后的字节数组,直接在 WASM 内存中解析 JSON,
 * 避免 JS 堆中持有完整数据集的字符串副本。
 *
 * # Arguments
 * * `bytes` - UTF-8 encoded JSON bytes (e.g., from decompressed Uint8Array)
 * * `options_json` - Same format as init_engine_with_options
 *
 * # Returns
 * Engine handle (u32) for use with execute_query functions
 *
 * # Example
 * ```js
 * // After decompressing .gz file:
 * const handle = init_engine_from_bytes(decompressedBytes, '{}');
 * const results = execute_query(handle, 'country = "India"');
 * ```
 */
#[wasm_bindgen]
pub fn init_engine_from_bytes(bytes: Vec<u8>, options_json: String) -> Result<u32, JsValue> {
    let json_str = std::str::from_utf8(&bytes)
        .map_err(|e| JsValue::from_str(&format!("Invalid UTF-8: {}", e)))?;
    let data: Vec<Value> = serde_json::from_str(json_str)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let options: EngineOptions = serde_json::from_str(&options_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid options JSON: {}", e)))?;
    if let Some(cap) = options.query_cache_cap {
        QUERY_CACHE.with(|cache| cache.borrow_mut().set_cap(cap));
    }
    let index_fields = normalize_index_fields(options.indexes);
    let indexes = build_indexes_for_fields(&data, &index_fields);
    let columnar_enabled = options.columnar.unwrap_or(false);
    let columnar_fields = options.columnar_fields.unwrap_or_default();
    let columnar_store = if columnar_enabled && !columnar_fields.is_empty() {
        Some(build_columnar_view(&data, &columnar_fields))
    } else {
        None
    };

    // Use JSON string length from original input - O(1) instead of re-serializing each record
    let approx_bytes = json_str.len();

    let id = NEXT_ENGINE_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.wrapping_add(1);
        id
    });
    ENGINES.with(|engines| {
        engines.borrow_mut().insert(
            id,
            Engine {
                data,
                indexes,
                index_fields,
                text_index: None,
                columnar_enabled,
                columnar_fields,
                columnar_store,
                result_cache: ResultCache::new(
                    DEFAULT_RESULT_CACHE_CAP,
                    DEFAULT_RESULT_CACHE_MIN_HITS,
                ),
                metrics: EngineMetrics::new(),
                approx_bytes,
            },
        );
    });
    Ok(id)
}

#[derive(serde::Serialize)]
struct EngineDataSize {
    row_count: usize,
    approx_bytes: usize,
}

/**
 * WASM: Get engine data size info.
 *
 * Returns row count and approximate in-memory byte size.
 * Useful for displaying memory savings in demo UI.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Returns
 * JSON string: { "row_count": number, "approx_bytes": number }
 *
 * # Example
 * ```js
 * const info = get_engine_data_size(handle);
 * console.log(JSON.parse(info).row_count);
 * ```
 */
/**
 * WASM: Get engine data size info.
 *
 * Returns row count and approximate in-memory byte size.
 * Useful for displaying memory savings in demo UI.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Returns
 * JSON string: { "row_count": number, "approx_bytes": number }
 *
 * # Example
 * ```js
 * const info = get_engine_data_size(handle);
 * console.log(JSON.parse(info).row_count);
 * ```
 */
#[wasm_bindgen]
pub fn get_engine_data_size(handle: u32) -> Result<String, JsValue> {
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let row_count = engine.data.len();
        // Use cached value from engine init - O(1) instead of O(n) serialization
        let approx_bytes = engine.approx_bytes;
        let info = EngineDataSize {
            row_count,
            approx_bytes,
        };
        serde_json::to_string(&info)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

#[derive(serde::Serialize)]
pub struct PagedResult {
    pub total_matches: usize,
    pub rows: Vec<Value>,
}

/// Count all rows matching a parsed query (no limit/offset applied).
/// Used internally to provide accurate total_matches for paged results.
#[allow(dead_code)]
fn count_all_matches(items: &[Value], parsed: &Query, engine: &Engine) -> usize {
    let options = EvalOptions {
        case_sensitive: parsed.case_sensitive,
        strict: parsed.strict,
    };
    let optimized_expr = optimize_expr(&parsed.expr, Some(engine));
    let candidates = if options.strict && !options.case_sensitive {
        candidate_indices(engine, &optimized_expr, options)
    } else {
        None
    };
    let columnar_view = if let Some(store) = engine.columnar_store.as_ref() {
        ColumnarRef::Borrowed(store)
    } else {
        ColumnarRef::None
    };
    let iter: Box<dyn Iterator<Item = usize>> = if let Some(list) = candidates {
        Box::new(list.into_iter())
    } else {
        Box::new(0..items.len())
    };
    iter.filter(|&idx| {
        eval_expr(
            &optimized_expr,
            &items[idx],
            options,
            columnar_view.as_ref(),
            idx,
        )
    })
    .count()
}

/**
 * WASM: Execute query with pagination support.
 *
 * Returns paginated results with accurate total match count.
 * Use LIMIT and OFFSET in query for pagination control.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `query` - Search query (include LIMIT and OFFSET, e.g., 'country = "USA" LIMIT 25 OFFSET 0')
 *
 * # Returns
 * JSON string: { "total_matches": number, "rows": [...] }
 *
 * # Example
 * ```js
 * // Get first page of 25 results:
 * const result = execute_query_paged(handle, 'category = "software" LIMIT 25 OFFSET 0');
 * const data = JSON.parse(result);
 * console.log(data.total_matches); // Total matching records
 * console.log(data.rows);          // First 25 records
 * ```
 */
#[wasm_bindgen]
pub fn execute_query_paged(handle: u32, query: String) -> Result<String, JsValue> {
    let key = normalize_query(&query);
    let parsed = parse_query_cached(&key)
        .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines
            .get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let score_terms = if parsed.score_needed {
            if engine.text_index.is_none() {
                engine.text_index = Some(build_text_index(&engine.data));
            }
            collect_query_terms(&parsed.expr)
        } else {
            Vec::new()
        };

        // Build cache key WITHOUT LIMIT/OFFSET for pagination across pages
        // Cache stores full result set; pagination happens on retrieval
        let mut cache_key = key.clone();
        // Strip LIMIT and OFFSET from cache key so all pages share same cache
        if let Some(pos) = cache_key.to_uppercase().find("LIMIT") {
            cache_key = cache_key[..pos].trim().to_string();
        }
        if !parsed.order_by.is_empty() {
            cache_key = format!("{} ORDER BY {:?}", cache_key, parsed.order_by);
        }

        // Only use cache when there's an actual filter (not just LIMIT/OFFSET)
        let use_cache = !cache_key.is_empty();

        let started = Date::now();
        let total_matches: usize;

        // Check cache first - only if we have a valid cache key
        if use_cache {
            if let Some(cached) = engine.result_cache.get(&cache_key) {
                // Cache hit - use cached full result set
                let offset = parsed.offset.unwrap_or(0);
                let limit = parsed.limit.unwrap_or(usize::MAX);
                let indices: Vec<usize> = cached.iter().skip(offset).take(limit).cloned().collect();
                total_matches = cached.len();

                let rows = project_from_indices(
                    &indices,
                    &engine.data,
                    &parsed,
                    engine.text_index.as_ref(),
                    &score_terms,
                );
                engine.metrics.record(0.0, 0, true); // Cache hit
                let result = PagedResult {
                    total_matches,
                    rows,
                };
                return serde_json::to_string(&result)
                    .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)));
            }
        }

        // Cache miss - Need to execute search WITHOUT LIMIT to get ALL matches for caching
        // Create a query clone without LIMIT/OFFSET for the full scan
        let full_query = if let Some(pos) = key.to_uppercase().find("LIMIT") {
            key[..pos].trim().to_string()
        } else {
            key.clone()
        };

        // Parse the full query (without LIMIT) for cache storage
        let full_parsed = parse_query_cached(&full_query)
            .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;

        // Execute with full query to get ALL matches (not limited)
        let (indices, scanned) = execute_search_indices(&engine.data, &full_parsed, Some(engine))
            .map_err(|e| JsValue::from_str(&e))?;

        let elapsed = Date::now() - started;
        engine.metrics.record(elapsed, scanned, false);

        // Use scanned as total_matches - this is the true total!
        total_matches = scanned;

        // Cache full result (indices before pagination) for future pages
        // Only cache if we have a valid cache key and under capacity
        if use_cache
            && !cache_key.is_empty()
            && engine.result_cache.map.len() < engine.result_cache.cap
        {
            engine.result_cache.record(&cache_key, &indices);
        }

        // Now apply LIMIT/OFFSET to the full indices for current page response
        let offset = parsed.offset.unwrap_or(0);
        let limit = parsed.limit.unwrap_or(usize::MAX);
        let paged_indices: Vec<usize> = indices.iter().skip(offset).take(limit).cloned().collect();

        let rows = project_from_indices(
            &paged_indices,
            &engine.data,
            &parsed,
            engine.text_index.as_ref(),
            &score_terms,
        );

        let result = PagedResult {
            total_matches: total_matches,
            rows,
        };
        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Execute search query using engine handle.
 *
 * Returns all matching records (use execute_query_paged for pagination).
 * Results are cached for fast repeat queries.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `query` - Search query string
 *
 * # Returns
 * JSON string array of matching objects
 *
 * # Example
 * ```js
 * const results = execute_query(handle, 'country = "India" AND category = "software"');
 * const data = JSON.parse(results);
 * ```
 */
#[wasm_bindgen]
pub fn execute_query(handle: u32, query: String) -> Result<String, JsValue> {
    let key = normalize_query(&query);
    let parsed = parse_query_cached(&key)
        .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines
            .get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let score_terms = if parsed.score_needed {
            if engine.text_index.is_none() {
                engine.text_index = Some(build_text_index(&engine.data));
            }
            collect_query_terms(&parsed.expr)
        } else {
            Vec::new()
        };
        if let Some(cached) = engine.result_cache.get(&key) {
            let results = project_from_indices(
                &cached,
                &engine.data,
                &parsed,
                engine.text_index.as_ref(),
                &score_terms,
            );
            engine.metrics.record(0.0, 0, true);
            return serde_json::to_string(&results)
                .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)));
        }
        let started = Date::now();
        let (indices, scanned) = execute_search_indices(&engine.data, &parsed, Some(engine))
            .map_err(|e| JsValue::from_str(&e))?;
        engine.result_cache.record(&key, &indices);
        let elapsed = Date::now() - started;
        engine.metrics.record(elapsed, scanned, false);
        let p95 = engine.metrics.p95_latency();
        if p95 > 50.0 {
            engine.result_cache.set_min_hits(1);
        } else {
            engine
                .result_cache
                .set_min_hits(DEFAULT_RESULT_CACHE_MIN_HITS);
        }
        let results = project_from_indices(
            &indices,
            &engine.data,
            &parsed,
            engine.text_index.as_ref(),
            &score_terms,
        );
        serde_json::to_string(&results)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Execute query and return matched row indices.
 *
 * Useful for external pagination or custom result handling.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `query` - Search query string
 *
 * # Returns
 * JsValue array of indices (e.g., [0, 5, 12, ...])
 *
 * # Example
 * ```js
 * const indices = execute_query_indices(handle, 'name CONTAINS "test"');
 * ```
 */
#[wasm_bindgen]
pub fn execute_query_indices(handle: u32, query: String) -> Result<JsValue, JsValue> {
    let key = normalize_query(&query);
    let parsed = parse_query_cached(&key)
        .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines
            .get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        if let Some(cached) = engine.result_cache.get(&key) {
            engine.metrics.record(0.0, 0, true);
            return serde_wasm_bindgen::to_value(&cached)
                .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)));
        }
        if parsed.score_needed && engine.text_index.is_none() {
            engine.text_index = Some(build_text_index(&engine.data));
        }
        let started = Date::now();
        let (results, scanned) = execute_search_indices(&engine.data, &parsed, Some(engine))
            .map_err(|e| JsValue::from_str(&e))?;
        engine.result_cache.record(&key, &results);
        let elapsed = Date::now() - started;
        engine.metrics.record(elapsed, scanned, false);
        let p95 = engine.metrics.p95_latency();
        if p95 > 50.0 {
            engine.result_cache.set_min_hits(1);
        } else {
            engine
                .result_cache
                .set_min_hits(DEFAULT_RESULT_CACHE_MIN_HITS);
        }
        serde_wasm_bindgen::to_value(&results)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Get engine cache statistics.
 *
 * Returns cache performance metrics.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Returns
 * JsValue with: { hits, misses, entries, cap }
 *
 * # Example
 * ```js
 * const stats = engine_cache_stats(handle);
 * console.log(stats.hits, stats.misses);
 * ```
 */
#[wasm_bindgen]
pub fn engine_cache_stats(handle: u32) -> Result<JsValue, JsValue> {
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        serde_wasm_bindgen::to_value(&engine.result_cache.stats())
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Get engine performance metrics.
 *
 * Returns latency and cache performance metrics.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Returns
 * JsValue with: { p95_latency, avg_latency, cache_hits, cache_misses, cache_hit_rate }
 *
 * # Example
 * ```js
 * const metrics = get_metrics(handle);
 * console.log(metrics.p95_latency);
 * ```
 */
#[wasm_bindgen]
pub fn get_metrics(handle: u32) -> Result<JsValue, JsValue> {
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let total_cache = engine.metrics.cache_hits + engine.metrics.cache_misses;
        let cache_hit_rate = if total_cache == 0 {
            0.0
        } else {
            engine.metrics.cache_hits as f64 / total_cache as f64
        };
        let snapshot = EngineMetricsSnapshot {
            query_count: engine.metrics.query_count,
            avg_latency_ms: engine.metrics.avg_latency(),
            p95_latency_ms: engine.metrics.p95_latency(),
            rows_scanned: engine.metrics.rows_scanned,
            cache_hit_rate,
        };
        serde_wasm_bindgen::to_value(&snapshot)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Validate query syntax without executing.
 *
 * Returns validation result with any errors or warnings.
 *
 * # Arguments
 * * `query` - Search query string to validate
 *
 * # Returns
 * JsValue: { ok: boolean, normalized: string?, error: { message, pos }?, warnings: [] }
 *
 * # Example
 * ```js
 * const result = validate_query('country = "India"');
 * if (!result.ok) console.log(result.error.message);
 * ```
 */
#[wasm_bindgen]
pub fn validate_query(query: String) -> Result<JsValue, JsValue> {
    let normalized = normalize_query(&query);
    let result = match parse_query(&normalized) {
        Ok(_) => ValidationResult {
            ok: true,
            normalized: Some(normalized),
            error: None,
        },
        Err(e) => ValidationResult {
            ok: false,
            normalized: None,
            error: Some(ValidationErrorInfo {
                message: e.message,
                pos: e.pos,
            }),
        },
    };
    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
}

/**
 * WASM: Aggregate data using aggregation spec.
 *
 * Compute SUM, AVG, MIN, MAX, COUNT, DISTINCT on field values.
 *
 * # Arguments
 * * `items_json` - JSON array string
 * * `spec_json` - Aggregation specification:
 *   - field: string (field path like "price" or "meta.region")
 *   - op: "SUM" | "AVG" | "MIN" | "MAX" | "COUNT" | "DISTINCT"
 *
 * # Returns
 * JsValue array of aggregation results
 *
 * # Example
 * ```js
 * const results = aggregate_json(data, '[{"field": "price", "op": "SUM"}]');
 * ```
 */
#[wasm_bindgen]
pub fn aggregate_json(items_json: String, spec_json: String) -> Result<JsValue, JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    let spec: AggSpec = serde_json::from_str(&spec_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid spec: {}", e)))?;
    let results = if let Some(filter) = spec.filter.as_ref() {
        let parsed = parse_query_cached(filter)
            .map_err(|e| JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message)))?;
        let (indices, _scanned) =
            execute_search_indices(&data, &parsed, None).map_err(|e| JsValue::from_str(&e))?;
        aggregate_items_indices(&data, indices.into_iter(), &spec)
            .map_err(|e| JsValue::from_str(&e))?
    } else {
        aggregate_items(&data, &spec).map_err(|e| JsValue::from_str(&e))?
    };
    serde_wasm_bindgen::to_value(&results)
        .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
}

/**
 * WASM: Aggregate over engine handle.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `spec_json` - Aggregation specification
 *
 * # Returns
 * JsValue array of aggregation results
 *
 * # Example
 * ```js
 * const results = aggregate_handle(handle, '[{"field": "price", "op": "SUM"}]');
 * ```
 */
#[wasm_bindgen]
pub fn aggregate_handle(handle: u32, spec_json: String) -> Result<JsValue, JsValue> {
    let spec: AggSpec = serde_json::from_str(&spec_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid spec: {}", e)))?;
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let results = if let Some(filter) = spec.filter.as_ref() {
            let parsed = parse_query_cached(filter).map_err(|e| {
                JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message))
            })?;
            let (indices, _scanned) = execute_search_indices(&engine.data, &parsed, Some(engine))
                .map_err(|e| JsValue::from_str(&e))?;
            aggregate_items_indices(&engine.data, indices.into_iter(), &spec)
                .map_err(|e| JsValue::from_str(&e))?
        } else {
            aggregate_items(&engine.data, &spec).map_err(|e| JsValue::from_str(&e))?
        };
        serde_wasm_bindgen::to_value(&results)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Aggregate over engine handle and return JSON string.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `spec_json` - Aggregation specification
 *
 * # Returns
 * JSON string array of aggregation results
 *
 * # Example
 * ```js
 * const results = aggregate_handle_json(handle, '[{"field": "price", "op": "SUM"}]');
 * ```
 */
#[wasm_bindgen]
pub fn aggregate_handle_json(handle: u32, spec_json: String) -> Result<String, JsValue> {
    let spec: AggSpec = serde_json::from_str(&spec_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid spec: {}", e)))?;
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let results = if let Some(filter) = spec.filter.as_ref() {
            let parsed = parse_query_cached(filter).map_err(|e| {
                JsValue::from_str(&format!("Parse error at {}: {}", e.pos, e.message))
            })?;
            let (indices, _scanned) = execute_search_indices(&engine.data, &parsed, Some(engine))
                .map_err(|e| JsValue::from_str(&e))?;
            aggregate_items_indices(&engine.data, indices.into_iter(), &spec)
                .map_err(|e| JsValue::from_str(&e))?
        } else {
            aggregate_items(&engine.data, &spec).map_err(|e| JsValue::from_str(&e))?
        };
        serde_json::to_string(&results)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Replace dataset contents for an existing engine handle.
 *
 * Updates the data while preserving engine configuration (indexes, cache settings).
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `items_json` - New JSON array string to replace existing data
 *
 * # Returns
 * Ok(()) on success
 *
 * # Example
 * ```js
 * update_engine(handle, '[{"name":"new data"}]');
 * ```
 */
#[wasm_bindgen]
pub fn update_engine(handle: u32, items_json: String) -> Result<(), JsValue> {
    let data: Vec<Value> = serde_json::from_str(&items_json)
        .map_err(|e| JsValue::from_str(&format!("Invalid JSON: {}", e)))?;
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        if engines.contains_key(&handle) {
            let index_fields = engines
                .get(&handle)
                .map(|e| e.index_fields.clone())
                .unwrap_or_else(|| normalize_index_fields(None));
            let (columnar_enabled, columnar_fields) = engines
                .get(&handle)
                .map(|e| (e.columnar_enabled, e.columnar_fields.clone()))
                .unwrap_or((false, Vec::new()));
            let columnar_store = if columnar_enabled && !columnar_fields.is_empty() {
                Some(build_columnar_view(&data, &columnar_fields))
            } else {
                None
            };
            let result_cache =
                ResultCache::new(DEFAULT_RESULT_CACHE_CAP, DEFAULT_RESULT_CACHE_MIN_HITS);
            let indexes = build_indexes_for_fields(&data, &index_fields);
            // Recalculate approx_bytes on update
            let approx_bytes = data
                .iter()
                .map(|v| serde_json::to_string(v).map(|s| s.len()).unwrap_or(0))
                .sum::<usize>();
            engines.insert(
                handle,
                Engine {
                    indexes,
                    data,
                    index_fields,
                    text_index: None,
                    columnar_enabled,
                    columnar_fields,
                    columnar_store,
                    result_cache,
                    metrics: EngineMetrics::new(),
                    approx_bytes,
                },
            );
            Ok(())
        } else {
            Err(JsValue::from_str("Invalid engine handle"))
        }
    })
}

/**
 * WASM: Create an index for a specific field.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `field` - Field name to index (e.g., "country" or "meta.region")
 *
 * # Returns
 * Ok(()) on success
 *
 * # Example
 * ```js
 * create_index(handle, "country");
 * ```
 */
#[wasm_bindgen]
pub fn create_index(handle: u32, field: String) -> Result<(), JsValue> {
    if !is_valid_field_name(&field) {
        return Err(JsValue::from_str("Invalid field name"));
    }
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines
            .get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        if engine.indexes.contains_key(&field) {
            return Ok(());
        }
        let index = build_index_for_field(&engine.data, &field);
        engine.indexes.insert(field.clone(), index);
        if !engine.index_fields.iter().any(|f| f == &field) {
            engine.index_fields.push(field);
        }
        Ok(())
    })
}

/**
 * WASM: Drop an index for a specific field.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 * * `field` - Field name to remove index for
 *
 * # Returns
 * Ok(()) on success
 *
 * # Example
 * ```js
 * drop_index(handle, "country");
 * ```
 */
#[wasm_bindgen]
pub fn drop_index(handle: u32, field: String) -> Result<(), JsValue> {
    ENGINES.with(|engines| {
        let mut engines = engines.borrow_mut();
        let engine = engines
            .get_mut(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        engine.indexes.remove(&field);
        engine.index_fields.retain(|f| f != &field);
        Ok(())
    })
}

/**
 * WASM: List all indexes for an engine handle.
 *
 * Returns index statistics for each indexed field.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Returns
 * JsValue array: [{ field, unique_values, sample_values }]
 *
 * # Example
 * ```js
 * const indexes = list_indexes(handle);
 * ```
 */
#[wasm_bindgen]
pub fn list_indexes(handle: u32) -> Result<JsValue, JsValue> {
    ENGINES.with(|engines| {
        let engines = engines.borrow();
        let engine = engines
            .get(&handle)
            .ok_or_else(|| JsValue::from_str("Invalid engine handle"))?;
        let mut stats = Vec::new();
        for (field, map) in engine.indexes.iter() {
            let entries = map.values().map(|v| v.len()).sum::<usize>();
            stats.push(IndexStats {
                field: field.clone(),
                keys: map.len(),
                entries,
            });
        }
        serde_wasm_bindgen::to_value(&stats)
            .map_err(|e| JsValue::from_str(&format!("Serialize error: {}", e)))
    })
}

/**
 * WASM: Drop an engine handle and free memory.
 *
 * # Arguments
 * * `handle` - Engine handle from init_engine
 *
 * # Example
 * ```js
 * drop_engine(handle);
 * ```
 */
#[wasm_bindgen]
pub fn drop_engine(handle: u32) {
    ENGINES.with(|engines| {
        engines.borrow_mut().remove(&handle);
    });
}

/// Sort index hits without cloning full values.
fn compare_for_sort_idx(
    a: &IndexHit,
    b: &IndexHit,
    items: &[Value],
    order_by: &[OrderBy],
    options: EvalOptions,
) -> std::cmp::Ordering {
    let item_a = &items[a.idx];
    let item_b = &items[b.idx];
    for order in order_by {
        let (av, bv) = if order.field.eq_ignore_ascii_case("SCORE") {
            (
                Some(Value::Number(
                    serde_json::Number::from_f64(a.score).unwrap_or(serde_json::Number::from(0)),
                )),
                Some(Value::Number(
                    serde_json::Number::from_f64(b.score).unwrap_or(serde_json::Number::from(0)),
                )),
            )
        } else {
            (
                get_path(item_a, &order.field).cloned(),
                get_path(item_b, &order.field).cloned(),
            )
        };
        let nulls_first = order.nulls_first.unwrap_or(false);
        let mut ord = match (av, bv) {
            (Some(Value::Null), Some(Value::Null)) => std::cmp::Ordering::Equal,
            (Some(Value::Null), _) => {
                if nulls_first {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            }
            (_, Some(Value::Null)) => {
                if nulls_first {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            }
            (Some(Value::Number(x)), Some(Value::Number(y))) => {
                let ax = x.as_f64().unwrap_or(0.0);
                let by = y.as_f64().unwrap_or(0.0);
                ax.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Some(Value::String(x)), Some(Value::String(y))) => {
                if options.case_sensitive {
                    x.cmp(&y)
                } else {
                    x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase())
                }
            }
            (Some(Value::Bool(x)), Some(Value::Bool(y))) => x.cmp(&y),
            (Some(ax), Some(by)) => {
                let ax = serde_json::to_string(&ax).unwrap_or_default();
                let by = serde_json::to_string(&by).unwrap_or_default();
                ax.cmp(&by)
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        if order.desc {
            ord = ord.reverse();
        }
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a.idx.cmp(&b.idx)
}

/// Extract terms for scoring (BM25).
fn collect_query_terms(expr: &Expr) -> Vec<String> {
    let mut terms = Vec::new();
    collect_terms(expr, &mut terms);
    terms
}

/// Walk AST and push scoring terms into out.
fn collect_terms(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Term(t) => out.extend(tokenize_text(t)),
        Expr::Predicate(p) => {
            if matches!(
                p.op,
                Op::Like
                    | Op::NotLike
                    | Op::Contains
                    | Op::StartsWith
                    | Op::EndsWith
                    | Op::Eq
                    | Op::Neq
            ) {
                if let Some(ValueLit::Str(s)) = p.values.get(0) {
                    out.extend(tokenize_text(s));
                }
            }
        }
        Expr::Or(parts) | Expr::And(parts) => {
            for part in parts {
                collect_terms(part, out);
            }
        }
        Expr::Not(inner) => collect_terms(inner, out),
        _ => {}
    }
}

/// Tokenize text for scoring.
fn tokenize_text(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

/// Extract all strings from a JSON value (recursive).
fn extract_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(arr) => {
            for v in arr {
                extract_strings(v, out);
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                extract_strings(v, out);
            }
        }
        _ => {}
    }
}

/// Build a BM25 text index for a dataset.
fn build_text_index(data: &[Value]) -> TextIndex {
    let mut doc_len = Vec::with_capacity(data.len());
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut tf: Vec<HashMap<String, usize>> = Vec::with_capacity(data.len());

    for item in data {
        let mut strings = Vec::new();
        extract_strings(item, &mut strings);
        let mut freq: HashMap<String, usize> = HashMap::new();
        for s in strings {
            for tok in tokenize_text(&s) {
                *freq.entry(tok).or_insert(0) += 1;
            }
        }
        let len = freq.values().sum::<usize>();
        doc_len.push(len);
        for key in freq.keys() {
            *df.entry(key.clone()).or_insert(0) += 1;
        }
        tf.push(freq);
    }
    let avg_len = if doc_len.is_empty() {
        0.0
    } else {
        doc_len.iter().sum::<usize>() as f64 / doc_len.len() as f64
    };
    TextIndex {
        doc_len,
        avg_len,
        df,
        tf,
    }
}

/// Score a single document with BM25.
fn score_doc(doc_idx: usize, index: &TextIndex, terms: &[String]) -> f64 {
    if terms.is_empty() || index.doc_len.is_empty() {
        return 0.0;
    }
    let k1 = 1.2;
    let b = 0.75;
    let n_docs = index.doc_len.len() as f64;
    let doc_len = index.doc_len[doc_idx] as f64;
    let mut score = 0.0;
    let tf_map = &index.tf[doc_idx];
    for term in terms {
        let tf = *tf_map.get(term).unwrap_or(&0) as f64;
        if tf == 0.0 {
            continue;
        }
        let df = *index.df.get(term).unwrap_or(&0) as f64;
        let idf = ((n_docs - df + 0.5) / (df + 0.5) + 1.0).ln();
        let denom = tf + k1 * (1.0 - b + b * (doc_len / index.avg_len.max(1.0)));
        score += idf * (tf * (k1 + 1.0)) / denom;
    }
    score
}

/// Project a record into selected fields (SELECT).
fn project_value(value: &Value, projection: &[String], score: f64) -> Value {
    let mut map = serde_json::Map::new();
    for field in projection {
        if field.eq_ignore_ascii_case("SCORE") {
            map.insert(
                "SCORE".to_string(),
                Value::Number(
                    serde_json::Number::from_f64(score).unwrap_or(serde_json::Number::from(0)),
                ),
            );
            continue;
        }
        if let Some(val) = get_path(value, field) {
            insert_path(&mut map, field, val);
        }
    }
    Value::Object(map)
}

/// Insert a value into a nested path in a JSON object.
fn insert_path(target: &mut serde_json::Map<String, Value>, path: &str, val: &Value) {
    let mut current = target;
    let parts: Vec<&str> = path.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current.insert(part.to_string(), val.clone());
        } else {
            if !current.contains_key(*part) {
                current.insert(part.to_string(), Value::Object(serde_json::Map::new()));
            }
            if let Some(Value::Object(map)) = current.get_mut(*part) {
                current = map;
            } else {
                return;
            }
        }
    }
}

/**
 * WASM: Get engine version string.
 *
 * # Returns
 * Version string (e.g., "search_wasm_v1.1.0")
 *
 * # Example
 * ```js
 * console.log(engine_version());
 * ```
 */
#[wasm_bindgen]
pub fn engine_version() -> String {
    "search_wasm_v1.1.0".to_string()
}
