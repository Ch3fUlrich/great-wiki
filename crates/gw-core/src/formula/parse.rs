//! Parser: recursive descent over the token stream, precedence climbing from `or` down to
//! atoms. Nesting and node count are both capped, so a hostile expression is refused rather
//! than allowed to recurse until the stack is gone.

use super::lex::{lex, Tok, Token};
use super::FormulaError::{self, Parse};
use super::{MAX_DEPTH, MAX_NODES};
use crate::dataset::FieldKey;

pub(crate) const RE_VALUE: &str = "expected a value";
pub(crate) const RE_END: &str = "expected end of input";
pub(crate) const RE_RPAREN: &str = "expected )";
pub(crate) const RE_FUNC: &str = "unknown function";
pub(crate) const RE_IDENT: &str = "unknown identifier";

/// Binary operators, lowest precedence first in the sense that the parser layer decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Func {
    If,
    Concat,
    Round,
    Min,
    Max,
    Abs,
    Len,
    Today,
    Coalesce,
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Field(String),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

impl Func {
    fn from_name(name: &str) -> Option<Func> {
        let f = match name {
            "if" => Func::If,
            "concat" => Func::Concat,
            "round" => Func::Round,
            "min" => Func::Min,
            "max" => Func::Max,
            "abs" => Func::Abs,
            "len" => Func::Len,
            "today" => Func::Today,
            "coalesce" => Func::Coalesce,
            _ => return None,
        };
        Some(f)
    }

    /// Arity is settled at parse time, so the evaluator never has to report it.
    fn check_arity(self, n: usize, pos: usize) -> Result<(), FormulaError> {
        let want = match self {
            Func::If => n == 3,
            Func::Concat | Func::Min | Func::Max | Func::Coalesce => n >= 1,
            Func::Round => (1..=2).contains(&n),
            Func::Abs | Func::Len => n == 1,
            Func::Today => n == 0,
        };
        if want {
            return Ok(());
        }
        let msg = match self {
            Func::If => "if takes 3 arguments",
            Func::Concat => "concat takes at least 1 argument",
            Func::Round => "round takes 1 or 2 arguments",
            Func::Min => "min takes at least 1 argument",
            Func::Max => "max takes at least 1 argument",
            Func::Abs => "abs takes 1 argument",
            Func::Len => "len takes 1 argument",
            Func::Today => "today takes no arguments",
            Func::Coalesce => "coalesce takes at least 1 argument",
        };
        Err(Parse(msg, pos))
    }
}

struct P<'a> {
    toks: Vec<Token>,
    i: usize,
    nodes: usize,
    depth: usize,
    known: &'a dyn Fn(&str) -> bool,
}

/// Parse `src` into an expression. A field reference is checked against the dataset's keys
/// here, at schema-save time, so a saved formula can never name a field that is not there.
pub fn parse(src: &str, known_field: &dyn Fn(&str) -> bool) -> Result<super::Expr, FormulaError> {
    let toks = lex(src)?;
    let mut p = P {
        toks,
        i: 0,
        nodes: 0,
        depth: 0,
        known: known_field,
    };
    let node = p.or()?;
    if !matches!(p.peek().tok, Tok::Eof) {
        return Err(Parse(RE_END, p.peek().pos));
    }
    Ok(super::Expr(node))
}

