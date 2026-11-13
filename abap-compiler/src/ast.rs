//! Ausdrucks- und Bedingungs-AST sowie ihr Parser über die Tokens des ABAP-Parsers.

use abapgit_inspect::abap::{Kind, Token};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    /// Zeichenkettenliteral (Inhalt ohne Anführungszeichen); Länge in Zeichen
    Lit(String),
    Var(String),
    /// Attribut eines Objekts: `obj->attr` (auch `me->attr`)
    Attr { obj: String, attr: String },
    /// Methodenaufruf im Ausdruck mit Rückgabewert: `obj->methode( p = a )`
    Call { recv: String, method: String, args: Vec<NamedArg> },
    /// Teilfeld `name+off(len)` (Offset 0-basiert, wie in ABAP)
    Slice { name: String, off: usize, len: usize },
    /// Verkettung mit `&&`
    Concat(Vec<Expr>),
    /// Verkettung wie CONCATENATE ... COMPRESSED: nachgestellte Leerzeichen der Operanden entfallen
    Compressed(Vec<Expr>),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

/// Aktualparameter eines Methodenaufrufs. `name` ist der Formalparameter (None bei `meth( a )`),
/// `changing` das Schlüsselwort (None, wenn keines angegeben ist).
#[derive(Debug, Clone, PartialEq)]
pub struct NamedArg {
    pub name: Option<String>,
    pub changing: Option<bool>,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    Cmp(CmpOp, Expr, Expr),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
}

/// Fehler mit Zeilennummer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Zeile {}: {}", self.line, self.message)
    }
}

pub fn err<T>(line: usize, msg: impl Into<String>) -> Result<T, CompileError> {
    Err(CompileError { line, message: msg.into() })
}

/// Parst einen Ausdruck. Die gesamte Tokenfolge muss verbraucht werden.
pub fn parse_expr(toks: &[Token], line: usize) -> Result<Expr, CompileError> {
    let mut p = Parser { toks, pos: 0, line };
    let e = p.concat()?;
    p.finish(toks)?;
    Ok(e)
}

/// Parst eine Bedingung (IF, ELSEIF, WHILE, CHECK). Die gesamte Tokenfolge muss verbraucht werden.
pub fn parse_cond(toks: &[Token], line: usize) -> Result<Cond, CompileError> {
    let mut p = Parser { toks, pos: 0, line };
    let c = p.cond_or()?;
    p.finish(toks)?;
    Ok(c)
}

