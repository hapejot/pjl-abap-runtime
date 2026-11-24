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
    StaticCall {
        class: String,
        method: String,
        args: Vec<NamedArg>,
    },
    Lines(Box<Expr>),
    ExternalCall(String),
    Bool(Box<Cond>),
    StrLen(Box<Expr>),
    DynamicSlice {
        name: String,
        off: Box<Expr>,
        len: Box<Expr>,
    },
    /// Teilfeld `name+off(len)` (Offset 0-basiert, wie in ABAP)
    Slice { name: String, off: usize, len: usize },
    /// Verkettung mit `&&`
    Concat(Vec<Expr>),
    /// Verkettung wie CONCATENATE ... COMPRESSED: nachgestellte Leerzeichen der Operanden entfallen
    Compressed(Vec<Expr>),
    /// ABAP-String-Template mit gekennzeichneten Einbettungen
    Template(Vec<Expr>),
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
    BitAnd,
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
    Initial(Expr),
    Assigned(Expr),
    Bound(Expr),
    Supplied,
    BitSet(Expr, Expr),
    BitClear(Expr, Expr),
    In(Expr, Expr),
    Truthy(Expr),
    Between(Expr, Expr, Expr),
    Pattern {
        value: Expr,
        pattern: Expr,
        negated: bool,
    },
    Contains {
        value: Expr,
        needle: Expr,
        negated: bool,
    },
    ContainsAny {
        value: Expr,
        chars: Expr,
        negated: bool,
    },
    ContainsOnly {
        value: Expr,
        chars: Expr,
        negated: bool,
    },
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
    let end = toks
        .iter()
        .position(|token| token.is_punct("#"))
        .unwrap_or(toks.len());
    let toks = &toks[..end];
    let mut p = Parser { toks, pos: 0, line };
    let e = p.concat()?;
    p.finish(toks)?;
    Ok(e)
}

/// Parst eine Bedingung (IF, ELSEIF, WHILE, CHECK). Die gesamte Tokenfolge muss verbraucht werden.
pub fn parse_cond(toks: &[Token], line: usize) -> Result<Cond, CompileError> {
    let end = toks
        .iter()
        .position(|token| token.is_punct("#"))
        .unwrap_or(toks.len());
    let toks = &toks[..end];
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

fn parse_template(text: &str, line: usize) -> Result<Expr, CompileError> {
    let Some(inner) = text.strip_prefix('|').and_then(|value| value.strip_suffix('|')) else {
        return err(line, "String-Template unvollständig");
    };
    let chars: Vec<char> = inner.chars().collect();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' && index + 1 < chars.len() {
            index += 1;
            literal.push(chars[index]);
            index += 1;
            continue;
        }
        if chars[index] != '{' {
            literal.push(chars[index]);
            index += 1;
            continue;
        }
        if !literal.is_empty() {
            parts.push(Expr::Lit(std::mem::take(&mut literal)));
        }
        index += 1;
        let start = index;
        let mut depth = 1;
        while index < chars.len() && depth > 0 {
            match chars[index] {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            index += 1;
        }
        if depth != 0 {
            return err(line, "String-Template: '}' fehlt");
        }
        let mut expression: String = chars[start..index - 1].iter().collect();
        if let Some(format) = expression
            .to_uppercase()
            .find(" TIMESTAMP")
            .or_else(|| expression.to_uppercase().find(" DATE"))
            .or_else(|| expression.to_uppercase().find(" TIME"))
            .or_else(|| expression.to_uppercase().find(" ALPHA"))
            .or_else(|| expression.to_uppercase().find(" DECIMALS"))
            .or_else(|| expression.to_uppercase().find(" NUMBER"))
        {
            expression.truncate(format);
        }
        let tokens = abapgit_inspect::abap::tokenize(&expression);
        parts.push(parse_expr(&tokens, line)?);
    }
    if !literal.is_empty() || parts.is_empty() {
        parts.push(Expr::Lit(literal));
    }
    if parts.len() == 1 {
        Ok(parts.pop().unwrap())
    } else {
        Ok(Expr::Template(parts))
    }
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    line: usize,
}

