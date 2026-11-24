//! Hilfsfunktionen für Tokenfolgen: Schreibziele, Aktualparameter, Namen.

use super::*;

/// Tabellenelement nach `TABLE OF` bzw. `STANDARD TABLE OF`.
pub(super) fn table_elem_tokens(rest: &[Token]) -> Option<&[Token]> {
    let r = if rest.first().is_some_and(|token| {
        token.is_word("STANDARD") || token.is_word("SORTED") || token.is_word("HASHED")
    }) {
        &rest[1..]
    } else {
        rest
    };
    if r.len() >= 2 && r[0].is_word("TABLE") && r[1].is_word("OF") {
        Some(&r[2..])
    } else {
        None
    }
}

/// Schreibziel aus Tokens: `feld`, `feld-komp` oder `feld+off(len)`.
pub(super) fn parse_target(toks: &[Token], line: usize) -> Result<Target, CompileError> {
    let is_num = |t: &Token| !t.text.is_empty() && t.text.chars().all(|c| c.is_ascii_digit());
    match toks {
        [owner, arrow, name]
            if owner.kind == Kind::Word
                && (arrow.is_punct("=>") || arrow.is_punct("~"))
                && name.kind == Kind::Word =>
        {
            if arrow.is_punct("~") {
                Ok(Target::Attr {
                    obj: "me".into(),
                    attr: format!("{}~{}", word(owner), word(name)),
                })
            } else {
                Ok(Target::Var(format!(
                    "{}=>{}",
                    word(owner),
                    word(name)
                )))
            }
        }
        [t] if t.kind == Kind::Word => Ok(Target::Var(word(t))),
        [t, p, off, l, len, r]
            if t.kind == Kind::Word
                && p.is_punct("+")
                && is_num(off)
                && l.is_punct("(")
                && is_num(len)
                && r.is_punct(")") =>
        {
            let off: usize = off.text.parse().or_else(|_| err(line, "Offset ungültig"))?;
            let len: usize = len.text.parse().or_else(|_| err(line, "Länge ungültig"))?;
            if len == 0 {
                return err(line, "Teilfeldlänge muss größer 0 sein");
            }
            Ok(Target::Slice { name: word(t), off, len })
        }
        [o, arrow, a] if o.kind == Kind::Word && arrow.is_punct("->") && a.kind == Kind::Word => {
            Ok(Target::Attr { obj: word(o), attr: word(a) })
        }
        _ => err(line, "Ziel muss ein Feld, ein Teilfeld name+off(len) oder ein Attribut obj->attr sein"),
    }
}

/// Operand einer Verkettung: ein einzelnes Feld, Komponente oder Literal.
pub(super) fn single_operand(t: &Token, line: usize) -> Result<Expr, CompileError> {
    match t.kind {
        Kind::Literal => match literal_content(&t.text) {
            Some(s) => Ok(Expr::Lit(s)),
            None => err(line, format!("ungültiges Literal: {}", t.text)),
        },
        Kind::Word => Ok(Expr::Var(word(t))),
        _ => err(line, format!("ungültiger Operand: {}", t.text)),
    }
}

/// Aktualparameter aus den Tokens eines PERFORM.
pub(super) fn parse_args(toks: &[Token], line: usize) -> Result<Vec<Arg>, CompileError> {
    let mut out = Vec::new();
    let mut changing = false;
    for t in toks {
        if t.is_word("USING") || t.is_word("IMPORTING") {
            changing = false;
            continue;
        }
        if t.is_word("CHANGING") {
            changing = true;
            continue;
        }
        let expr = match t.kind {
            Kind::Literal => match literal_content(&t.text) {
                Some(s) => Expr::Lit(s),
                None => return err(line, format!("ungültiges Literal: {}", t.text)),
            },
            Kind::Word if t.text.chars().all(|c| c.is_ascii_digit()) => {
                Expr::Int(t.text.parse().or_else(|_| err(line, "Zahl zu groß"))?)
            }
            Kind::Word => Expr::Var(word(t)),
            _ => return err(line, format!("ungültiger Aktualparameter: {}", t.text)),
        };
        out.push(Arg { expr, changing });
    }
    Ok(out)
}

/// Schreibziel aus einem Ausdruck (RECEIVING-Ziel).
pub(super) fn expr_to_target(e: Expr, line: usize) -> Result<Target, CompileError> {
    match e {
        Expr::Var(n) => Ok(Target::Var(n)),
        Expr::Attr { obj, attr } => Ok(Target::Attr { obj, attr }),
        Expr::Slice { name, off, len } => Ok(Target::Slice { name, off, len }),
        _ => err(line, "RECEIVING erwartet eine Variable als Ziel"),
    }
}

pub(super) fn word(t: &Token) -> String {
    t.text.to_lowercase()
}