/// Inhalt eines Zeichenkettenliterals ohne Anführungszeichen; doppelte Quotes werden aufgelöst.
pub fn literal_content(text: &str) -> Option<String> {
    let quote = text.chars().next()?;
    if quote != '\'' && quote != '`' {
        return None;
    }
    let inner = text.get(1..text.len().checked_sub(1)?)?;
    Some(inner.replace(&format!("{quote}{quote}"), &quote.to_string()))
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    line: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.toks.get(self.pos + offset)
    }

    fn finish(&self, toks: &[Token]) -> Result<(), CompileError> {
        if self.pos != toks.len() {
            return err(self.line, format!("unerwartetes Token '{}'", toks[self.pos].text));
        }
        Ok(())
    }

    // --- Bedingungen -------------------------------------------------------

    fn cond_or(&mut self) -> Result<Cond, CompileError> {
        let mut lhs = self.cond_and()?;
        while matches!(self.peek(), Some(t) if t.is_word("OR")) {
            self.pos += 1;
            let rhs = self.cond_and()?;
            lhs = Cond::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn cond_and(&mut self) -> Result<Cond, CompileError> {
        let mut lhs = self.cond_not()?;
        while matches!(self.peek(), Some(t) if t.is_word("AND")) {
            self.pos += 1;
            let rhs = self.cond_not()?;
            lhs = Cond::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn cond_not(&mut self) -> Result<Cond, CompileError> {
        if matches!(self.peek(), Some(t) if t.is_word("NOT")) {
            self.pos += 1;
            return Ok(Cond::Not(Box::new(self.cond_not()?)));
        }
        self.cond_atom()
    }

    fn cond_atom(&mut self) -> Result<Cond, CompileError> {
        // Geklammerte Bedingung versuchen; sonst als Vergleich mit Klammerausdruck lesen.
        if matches!(self.peek(), Some(t) if t.is_punct("(")) {
            let save = self.pos;
            self.pos += 1;
            if let Ok(c) = self.cond_or() {
                if matches!(self.peek(), Some(t) if t.is_punct(")")) {
                    self.pos += 1;
                    return Ok(c);
                }
            }
            self.pos = save;
        }
        let lhs = self.concat()?;
        let op = match self.peek() {
            Some(t) if t.is_punct("=") || t.is_word("EQ") => CmpOp::Eq,
            Some(t) if t.is_punct("<>") || t.is_word("NE") => CmpOp::Ne,
            Some(t) if t.is_punct("<") || t.is_word("LT") => CmpOp::Lt,
            Some(t) if t.is_punct(">") || t.is_word("GT") => CmpOp::Gt,
            Some(t) if t.is_punct("<=") || t.is_word("LE") => CmpOp::Le,
            Some(t) if t.is_punct(">=") || t.is_word("GE") => CmpOp::Ge,
            _ => return err(self.line, "Vergleichsoperator erwartet"),
        };
        self.pos += 1;
        let rhs = self.concat()?;
        Ok(Cond::Cmp(op, lhs, rhs))
    }

    // --- Ausdrücke ---------------------------------------------------------

    fn concat(&mut self) -> Result<Expr, CompileError> {
        let first = self.add()?;
        if !matches!(self.peek(), Some(t) if t.is_punct("&&")) {
            return Ok(first);
        }
        let mut parts = vec![first];
        while matches!(self.peek(), Some(t) if t.is_punct("&&")) {
            self.pos += 1;
            parts.push(self.add()?);
        }
        Ok(Expr::Concat(parts))
    }

    fn add(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(t) if t.is_punct("+") => BinOp::Add,
                Some(t) if t.is_punct("-") => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.mul()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn mul(&mut self) -> Result<Expr, CompileError> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(t) if t.is_punct("*") => BinOp::Mul,
                Some(t) if t.is_punct("/") => BinOp::Div,
                Some(t) if t.is_word("MOD") => BinOp::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.unary()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, CompileError> {
        if matches!(self.peek(), Some(t) if t.is_punct("-")) {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    /// Erkennt `name+off(len)` (Teilfeld); liefert das Ergebnis, wenn das Muster passt.
    fn try_slice(&mut self, name: &str) -> Result<Option<Expr>, CompileError> {
        let is_num = |t: Option<&Token>| t.map(|t| t.text.chars().all(|c| c.is_ascii_digit()) && !t.text.is_empty()) == Some(true);
        let matches_pattern = matches!(self.peek(), Some(t) if t.is_punct("+"))
            && is_num(self.peek_at(1))
            && matches!(self.peek_at(2), Some(t) if t.is_punct("("))
            && is_num(self.peek_at(3))
            && matches!(self.peek_at(4), Some(t) if t.is_punct(")"));
        if !matches_pattern {
            return Ok(None);
        }
        let off: usize = self.peek_at(1).unwrap().text.parse().or_else(|_| err(self.line, "Offset ungültig"))?;
        let len: usize = self.peek_at(3).unwrap().text.parse().or_else(|_| err(self.line, "Länge ungültig"))?;
        if len == 0 {
            return err(self.line, "Teilfeldlänge muss größer 0 sein");
        }
        self.pos += 5;
        Ok(Some(Expr::Slice { name: name.to_lowercase(), off, len }))
    }

    /// Parameterliste eines Methodenaufrufs. ABAP kennt keine Kommas: `p = a`, optional mit
    /// EXPORTING/IMPORTING/CHANGING/RECEIVING. `closing`: Liste endet mit `)`, sonst am Tokenende.
    fn arg_list(&mut self, closing: bool) -> Result<(Vec<NamedArg>, Option<Expr>), CompileError> {
        let mut args = Vec::new();
        let mut receiving: Option<Expr> = None;
        let mut changing: Option<bool> = None;
        let mut recv_mode = false;
        loop {
            let Some(t) = self.peek().cloned() else {
                if closing {
                    return err(self.line, "fehlende schließende Klammer im Aufruf");
                }
                break;
            };
            if closing && t.is_punct(")") {
                self.pos += 1;
                break;
            }
            if t.is_punct(",") {
                return err(self.line, "Komma ist in ABAP-Parameterlisten nicht erlaubt");
            }
            if t.is_word("EXPORTING") || t.is_word("IMPORTING") {
                self.pos += 1;
                changing = Some(false);
                recv_mode = false;
                continue;
            }
            if t.is_word("CHANGING") {
                self.pos += 1;
                changing = Some(true);
                recv_mode = false;
                continue;
            }
            if t.is_word("RECEIVING") {
                self.pos += 1;
                recv_mode = true;
                continue;
            }
            if t.is_word("EXCEPTIONS") {
                return err(self.line, "EXCEPTIONS wird noch nicht unterstützt");
            }
            let named = t.kind == Kind::Word && matches!(self.peek_at(1), Some(n) if n.is_punct("="));
            let name = if named {
                self.pos += 2;
                Some(t.text.to_lowercase())
            } else {
                None
            };
            let expr = self.concat()?;
            if recv_mode {
                if receiving.is_some() {
                    return err(self.line, "RECEIVING nur einmal erlaubt");
                }
                receiving = Some(expr);
            } else {
                args.push(NamedArg { name, changing, expr });
            }
        }
        Ok((args, receiving))
    }

    fn primary(&mut self) -> Result<Expr, CompileError> {
        let Some(t) = self.peek().cloned() else {
            return err(self.line, "Ausdruck endet unerwartet");
        };
        self.pos += 1;
        match t.kind {
            Kind::Punct if t.text == "(" => {
                let e = self.concat()?;
                match self.peek() {
                    Some(c) if c.is_punct(")") => {
                        self.pos += 1;
                        Ok(e)
                    }
                    _ => err(self.line, "fehlende schließende Klammer"),
                }
            }
            Kind::Literal => match literal_content(&t.text) {
                Some(s) => Ok(Expr::Lit(s)),
                None => err(self.line, format!("ungültiges Literal: {}", t.text)),
            },
            Kind::Word if t.text.chars().all(|c| c.is_ascii_digit()) => t
                .text
                .parse::<i64>()
                .map(Expr::Int)
                .or_else(|_| err(self.line, format!("Zahl zu groß: {}", t.text))),
            Kind::Word if matches!(self.peek(), Some(n) if n.is_punct("->")) => {
                self.pos += 1;
                let Some(a) = self.peek().cloned().filter(|a| a.kind == Kind::Word) else {
                    return err(self.line, "Attribut- oder Methodenname nach '->' erwartet");
                };
                self.pos += 1;
                if matches!(self.peek(), Some(p) if p.is_punct("(")) {
                    self.pos += 1;
                    let (args, receiving) = self.arg_list(true)?;
                    if receiving.is_some() {
                        return err(self.line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    return Ok(Expr::Call {
                        recv: t.text.to_lowercase(),
                        method: a.text.to_lowercase(),
                        args,
                    });
                }
                Ok(Expr::Attr { obj: t.text.to_lowercase(), attr: a.text.to_lowercase() })
            }
            Kind::Word => match self.try_slice(&t.text)? {
                Some(s) => Ok(s),
                None => Ok(Expr::Var(t.text.to_lowercase())),
            },
            _ => err(self.line, format!("unerwartetes Token '{}'", t.text)),
        }
    }
}

/// Parameterliste ohne Klammern (CALL METHOD ... EXPORTING ...). Liefert Argumente und RECEIVING-Ziel.
pub fn parse_call_args(toks: &[Token], line: usize) -> Result<(Vec<NamedArg>, Option<Expr>), CompileError> {
    let mut p = Parser { toks, pos: 0, line };
    let r = p.arg_list(false)?;
    p.finish(toks)?;
    Ok(r)
}