fn simple_slice_expr(token: &Token, line: usize) -> Result<Expr, CompileError> {
    if token.is_punct("*") {
        return Ok(Expr::Int(-1));
    }
    match token.kind {
        Kind::Word if token.text.chars().all(|c| c.is_ascii_digit()) => token
            .text
            .parse::<i64>()
            .map(Expr::Int)
            .or_else(|_| err(line, "Teilfeldindex zu groß")),
        Kind::Word => Ok(Expr::Var(token.text.to_lowercase())),
        _ => err(line, "Teilfeldindex muss Zahl oder Feld sein"),
    }
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
        if matches!(self.peek(), Some(token) if token.is_word("O")) {
            self.pos += 1;
            let mask = self.concat()?;
            return Ok(Cond::BitSet(lhs, mask));
        }
        if matches!(self.peek(), Some(token) if token.is_word("IN")) {
            self.pos += 1;
            let values = self.concat()?;
            return Ok(Cond::In(lhs, values));
        }
        if matches!(self.peek(), Some(token) if token.is_word("NOT"))
            && matches!(self.peek_at(1), Some(token) if token.is_word("IN"))
        {
            self.pos += 2;
            let values = self.concat()?;
            return Ok(Cond::Not(Box::new(Cond::In(lhs, values))));
        }
        if self.peek().is_none()
            || matches!(self.peek(), Some(token) if token.is_word("AND") || token.is_word("OR") || token.is_punct(")"))
        {
            return Ok(Cond::Truthy(lhs));
        }
        if matches!(self.peek(), Some(token) if token.is_word("Z")) {
            self.pos += 1;
            let mask = self.concat()?;
            return Ok(Cond::BitClear(lhs, mask));
        }
        if matches!(self.peek(), Some(token) if token.is_word("BETWEEN")) {
            self.pos += 1;
            let low = self.concat()?;
            if !matches!(self.peek(), Some(token) if token.is_word("AND")) {
                return err(self.line, "BETWEEN erwartet AND");
            }
            self.pos += 1;
            let high = self.concat()?;
            return Ok(Cond::Between(lhs, low, high));
        }
        if matches!(self.peek(), Some(token) if token.is_word("NOT"))
            && matches!(self.peek_at(1), Some(token) if token.is_word("BETWEEN"))
        {
            self.pos += 2;
            let low = self.concat()?;
            if !matches!(self.peek(), Some(token) if token.is_word("AND")) {
                return err(self.line, "BETWEEN erwartet AND");
            }
            self.pos += 1;
            let high = self.concat()?;
            return Ok(Cond::Not(Box::new(Cond::Between(lhs, low, high))));
        }
        if matches!(self.peek(), Some(token) if token.is_word("CP") || token.is_word("NP")) {
            let negated = self.peek().is_some_and(|token| token.is_word("NP"));
            self.pos += 1;
            let pattern = self.concat()?;
            return Ok(Cond::Pattern {
                value: lhs,
                pattern,
                negated,
            });
        }
        if matches!(self.peek(), Some(token) if token.is_word("CS") || token.is_word("NS")) {
            let negated = self.peek().is_some_and(|token| token.is_word("NS"));
            self.pos += 1;
            let needle = self.concat()?;
            return Ok(Cond::Contains {
                value: lhs,
                needle,
                negated,
            });
        }
        if matches!(self.peek(), Some(token) if token.is_word("CA") || token.is_word("NA")) {
            let negated = self.peek().is_some_and(|token| token.is_word("NA"));
            self.pos += 1;
            let chars = self.concat()?;
            return Ok(Cond::ContainsAny {
                value: lhs,
                chars,
                negated,
            });
        }
        if matches!(self.peek(), Some(token) if token.is_word("CO") || token.is_word("CN")) {
            let negated = self.peek().is_some_and(|token| token.is_word("CN"));
            self.pos += 1;
            let chars = self.concat()?;
            return Ok(Cond::ContainsOnly {
                value: lhs,
                chars,
                negated,
            });
        }
        if matches!(self.peek(), Some(token) if token.is_word("IS")) {
            self.pos += 1;
            let negate = matches!(self.peek(), Some(token) if token.is_word("NOT"));
            if negate {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(token) if token.is_word("INITIAL")) {
                if matches!(self.peek(), Some(token) if token.is_word("ASSIGNED")) {
                    self.pos += 1;
                    let condition = Cond::Assigned(lhs);
                    return Ok(if negate {
                        Cond::Not(Box::new(condition))
                    } else {
                        condition
                    });
                }
                if matches!(self.peek(), Some(token) if token.is_word("BOUND")) {
                    self.pos += 1;
                    let condition = Cond::Bound(lhs);
                    return Ok(if negate {
                        Cond::Not(Box::new(condition))
                    } else {
                        condition
                    });
                }
                if matches!(self.peek(), Some(token) if token.is_word("SUPPLIED")) {
                    self.pos += 1;
                    let condition = Cond::Supplied;
                    return Ok(if negate {
                        Cond::Not(Box::new(condition))
                    } else {
                        condition
                    });
                }
                return err(self.line, "IS erwartet INITIAL, ASSIGNED, BOUND oder SUPPLIED");
            }
            self.pos += 1;
            let condition = Cond::Initial(lhs);
            return Ok(if negate {
                Cond::Not(Box::new(condition))
            } else {
                condition
            });
        }
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
        if !matches!(self.peek(), Some(t) if t.is_punct("&&") || t.is_punct("&")) {
            return Ok(first);
        }
        let mut parts = vec![first];
        while matches!(self.peek(), Some(t) if t.is_punct("&&") || t.is_punct("&")) {
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
                Some(t) if t.is_word("DIV") => BinOp::Div,
                Some(t) if t.is_word("MOD") => BinOp::Mod,
                Some(t) if t.is_word("BIT-AND") => BinOp::BitAnd,
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
        let matches_pattern = matches!(self.peek(), Some(t) if t.is_punct("+"))
            && matches!(self.peek_at(2), Some(t) if t.is_punct("("))
            && matches!(self.peek_at(4), Some(t) if t.is_punct(")"));
        if !matches_pattern {
            return Ok(None);
        }
        let off_token = self.peek_at(1).unwrap();
        let len_token = self.peek_at(3).unwrap();
        let off = simple_slice_expr(off_token, self.line)?;
        let len = simple_slice_expr(len_token, self.line)?;
        self.pos += 5;
        Ok(Some(match (off, len) {
            (Expr::Int(off), Expr::Int(len)) => {
                if len == 0 {
                    return err(self.line, "Teilfeldlänge muss größer 0 sein");
                }
                Expr::Slice {
                    name: name.to_lowercase(),
                    off: off as usize,
                    len: len as usize,
                }
            }
            (off, len) => Expr::DynamicSlice {
                name: name.to_lowercase(),
                off: Box::new(off),
                len: Box::new(len),
            },
        }))
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
                self.pos = self.toks.len();
                break;
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
            Kind::Template => parse_template(&t.text, self.line),
            Kind::Word if t.text.chars().all(|c| c.is_ascii_digit()) => t
                .text
                .parse::<i64>()
                .map(Expr::Int)
                .or_else(|_| err(self.line, format!("Zahl zu groß: {}", t.text))),
            Kind::Word
                if matches!(self.peek(), Some(n) if n.is_punct("=>") || n.is_punct("~")) =>
            {
                let separator = self.peek().cloned().unwrap();
                self.pos += 1;
                let Some(mut name) = self.peek().cloned().filter(|name| name.kind == Kind::Word) else {
                    return err(self.line, "Konstantenname nach '=>' erwartet");
                };
                self.pos += 1;
                if matches!(self.peek(), Some(token) if token.is_punct("~")) {
                    self.pos += 1;
                    let method = self
                        .peek()
                        .filter(|token| token.kind == Kind::Word)
                        .ok_or_else(|| CompileError {
                            line: self.line,
                            message: "Interface-Methode nach '~' erwartet".into(),
                        })?;
                    name.text = format!("{}~{}", name.text, method.text);
                    self.pos += 1;
                }
                if matches!(self.peek(), Some(token) if token.is_punct("-"))
                    && matches!(self.peek_at(1), Some(token) if token.kind == Kind::Word)
                {
                    name.text.push('-');
                    name.text.push_str(&self.peek_at(1).unwrap().text);
                    self.pos += 2;
                }
                let separator = if separator.is_punct("~") { "~" } else { "=>" };
                if matches!(self.peek(), Some(p) if p.is_punct("(")) {
                    self.pos += 1;
                    let (args, receiving) = self.arg_list(true)?;
                    if receiving.is_some() {
                        return err(self.line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    let method = if separator == "~" {
                            format!("{}~{}", t.text.to_lowercase(), name.text.to_lowercase())
                        } else {
                            name.text.to_lowercase()
                        };
                    if matches!(self.peek(), Some(token) if token.is_punct("->")) {
                        let mut chain = format!(
                            "{}=>{}",
                            t.text.to_lowercase(),
                            method
                        );
                        while matches!(self.peek(), Some(token) if token.is_punct("->")) {
                            self.pos += 1;
                            let chained = self
                                .peek()
                                .filter(|token| token.kind == Kind::Word)
                                .map(|token| token.text.to_lowercase())
                                .ok_or_else(|| CompileError {
                                    line: self.line,
                                    message: "Methodenname nach '->' erwartet".into(),
                                })?;
                            self.pos += 1;
                            chain.push_str("->");
                            chain.push_str(&chained);
                            if matches!(self.peek(), Some(token) if token.is_punct("(")) {
                                self.pos += 1;
                                let _ = self.arg_list(true)?;
                            }
                        }
                        return Ok(Expr::ExternalCall(chain));
                    }
                    return Ok(Expr::StaticCall {
                        class: t.text.to_lowercase(),
                        method,
                        args,
                    });
                }
                if separator == "~" {
                    Ok(Expr::Attr {
                        obj: "me".into(),
                        attr: format!(
                            "{}~{}",
                            t.text.to_lowercase(),
                            name.text.to_lowercase()
                        ),
                    })
                } else {
                    Ok(Expr::Var(format!(
                        "{}=>{}",
                        t.text.to_lowercase(),
                        name.text.to_lowercase()
                    )))
                }
            }
            Kind::Word
                if t.text.starts_with('<')
                    && t.text.ends_with('>')
                    && matches!(self.peek(), Some(token) if token.is_punct("(")) =>
            {
                self.pos += 1;
                let len = self
                    .peek()
                    .ok_or_else(|| CompileError {
                        line: self.line,
                        message: "Teilfeldlänge erwartet".into(),
                    })?
                    .clone();
                self.pos += 1;
                if !matches!(self.peek(), Some(token) if token.is_punct(")")) {
                    return err(self.line, "Teilfeld: ')' erwartet");
                }
                self.pos += 1;
                let Expr::Int(len) = simple_slice_expr(&len, self.line)? else {
                    return err(self.line, "dynamisches Teilfeld als Feldsymbol wird extern behandelt");
                };
                Ok(Expr::Slice {
                    name: t.text.to_lowercase(),
                    off: 0,
                    len: len as usize,
                })
            }
            Kind::Word
                if matches!(self.peek(), Some(token) if token.is_punct("["))
                    && matches!(self.peek_at(1), Some(token) if token.is_punct("]")) =>
            {
                self.pos += 2;
                Ok(Expr::Var(t.text.to_lowercase()))
            }
            Kind::Word if matches!(self.peek(), Some(token) if token.is_punct("(")) => {
                self.pos += 1;
                if t.is_word("boolc") {
                    let condition = self.cond_or()?;
                    if !matches!(self.peek(), Some(token) if token.is_punct(")")) {
                        return err(self.line, "boolc: ')' erwartet");
                    }
                    self.pos += 1;
                    return Ok(Expr::Bool(Box::new(condition)));
                }
                if t.is_word("strlen") {
                    let value = self.concat()?;
                    if !matches!(self.peek(), Some(token) if token.is_punct(")")) {
                        return err(self.line, "strlen: ')' erwartet");
                    }
                    self.pos += 1;
                    return Ok(Expr::StrLen(Box::new(value)));
                }
                let (mut args, receiving) = self.arg_list(true)?;
                if receiving.is_some() {
                    return err(self.line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                }
                if t.is_word("lines") && args.len() == 1 && args[0].name.is_none() {
                    return Ok(Expr::Lines(Box::new(args.remove(0).expr)));
                }
                if matches!(self.peek(), Some(token) if token.is_punct("->")) {
                    let mut chain = t.text.to_lowercase();
                    while matches!(self.peek(), Some(token) if token.is_punct("->")) {
                        self.pos += 1;
                        let next = self
                            .peek()
                            .filter(|token| token.kind == Kind::Word)
                            .map(|token| token.text.to_lowercase())
                            .ok_or_else(|| CompileError {
                                line: self.line,
                                message: "Methodenname nach '->' erwartet".into(),
                            })?;
                        self.pos += 1;
                        chain.push_str("->");
                        chain.push_str(&next);
                        if matches!(self.peek(), Some(token) if token.is_punct("(")) {
                            self.pos += 1;
                            let _ = self.arg_list(true)?;
                        }
                    }
                    return Ok(Expr::ExternalCall(chain));
                }
                Ok(Expr::Call {
                    recv: "me".into(),
                    method: t.text.to_lowercase(),
                    args,
                })
            }
            Kind::Word if matches!(self.peek(), Some(n) if n.is_punct("->")) => {
                self.pos += 1;
                if matches!(self.peek(), Some(token) if token.is_punct("*")) {
                    self.pos += 1;
                    return Ok(Expr::Attr {
                        obj: t.text.to_lowercase(),
                        attr: "*".into(),
                    });
                }
                let Some(a) = self.peek().cloned().filter(|a| a.kind == Kind::Word) else {
                    return err(self.line, "Attribut- oder Methodenname nach '->' erwartet");
                };
                self.pos += 1;
                let method_name = if matches!(self.peek(), Some(token) if token.is_punct("~")) {
                    self.pos += 1;
                    let method = self
                        .peek()
                        .filter(|token| token.kind == Kind::Word)
                        .ok_or_else(|| CompileError {
                            line: self.line,
                            message: "Interface-Methode nach '~' erwartet".into(),
                        })?
                        .text
                        .to_lowercase();
                    self.pos += 1;
                    format!("{}~{}", a.text.to_lowercase(), method)
                } else {
                    a.text.to_lowercase()
                };
                if matches!(self.peek(), Some(p) if p.is_punct("(")) {
                    self.pos += 1;
                    let (args, receiving) = self.arg_list(true)?;
                    if receiving.is_some() {
                        return err(self.line, "RECEIVING ist nur bei CALL METHOD erlaubt");
                    }
                    if matches!(self.peek(), Some(token) if token.is_punct("->")) {
                        let mut chain = format!(
                            "{}->{}",
                            t.text.to_lowercase(),
                            method_name
                        );
                        while matches!(self.peek(), Some(token) if token.is_punct("->")) {
                            self.pos += 1;
                            let next = self
                                .peek()
                                .filter(|token| token.kind == Kind::Word)
                                .map(|token| token.text.to_lowercase())
                                .ok_or_else(|| CompileError {
                                    line: self.line,
                                    message: "Methodenname nach '->' erwartet".into(),
                                })?;
                            self.pos += 1;
                            chain.push_str("->");
                            chain.push_str(&next);
                            if matches!(self.peek(), Some(token) if token.is_punct("(")) {
                                self.pos += 1;
                                let _ = self.arg_list(true)?;
                            }
                        }
                        return Ok(Expr::ExternalCall(chain));
                    }
                    return Ok(Expr::Call {
                        recv: t.text.to_lowercase(),
                        method: method_name,
                        args,
                    });
                }
                Ok(Expr::Attr {
                    obj: t.text.to_lowercase(),
                    attr: method_name,
                })
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