impl<'a> P<'a> {
    fn peek(&self) -> &Token {
        // lex() always appends Eof, so the index is never past the end.
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn at(&self) -> usize {
        self.peek().pos
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(&self.peek().tok, Tok::Ident(s) if s == kw)
    }

    /// One more AST node, or the budget is spent.
    fn counted(&mut self) -> Result<(), FormulaError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(FormulaError::TooManyNodes);
        }
        Ok(())
    }

    /// One level deeper: parens, call arguments, `not` and unary minus all nest.
    fn enter(&mut self) -> Result<(), FormulaError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(FormulaError::TooDeep);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn or(&mut self) -> Result<Node, FormulaError> {
        let mut lhs = self.and()?;
        while self.is_kw("or") {
            self.i += 1;
            self.counted()?;
            let rhs = self.and()?;
            lhs = Node::Bin(Op::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Node, FormulaError> {
        let mut lhs = self.not()?;
        while self.is_kw("and") {
            self.i += 1;
            self.counted()?;
            let rhs = self.not()?;
            lhs = Node::Bin(Op::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn not(&mut self) -> Result<Node, FormulaError> {
        if self.is_kw("not") {
            self.i += 1;
            self.counted()?;
            self.enter()?;
            let inner = self.not()?;
            self.leave();
            return Ok(Node::Not(Box::new(inner)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Node, FormulaError> {
        let mut lhs = self.additive()?;
        loop {
            let op = match self.peek().tok {
                Tok::Eq => Op::Eq,
                Tok::Ne => Op::Ne,
                Tok::Lt => Op::Lt,
                Tok::Le => Op::Le,
                Tok::Gt => Op::Gt,
                Tok::Ge => Op::Ge,
                _ => break,
            };
            self.i += 1;
            self.counted()?;
            let rhs = self.additive()?;
            lhs = Node::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> Result<Node, FormulaError> {
        let mut lhs = self.multiplicative()?;
        loop {
            let op = match self.peek().tok {
                Tok::Plus => Op::Add,
                Tok::Minus => Op::Sub,
                _ => break,
            };
            self.i += 1;
            self.counted()?;
            let rhs = self.multiplicative()?;
            lhs = Node::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn multiplicative(&mut self) -> Result<Node, FormulaError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek().tok {
                Tok::Star => Op::Mul,
                Tok::Slash => Op::Div,
                Tok::Percent => Op::Mod,
                _ => break,
            };
            self.i += 1;
            self.counted()?;
            let rhs = self.unary()?;
            lhs = Node::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Node, FormulaError> {
        if matches!(self.peek().tok, Tok::Minus) {
            self.i += 1;
            self.counted()?;
            self.enter()?;
            let inner = self.unary()?;
            self.leave();
            return Ok(Node::Neg(Box::new(inner)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Node, FormulaError> {
        let pos = self.at();
        let tok = self.peek().tok.clone();
        self.i += 1;
        let node = match tok {
            Tok::Num(v) => {
                self.counted()?;
                Node::Num(v)
            }
            Tok::Str(s) => {
                self.counted()?;
                Node::Str(s)
            }
            Tok::Field(key) => {
                self.counted()?;
                if FieldKey::parse(&key).is_err() || !(self.known)(&key) {
                    return Err(FormulaError::UnknownField(key));
                }
                Node::Field(key)
            }
            Tok::Ident(name) => self.named(name, pos)?,
            Tok::LParen => {
                self.enter()?;
                let inner = self.or()?;
                self.leave();
                if !matches!(self.peek().tok, Tok::RParen) {
                    return Err(Parse(RE_RPAREN, self.at()));
                }
                self.i += 1;
                inner
            }
            _ => return Err(Parse(RE_VALUE, pos)),
        };
        Ok(node)
    }

    /// `true`, `false`, `null`, and a function call.
    fn named(&mut self, name: String, pos: usize) -> Result<Node, FormulaError> {
        if matches!(self.peek().tok, Tok::LParen) {
            let Some(func) = Func::from_name(&name) else {
                return Err(Parse(RE_FUNC, pos));
            };
            self.i += 1;
            let args = self.arguments(func, pos)?;
            self.counted()?;
            return Ok(Node::Call(func, args));
        }
        match name.as_str() {
            "true" => {
                self.counted()?;
                Ok(Node::Bool(true))
            }
            "false" => {
                self.counted()?;
                Ok(Node::Bool(false))
            }
            "null" => {
                self.counted()?;
                Ok(Node::Null)
            }
            // Reached only when a connective has no left operand, e.g. `and true`.
            "and" | "or" | "not" => Err(Parse(RE_VALUE, pos)),
            _ => Err(Parse(RE_IDENT, pos)),
        }
    }

    fn arguments(&mut self, func: Func, pos: usize) -> Result<Vec<Node>, FormulaError> {
        let mut args = Vec::new();
        if matches!(self.peek().tok, Tok::RParen) {
            self.i += 1;
        } else {
            loop {
                self.enter()?;
                let arg = self.or()?;
                self.leave();
                args.push(arg);
                if matches!(self.peek().tok, Tok::Comma) {
                    self.i += 1;
                    continue;
                }
                if matches!(self.peek().tok, Tok::RParen) {
                    self.i += 1;
                    break;
                }
                return Err(Parse(RE_RPAREN, self.at()));
            }
        }
        func.check_arity(args.len(), pos)?;
        Ok(args)
    }
}

/// Distinct field keys an expression reads, in order of first appearance. The dataset schema
/// uses these for the dependency graph, so a formula field can be refused for cycling.
pub fn field_refs(e: &super::Expr) -> Vec<String> {
    let mut out = Vec::new();
    collect(&e.0, &mut out);
    out
}

fn collect(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Field(key) => {
            if !out.contains(key) {
                out.push(key.clone());
            }
        }
        Node::Num(_) | Node::Str(_) | Node::Bool(_) | Node::Null => {}
        Node::Neg(inner) | Node::Not(inner) => collect(inner, out),
        Node::Bin(_, lhs, rhs) => {
            collect(lhs, out);
            collect(rhs, out);
        }
        Node::Call(_, args) => args.iter().for_each(|a| collect(a, out)),
    }
}
