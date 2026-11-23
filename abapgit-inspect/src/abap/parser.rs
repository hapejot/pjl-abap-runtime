//! Parser: Statements, Blöcke, Klassen, Routinen.

use tracing::trace;

use super::lexer::{tokenize, Kind, Token};

#[derive(Debug, Clone)]
pub struct Statement {
    pub tokens: Vec<Token>,
    pub line: usize,
}

impl Statement {
    /// Erstes Wort in Großbuchstaben (z.B. `METHODS`, `CLASS-DATA`).
    pub fn keyword(&self) -> String {
        self.tokens
            .first()
            .map(|t| t.text.to_uppercase())
            .unwrap_or_default()
    }

    pub fn has(&self, kw: &str) -> bool {
        self.tokens.iter().any(|t| t.is_word(kw))
    }

    pub fn text(&self) -> String {
        self.tokens
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Namen nach `->` bzw. `=>` (Methodenaufrufe, Attributzugriffe), kleingeschrieben.
    pub fn method_refs(&self) -> Vec<String> {
        self.tokens
            .windows(2)
            .filter(|w| (w[0].is_punct("->") || w[0].is_punct("=>")) && w[1].kind == Kind::Word)
            .map(|w| w[1].text.to_lowercase())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum Item {
    Statement(Statement),
    /// Block wie IF/ENDIF, LOOP/ENDLOOP; Zwischenschlüsselwörter (ELSE, WHEN, CATCH) bleiben Statements im Body.
    Block {
        head: Statement,
        body: Vec<Item>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: String,
    pub is_class: bool,
    /// (Abschnitt wie IMPORTING, Rohtext der Parameter)
    pub sections: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum MemberKind {
    Method(MethodSig),
    Data,
    Types,
    Constants,
    Interfaces,
    Aliases,
    Events,
    Other,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub visibility: Visibility,
    pub kind: MemberKind,
    pub statement: Statement,
}

#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    pub head: Statement,
    pub body: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    Definition,
    Implementation,
    Interface,
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub name: String,
    pub kind: ClassKind,
    pub head: Statement,
    /// Deklarationen (nur Definition/Interface)
    pub members: Vec<Member>,
    /// Methodenimplementierungen (nur Implementation)
    pub methods: Vec<Method>,
    /// Sonstige Statements im Implementation-Teil
    pub other: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum TopItem {
    Statement(Statement),
    /// Block auf Programmebene (z.B. SELECT ... ENDSELECT)
    Block {
        head: Statement,
        body: Vec<Item>,
    },
    Class(ClassDef),
    /// FORM, FUNCTION, MODULE oder Ereignisblock (START-OF-SELECTION usw.)
    Routine {
        kind: String,
        name: String,
        head: Statement,
        body: Vec<Item>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub items: Vec<TopItem>,
    pub errors: Vec<ParseError>,
}

struct Cursor {
    stmts: Vec<Statement>,
    pos: usize,
    errors: Vec<ParseError>,
}

impl Cursor {
    fn peek(&self) -> Option<&Statement> {
        self.stmts.get(self.pos)
    }

    fn error(&mut self, line: usize, msg: impl Into<String>) {
        self.errors.push(ParseError {
            line,
            message: msg.into(),
        });
    }
}

fn name_at(s: &Statement, idx: usize) -> String {
    s.tokens
        .get(idx)
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

/// END-Schlüsselwort, das einen Block schließt (ENDIF, ENDMETHOD, ...). END-OF-SELECTION zählt nicht.
fn is_end(s: &Statement) -> bool {
    let k = s.keyword();
    k.starts_with("END") && !k.starts_with("END-") && k != "END"
}

fn is_event(s: &Statement) -> bool {
    match s.keyword().as_str() {
        "START-OF-SELECTION" | "END-OF-SELECTION" | "INITIALIZATION" | "LOAD-OF-PROGRAM"
        | "TOP-OF-PAGE" | "END-OF-PAGE" | "GET" => true,
        "AT" => matches!(
            name_at(s, 1).to_uppercase().as_str(),
            "SELECTION-SCREEN" | "LINE-SELECTION" | "USER-COMMAND"
        ),
        _ => false,
    }
}

fn is_top_starter(s: &Statement) -> bool {
    is_event(s)
        || matches!(
            s.keyword().as_str(),
            "CLASS" | "INTERFACE" | "FORM" | "FUNCTION" | "MODULE"
        )
}

fn never(_: &Statement) -> bool {
    false
}

/// Liefert das Schlussschlüsselwort, falls das Statement einen Block eröffnet.
fn block_closer(s: &Statement) -> Option<&'static str> {
    match s.keyword().as_str() {
        "IF" => Some("ENDIF"),
        "LOOP" => Some("ENDLOOP"),
        "DO" => Some("ENDDO"),
        "WHILE" => Some("ENDWHILE"),
        "CASE" => Some("ENDCASE"),
        "TRY" => Some("ENDTRY"),
        "SELECT" if !s.has("SINGLE") && !s.has("TABLE") => Some("ENDSELECT"),
        "AT" if matches!(
            name_at(s, 1).to_uppercase().as_str(),
            "NEW" | "END" | "FIRST" | "LAST"
        ) =>
        {
            Some("ENDAT")
        }
        "ON" if name_at(s, 1).eq_ignore_ascii_case("CHANGE") => Some("ENDON"),
        "CATCH" if s.has("SYSTEM-EXCEPTIONS") => Some("ENDCATCH"),
        "CHAIN" => Some("ENDCHAIN"),
        _ => None,
    }
}

/// Teilt Tokens an `.` in Statements; Chaining (`DATA: a, b.`) wird aufgelöst.
fn split_statements(tokens: Vec<Token>) -> Vec<Statement> {
    let mut out = Vec::new();
    let mut cur: Vec<Token> = Vec::new();
    for t in tokens {
        if t.is_punct(".") {
            push_statement(std::mem::take(&mut cur), &mut out);
        } else {
            cur.push(t);
        }
    }
    push_statement(cur, &mut out);
    out
}

fn push_statement(cur: Vec<Token>, out: &mut Vec<Statement>) {
    if cur.is_empty() {
        return;
    }
    if cur.len() > 2 && cur[0].kind == Kind::Word && cur[1].is_punct(":") {
        let kw = cur[0].clone();
        let mut depth = 0i32;
        let mut piece: Vec<Token> = Vec::new();
        for t in &cur[2..] {
            if t.is_punct("(") {
                depth += 1;
            }
            if t.is_punct(")") {
                depth -= 1;
            }
            if depth == 0 && t.is_punct(",") {
                emit_piece(&kw, std::mem::take(&mut piece), out);
            } else {
                piece.push(t.clone());
            }
        }
        emit_piece(&kw, piece, out);
    } else {
        let line = cur[0].line;
        out.push(Statement { tokens: cur, line });
    }
}

fn emit_piece(kw: &Token, piece: Vec<Token>, out: &mut Vec<Statement>) {
    if piece.is_empty() {
        return;
    }
    let line = piece[0].line;
    let mut tokens = vec![kw.clone()];
    tokens.extend(piece);
    out.push(Statement { tokens, line });
}

/// Parst Items bis END-Schlüsselwort, `until` oder EOF.
fn parse_items(c: &mut Cursor, until: &dyn Fn(&Statement) -> bool) -> Vec<Item> {
    let mut items = Vec::new();
    while let Some(s) = c.peek().cloned() {
        if is_end(&s) || until(&s) {
            break;
        }
        c.pos += 1;
        match block_closer(&s) {
            Some(closer) => {
                let body = parse_items(c, &never);
                take_closer(c, closer);
                items.push(Item::Block { head: s, body });
            }
            None => items.push(Item::Statement(s)),
        }
    }
    items
}

/// Konsumiert das erwartete END-Statement und meldet Abweichungen.
fn take_closer(c: &mut Cursor, closer: &str) {
    match c.peek().cloned() {
        Some(e) if e.keyword() == closer => c.pos += 1,
        Some(e) if is_end(&e) => {
            c.error(
                e.line,
                format!("{closer} erwartet, {} gefunden", e.keyword()),
            );
            c.pos += 1;
        }
        Some(e) => c.error(e.line, format!("{closer} fehlt")),
        None => c.error(0, format!("{closer} fehlt (Dateiende)")),
    }
}

fn parse_routine(
    c: &mut Cursor,
    head: Statement,
    closer: Option<&str>,
    until: &dyn Fn(&Statement) -> bool,
) -> TopItem {
    let kind = head.keyword();
    let mut name = name_at(&head, 1);
    if name.is_empty() {
        name = kind.clone();
    }
    let body = parse_items(c, until);
    if let Some(cl) = closer {
        take_closer(c, cl);
    }
    TopItem::Routine {
        kind,
        name,
        head,
        body,
    }
}

fn parse_class(c: &mut Cursor, head: Statement, kind: ClassKind) -> ClassDef {
    let name = name_at(&head, 1);
    let line = head.line;
    let end_kw = if kind == ClassKind::Interface {
        "ENDINTERFACE"
    } else {
        "ENDCLASS"
    };
    let mut def = ClassDef {
        name,
        kind,
        head,
        members: Vec::new(),
        methods: Vec::new(),
        other: Vec::new(),
    };
    let mut vis = Visibility::Public;

    loop {
        let Some(s) = c.peek().cloned() else {
            c.error(line, format!("{end_kw} fehlt (Dateiende)"));
            break;
        };
        c.pos += 1;
        if s.keyword() == end_kw {
            break;
        }
        if is_end(&s) {
            c.error(s.line, format!("unerwartet {}", s.keyword()));
            continue;
        }
        let kw = s.keyword();

        if kind != ClassKind::Implementation
            && matches!(kw.as_str(), "PUBLIC" | "PROTECTED" | "PRIVATE")
            && s.tokens.len() <= 2
        {
            vis = match kw.as_str() {
                "PUBLIC" => Visibility::Public,
                "PROTECTED" => Visibility::Protected,
                _ => Visibility::Private,
            };
            continue;
        }

        if kind == ClassKind::Implementation {
            if kw == "METHOD" {
                let mname = name_at(&s, 1);
                let body = parse_items(c, &never);
                take_closer(c, "ENDMETHOD");
                def.methods.push(Method {
                    name: mname,
                    head: s,
                    body,
                });
            } else {
                def.other.push(Item::Statement(s));
            }
            continue;
        }

        let mkind = classify_member(&s);
        def.members.push(Member {
            visibility: vis,
            kind: mkind,
            statement: s,
        });
    }
    def
}

fn classify_member(s: &Statement) -> MemberKind {
    match s.keyword().as_str() {
        "METHODS" | "CLASS-METHODS" => MemberKind::Method(parse_sig(s)),
        "DATA" | "CLASS-DATA" => MemberKind::Data,
        "TYPES" => MemberKind::Types,
        "CONSTANTS" => MemberKind::Constants,
        "INTERFACES" => MemberKind::Interfaces,
        "ALIASES" => MemberKind::Aliases,
        "EVENTS" | "CLASS-EVENTS" => MemberKind::Events,
        _ => MemberKind::Other,
    }
}

fn parse_sig(s: &Statement) -> MethodSig {
    let is_class = s.keyword().starts_with("CLASS");
    let name = name_at(s, 1);
    let mut sections: Vec<(String, String)> = Vec::new();
    for t in s.tokens.iter().skip(2) {
        let up = t.text.to_uppercase();
        let is_section = t.kind == Kind::Word
            && matches!(
                up.as_str(),
                "IMPORTING" | "EXPORTING" | "CHANGING" | "RETURNING" | "RAISING" | "EXCEPTIONS"
            );
        if is_section {
            sections.push((up, String::new()));
        } else if let Some(last) = sections.last_mut() {
            if !last.1.is_empty() {
                last.1.push(' ');
            }
            last.1.push_str(&t.text);
        }
    }
    MethodSig {
        name,
        is_class,
        sections,
    }
}

/// Parst ABAP-Quelltext in eine Struktur aus Klassen, Routinen und Statements.
pub fn parse(src: &str) -> Program {
    let mut c = Cursor {
        stmts: split_statements(tokenize(src)),
        pos: 0,
        errors: Vec::new(),
    };
    for s in c.stmts.iter() {
        trace!("{:?}", s);
    }
    let mut items = Vec::new();

    while let Some(s) = c.peek().cloned() {
        c.pos += 1;
        if is_end(&s) {
            c.error(s.line, format!("unerwartet {}", s.keyword()));
            items.push(TopItem::Statement(s));
            continue;
        }
        let kw = s.keyword();
        let deferred = s.has("DEFERRED") || s.has("LOAD");

        let item = match kw.as_str() {
            "CLASS" if s.has("IMPLEMENTATION") => {
                TopItem::Class(parse_class(&mut c, s, ClassKind::Implementation))
            }
            "CLASS" if s.has("DEFINITION") && !deferred => {
                TopItem::Class(parse_class(&mut c, s, ClassKind::Definition))
            }
            "INTERFACE" if !deferred => {
                TopItem::Class(parse_class(&mut c, s, ClassKind::Interface))
            }
            "FORM" => parse_routine(&mut c, s, Some("ENDFORM"), &never),
            "FUNCTION" => parse_routine(&mut c, s, Some("ENDFUNCTION"), &never),
            "MODULE" if s.has("OUTPUT") || s.has("INPUT") => {
                parse_routine(&mut c, s, Some("ENDMODULE"), &never)
            }
            _ if is_event(&s) => parse_routine(&mut c, s, None, &is_top_starter),
            _ => match block_closer(&s) {
                Some(closer) => {
                    let body = parse_items(&mut c, &never);
                    take_closer(&mut c, closer);
                    TopItem::Block { head: s, body }
                }
                None => TopItem::Statement(s),
            },
        };
        items.push(item);
    }

    Program {
        items,
        errors: c.errors,
    }
}
