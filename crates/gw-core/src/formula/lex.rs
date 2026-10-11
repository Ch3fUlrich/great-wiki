//! Lexer: formula text into tokens. It knows nothing about grammar — `1+` lexes happily and
//! the parser is the one that complains about the missing operand.
//!
//! Every position in an error is a **char index**, so a message counts the same characters a
//! reader counts, and no offending byte from the input is ever echoed back.

use serde_json::Number;

use super::FormulaError::{self, Lex};

pub(crate) const RE_CHAR: &str = "unexpected character";
pub(crate) const RE_NUMBER: &str = "number out of range";
pub(crate) const RE_DOT: &str = "number ends with a dot";
pub(crate) const RE_STR: &str = "unterminated string";
pub(crate) const RE_ESC: &str = "bad escape";
pub(crate) const RE_BRACE: &str = "unterminated field reference";
pub(crate) const RE_EQ: &str = "expected ==";
pub(crate) const RE_NE: &str = "expected !=";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    /// The text between `{` and `}`, not yet checked against the dataset's field keys.
    Field(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    Comma,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

pub(crate) struct Token {
    pub(crate) tok: Tok,
    pub(crate) pos: usize,
}

/// Lex the whole input. Only the characters of the language are accepted; a `?` or a lone `=`
/// is refused here rather than silently skipped.
pub(crate) fn lex(src: &str) -> Result<Vec<Token>, FormulaError> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < cs.len() {
        let pos = i;
        let used = match cs[i] {
            ' ' | '\t' | '\r' | '\n' => 1,
            '0'..='9' => number(&cs, pos, &mut out)?,
            '"' => string(&cs, pos, &mut out)?,
            '{' => field(&cs, pos, &mut out)?,
            'a'..='z' | 'A'..='Z' | '_' => ident(&cs, pos, &mut out),
            c => operator(&cs, pos, c, &mut out)?,
        };
        i += used;
    }
    out.push(Token {
        tok: Tok::Eof,
        pos: cs.len(),
    });
    Ok(out)
}

fn number(cs: &[char], pos: usize, out: &mut Vec<Token>) -> Result<usize, FormulaError> {
    let mut i = pos;
    while cs.get(i).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
    }
    if cs.get(i) == Some(&'.') {
        if !cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            return Err(Lex(RE_DOT, pos));
        }
        i += 1;
        while cs.get(i).is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
        }
    }
    let text: String = cs[pos..i].iter().collect();
    let Ok(val) = text.parse::<f64>() else {
        return Err(Lex(RE_NUMBER, pos));
    };
    // serde_json refuses to build a non-finite number, so a literal that overflowed is refused
    // here instead of being carried into the evaluator.
    if !val.is_finite() || Number::from_f64(val).is_none() {
        return Err(Lex(RE_NUMBER, pos));
    }
    out.push(Token {
        tok: Tok::Num(val),
        pos,
    });
    Ok(i - pos)
}

fn string(cs: &[char], pos: usize, out: &mut Vec<Token>) -> Result<usize, FormulaError> {
    let mut s = String::new();
    let mut i = pos + 1;
    loop {
        let Some(&c) = cs.get(i) else {
            return Err(Lex(RE_STR, pos));
        };
        match c {
            '"' => {
                i += 1;
                break;
            }
            '\\' => {
                let esc = i;
                match cs.get(i + 1) {
                    Some(&'"') | Some(&'\\') => {
                        s.push(cs[i + 1]);
                        i += 2;
                    }
                    _ => return Err(Lex(RE_ESC, esc)),
                }
            }
            _ => {
                s.push(c);
                i += 1;
            }
        }
    }
    out.push(Token {
        tok: Tok::Str(s),
        pos,
    });
    Ok(i - pos)
}

fn field(cs: &[char], pos: usize, out: &mut Vec<Token>) -> Result<usize, FormulaError> {
    let mut key = String::new();
    let mut i = pos + 1;
    loop {
        let Some(&c) = cs.get(i) else {
            return Err(Lex(RE_BRACE, pos));
        };
        if c == '}' {
            i += 1;
            break;
        }
        if c == '{' {
            return Err(Lex(RE_BRACE, pos));
        }
        key.push(c);
        i += 1;
    }
    out.push(Token {
        tok: Tok::Field(key),
        pos,
    });
    Ok(i - pos)
}

fn ident(cs: &[char], pos: usize, out: &mut Vec<Token>) -> usize {
    let mut i = pos;
    while cs
        .get(i)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
    {
        i += 1;
    }
    let text: String = cs[pos..i].iter().collect();
    out.push(Token {
        tok: Tok::Ident(text),
        pos,
    });
    i - pos
}

fn operator(cs: &[char], pos: usize, c: char, out: &mut Vec<Token>) -> Result<usize, FormulaError> {
    let next = cs.get(pos + 1) == Some(&'=');
    let (tok, used) = match (c, next) {
        ('+', _) => (Tok::Plus, 1),
        ('-', _) => (Tok::Minus, 1),
        ('*', _) => (Tok::Star, 1),
        ('/', _) => (Tok::Slash, 1),
        ('%', _) => (Tok::Percent, 1),
        ('(', _) => (Tok::LParen, 1),
        (')', _) => (Tok::RParen, 1),
        (',', _) => (Tok::Comma, 1),
        ('<', false) => (Tok::Lt, 1),
        ('<', true) => (Tok::Le, 2),
        ('>', false) => (Tok::Gt, 1),
        ('>', true) => (Tok::Ge, 2),
        ('=', false) => return Err(Lex(RE_EQ, pos)),
        ('=', true) => (Tok::Eq, 2),
        ('!', false) => return Err(Lex(RE_NE, pos)),
        ('!', true) => (Tok::Ne, 2),
        _ => return Err(Lex(RE_CHAR, pos)),
    };
    out.push(Token { tok, pos });
    Ok(used)
}
