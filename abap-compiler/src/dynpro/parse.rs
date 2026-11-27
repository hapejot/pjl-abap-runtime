//! Parser für `.dynp` nach Spezifikation 4.1 und 4.2: zeilenorientiert, eine Anweisung pro Zeile.
//! Der erste Syntaxfehler wird mit Datei, Zeile und Spalte gemeldet. Prüfungen gegen das Programm stehen in `check.rs`.

use super::*;

/// Token einer `.dynp`-Zeile. Zeichenketten sind ohne Hochkommas gespeichert, Hochkommas doppelt entschlüsselt.
#[derive(Debug, Clone)]
struct Tok {
    text: String,
    quoted: bool,
    col: usize,
}

impl Tok {
    /// Schlüsselwort ohne Groß-/Kleinschreibung; Zeichenketten sind nie ein Schlüsselwort.
    fn is_kw(&self, kw: &str) -> bool {
        !self.quoted && self.text.eq_ignore_ascii_case(kw)
    }

    /// Spalte direkt hinter dem Token.
    fn end_col(&self) -> usize {
        self.col + self.text.chars().count() + if self.quoted { 2 } else { 0 }
    }
}

/// Zerlegt eine Zeile. `*` am Zeilenanfang und `"` außerhalb einer Zeichenkette beginnen einen Kommentar.
fn tokenize(file: &str, line: usize, text: &str) -> Result<Vec<Tok>, DynproError> {
    let chars: Vec<char> = text.chars().collect();
    if chars.iter().find(|c| !c.is_whitespace()) == Some(&'*') {
        return Ok(Vec::new());
    }
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            break;
        }
        let col = i + 1;
        if c == '\'' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => {
                        return Err(DynproError::error(
                            file,
                            line,
                            col,
                            "Unterminated string literal",
                        ))
                    }
                    Some('\'') if chars.get(i + 1) == Some(&'\'') => {
                        s.push('\'');
                        i += 2;
                    }
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            toks.push(Tok {
                text: s,
                quoted: true,
                col,
            });
        } else {
            let start = i;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && chars[i] != '\''
                && chars[i] != '"'
            {
                i += 1;
            }
            toks.push(Tok {
                text: chars[start..i].iter().collect(),
                quoted: false,
                col,
            });
        }
    }
    Ok(toks)
}

/// Erstes Schlüsselwort einer Zeile in Großbuchstaben; leer bei einer Zeichenkette am Zeilenanfang.
fn keyword(toks: &[Tok]) -> String {
    match toks.first() {
        Some(t) if !t.quoted => t.text.to_uppercase(),
        _ => String::new(),
    }
}

/// Spalte für eine Fehlermeldung: das Token an `idx`, sonst die Stelle hinter dem letzten Token.
fn col_at(toks: &[Tok], idx: usize) -> usize {
    match toks.get(idx) {
        Some(t) => t.col,
        None => toks.last().map_or(1, Tok::end_col),
    }
}

/// Beschreibung des Tokens an `idx` für Fehlermeldungen.
fn found(toks: &[Tok], idx: usize) -> String {
    match toks.get(idx) {
        Some(t) => format!("'{}'", t.text),
        None => "end of line".to_string(),
    }
}

/// Offene `CHAIN` beim Lesen der Ablauflogik: Kettennummer, Anzahl der Felder, ob schon ein Modul steht.
struct OpenChain {
    number: usize,
    fields: usize,
    module: bool,
}

/// Die Zeilen der Datei mit Zeilennummer, leere und Kommentarzeilen entfernt.
struct Lines<'a> {
    file: &'a str,
    items: Vec<(usize, Vec<Tok>)>,
    pos: usize,
}

impl<'a> Lines<'a> {
    fn take(&mut self) -> Option<(usize, Vec<Tok>)> {
        let item = self.items.get(self.pos).cloned();
        self.pos += 1;
        item
    }

    fn fail<T>(
        &self,
        line: usize,
        toks: &[Tok],
        idx: usize,
        msg: impl Into<String>,
    ) -> Result<T, DynproError> {
        Err(DynproError::error(self.file, line, col_at(toks, idx), msg))
    }

    /// Ende der Datei, obwohl ein Block noch offen ist.
    fn eof(&self, expected: &str) -> DynproError {
        let (line, col) = self
            .items
            .last()
            .map_or((1, 1), |(l, t)| (*l, t.last().map_or(1, Tok::end_col)));
        DynproError::error(
            self.file,
            line,
            col,
            format!("Syntax error: end of file, expected {expected}"),
        )
    }

    /// Ungeprüfte Reste der Zeile sind ein Fehler.
    fn expect_end(&self, line: usize, toks: &[Tok], len: usize) -> Result<(), DynproError> {
        if toks.len() > len {
            return self.fail(
                line,
                toks,
                len,
                format!("Syntax error: unexpected {}", found(toks, len)),
            );
        }
        Ok(())
    }

    /// Unquotiertes Wort (Name, Funktionscode).
    fn expect_word(
        &self,
        line: usize,
        toks: &[Tok],
        idx: usize,
        what: &str,
    ) -> Result<String, DynproError> {
        match toks.get(idx) {
            Some(t) if !t.quoted && !t.text.is_empty() => Ok(t.text.clone()),
            _ => self.fail(
                line,
                toks,
                idx,
                format!("Syntax error: expected {what}, found {}", found(toks, idx)),
            ),
        }
    }

    /// Zeichenkette in Hochkommas.
    fn expect_text(
        &self,
        line: usize,
        toks: &[Tok],
        idx: usize,
        what: &str,
    ) -> Result<String, DynproError> {
        match toks.get(idx) {
            Some(t) if t.quoted => Ok(t.text.clone()),
            _ => self.fail(
                line,
                toks,
                idx,
                format!(
                    "Syntax error: expected {what} in single quotes, found {}",
                    found(toks, idx)
                ),
            ),
        }
    }

    /// Nicht negative ganze Zahl (Zeilen, Spalten, Dynpro-Nummern, Tastennummern).
    fn expect_number(
        &self,
        line: usize,
        toks: &[Tok],
        idx: usize,
        what: &str,
    ) -> Result<usize, DynproError> {
        match toks.get(idx) {
            Some(t)
                if !t.quoted
                    && !t.text.is_empty()
                    && t.text.chars().all(|c| c.is_ascii_digit()) =>
            {
                t.text.parse::<usize>().or_else(|_| {
                    self.fail(
                        line,
                        toks,
                        idx,
                        format!("Number too large for {what}: {}", t.text),
                    )
                })
            }
            _ => self.fail(
                line,
                toks,
                idx,
                format!("Syntax error: expected {what}, found {}", found(toks, idx)),
            ),
        }
    }

    /// Farbe 1 bis 7 (Spec 8.3).
    fn expect_color(&self, line: usize, toks: &[Tok], idx: usize) -> Result<u8, DynproError> {
        let n = self.expect_number(line, toks, idx, "color number")?;
        if (1..=7).contains(&n) {
            Ok(n as u8)
        } else {
            self.fail(line, toks, idx, format!("Color must be 1 to 7, found {n}"))
        }
    }

    fn dynpro(&mut self, line: usize, head: &[Tok]) -> Result<Dynpro, DynproError> {
        let raw = self.expect_number(line, head, 1, "dynpro number")?;
        if !(1..=9999).contains(&raw) {
            return self.fail(
                line,
                head,
                1,
                format!("Dynpro number must be 1 to 9999, found {raw}"),
            );
        }
        let number = raw as u32;
        let mut idx = 2;
        let mut modal = false;
        if head.get(2).is_some_and(|t| t.is_kw("KIND")) {
            modal = match head.get(3) {
                Some(t) if t.is_kw("NORMAL") => false,
                Some(t) if t.is_kw("MODAL") => true,
                _ => {
                    return self.fail(
                        line,
                        head,
                        3,
                        format!(
                            "Syntax error: expected NORMAL or MODAL, found {}",
                            found(head, 3)
                        ),
                    )
                }
            };
            idx = 4;
        }
        self.expect_end(line, head, idx)?;

        let mut d = Dynpro {
            number,
            line,
            modal,
            rows: 20,
            cols: 80,
            title: String::new(),
            elements: Vec::new(),
            flow: Vec::new(),
            value_requests: Vec::new(),
        };
        let mut seen_flow = false;
        loop {
            let Some((ln, toks)) = self.take() else {
                return Err(self.eof("ENDDYNPRO"));
            };
            match keyword(&toks).as_str() {
                "ENDDYNPRO" => {
                    self.expect_end(ln, &toks, 1)?;
                    return Ok(d);
                }
                "SIZE" => {
                    d.rows = self.expect_number(ln, &toks, 1, "row count")?;
                    d.cols = self.expect_number(ln, &toks, 2, "column count")?;
                    self.expect_end(ln, &toks, 3)?;
                }
                "TITLE" => {
                    d.title = self.expect_text(ln, &toks, 1, "title")?;
                    self.expect_end(ln, &toks, 2)?;
                }
                "STATUS" => {
                    return Err(DynproError::error(
                        self.file,
                        ln,
                        1,
                        "STATUS in DYNPRO is not supported; call SET PF-STATUS in a PBO module"
                            .to_string(),
                    ))
                }
                "TEXT" => d.elements.push(self.text_element(ln, &toks)?),
                "FIELD" => d.elements.push(self.field_element(ln, &toks)?),
                "BUTTON" => d.elements.push(self.button_element(ln, &toks)?),
                "FRAME" => d.elements.push(self.frame_element(ln, &toks)?),
                "FLOW" => {
                    if seen_flow {
                        return self.fail(ln, &toks, 0, "FLOW may appear only once in a DYNPRO");
                    }
                    seen_flow = true;
                    self.expect_end(ln, &toks, 1)?;
                    let (flow, value_requests) = self.flow()?;
                    d.flow = flow;
                    d.value_requests = value_requests;
                }
                _ => {
                    return self.fail(
                        ln,
                        &toks,
                        0,
                        format!(
                            "Syntax error: unknown keyword {} in DYNPRO {number:04}",
                            found(&toks, 0)
                        ),
                    )
                }
            }
        }
    }

    /// `TEXT zeile spalte 'text' { COLOR n | INTENSIFIED }`
    fn text_element(&self, line: usize, toks: &[Tok]) -> Result<Element, DynproError> {
        let row = self.expect_number(line, toks, 1, "row number")?;
        let col = self.expect_number(line, toks, 2, "column number")?;
        let text = self.expect_text(line, toks, 3, "text")?;
        let mut color = None;
        let mut intensified = false;
        let mut i = 4;
        while i < toks.len() {
            if toks[i].is_kw("COLOR") {
                color = Some(self.expect_color(line, toks, i + 1)?);
                i += 2;
            } else if toks[i].is_kw("INTENSIFIED") {
                intensified = true;
                i += 1;
            } else {
                return self.fail(
                    line,
                    toks,
                    i,
                    format!(
                        "Syntax error: expected COLOR or INTENSIFIED, found {}",
                        found(toks, i)
                    ),
                );
            }
        }
        Ok(Element {
            line,
            kw_col: toks[0].col,
            row,
            col,
            kind: ElementKind::Text {
                text,
                color,
                intensified,
            },
        })
    }

    /// `FIELD name zeile spalte { attribut }`. Ohne `INPUT` und `OUTPUT` ist das Feld `INPUT`.
    fn field_element(&self, line: usize, toks: &[Tok]) -> Result<Element, DynproError> {
        let name = self.expect_word(line, toks, 1, "field name")?;
        let row = self.expect_number(line, toks, 2, "row number")?;
        let col = self.expect_number(line, toks, 3, "column number")?;
        let mut attrs = FieldAttrs::default();
        let mut i = 4;
        while i < toks.len() {
            let up = if toks[i].quoted {
                String::new()
            } else {
                toks[i].text.to_uppercase()
            };
            match up.as_str() {
                "INPUT" => attrs.input = true,
                "OUTPUT" => attrs.output = true,
                "REQUIRED" => attrs.required = true,
                "INVISIBLE" => attrs.invisible = true,
                "INTENSIFIED" => attrs.intensified = true,
                "COLOR" => {
                    attrs.color = Some(self.expect_color(line, toks, i + 1)?);
                    i += 2;
                    continue;
                }
                "PASSWORD" => attrs.password = true,
                _ => {
                    return self.fail(
                        line,
                        toks,
                        i,
                        format!(
                            "Syntax error: expected field attribute, found {}",
                            found(toks, i)
                        ),
                    )
                }
            }
            i += 1;
        }
        if attrs.input && attrs.output {
            return self.fail(line, toks, 0, "INPUT and OUTPUT cannot be combined");
        }
        if !attrs.input && !attrs.output {
            attrs.input = true;
        }
        Ok(Element {
            line,
            kw_col: toks[0].col,
            row,
            col,
            kind: ElementKind::Field { name, attrs },
        })
    }

    /// `BUTTON zeile spalte 'text' CODE funktionscode`
    fn button_element(&self, line: usize, toks: &[Tok]) -> Result<Element, DynproError> {
        let row = self.expect_number(line, toks, 1, "row number")?;
        let col = self.expect_number(line, toks, 2, "column number")?;
        let text = self.expect_text(line, toks, 3, "button text")?;
        if !toks.get(4).is_some_and(|t| t.is_kw("CODE")) {
            return self.fail(
                line,
                toks,
                4,
                format!("Syntax error: expected CODE, found {}", found(toks, 4)),
            );
        }
        let code = self.expect_word(line, toks, 5, "function code")?;
        self.expect_end(line, toks, 6)?;
        Ok(Element {
            line,
            kw_col: toks[0].col,
            row,
            col,
            kind: ElementKind::Button { text, code },
        })
    }

    /// `FRAME zeile1 spalte1 zeile2 spalte2 [ 'titel' ]`
    fn frame_element(&self, line: usize, toks: &[Tok]) -> Result<Element, DynproError> {
        let row = self.expect_number(line, toks, 1, "row number")?;
        let col = self.expect_number(line, toks, 2, "column number")?;
        let row2 = self.expect_number(line, toks, 3, "row number")?;
        let col2 = self.expect_number(line, toks, 4, "column number")?;
        let title = if toks.len() > 5 {
            self.expect_end(line, toks, 6)?;
            self.expect_text(line, toks, 5, "frame title")?
        } else {
            String::new()
        };
        Ok(Element {
            line,
            kw_col: toks[0].col,
            row,
            col,
            kind: ElementKind::Frame { row2, col2, title },
        })
    }

    /// Ablauflogik bis `ENDFLOW`. Die Zeile `FLOW` ist bereits gelesen.
    /// Die Einträge bleiben in Quelltextfolge. Eine `CHAIN` ist nicht verschachtelbar und nur in der PAI-Folge erlaubt.
    /// Zeilen des Blocks `PROCESS ON VALUE-REQUEST` (Spec 12b) kommen in die zweite Liste.
    fn flow(&mut self) -> Result<(Vec<FlowItem>, Vec<ValueRequest>), DynproError> {
        let mut items = Vec::new();
        let mut value_requests: Vec<ValueRequest> = Vec::new();
        // Im Block `PROCESS ON VALUE-REQUEST` gilt nur `FIELD name MODULE m`
        let mut in_value_request = false;
        let mut phase: Option<Phase> = None;
        // Offene CHAIN, falls zwischen CHAIN und ENDCHAIN gelesen wird
        let mut open: Option<OpenChain> = None;
        // Anzahl der bisher begonnenen Ketten (Kettennummer ab 1)
        let mut chains = 0;
        loop {
            let Some((ln, toks)) = self.take() else {
                let expected = if open.is_some() {
                    "ENDCHAIN"
                } else {
                    "ENDFLOW"
                };
                return Err(self.eof(expected));
            };
            let kw = keyword(&toks);
            if let Some(chain) = open.as_mut() {
                // Innerhalb einer CHAIN sind nur FIELD, MODULE ... ON CHAIN-* und ENDCHAIN erlaubt
                match kw.as_str() {
                    "ENDCHAIN" => {
                        self.expect_end(ln, &toks, 1)?;
                        if chain.fields == 0 {
                            return self.fail(
                                ln,
                                &toks,
                                0,
                                "Syntax error: CHAIN without FIELD before ENDCHAIN",
                            );
                        }
                        open = None;
                    }
                    "FIELD" => {
                        let name = self.expect_word(ln, &toks, 1, "field name")?;
                        self.expect_end(ln, &toks, 2)?;
                        items.push(FlowItem::Field(FieldFlow {
                            name,
                            module: None,
                            cond: Cond::Plain,
                            chain: Some(chain.number),
                            line: ln,
                        }));
                        chain.fields += 1;
                    }
                    "MODULE" => {
                        if chain.module {
                            return self.fail(
                                ln,
                                &toks,
                                0,
                                "Syntax error: only one MODULE ON CHAIN-INPUT or ON CHAIN-REQUEST is allowed per CHAIN",
                            );
                        }
                        let module = self.expect_word(ln, &toks, 1, "module name")?;
                        let (cond, at_exit) = self.module_tail(ln, &toks, 2)?;
                        if at_exit || !matches!(cond, Cond::ChainInput | Cond::ChainRequest) {
                            return self.fail(
                                ln,
                                &toks,
                                2,
                                "Syntax error: MODULE in CHAIN needs ON CHAIN-INPUT or ON CHAIN-REQUEST",
                            );
                        }
                        items.push(FlowItem::ChainModule(ChainModule {
                            chain: chain.number,
                            module,
                            cond,
                            line: ln,
                        }));
                        chain.module = true;
                    }
                    "CHAIN" => {
                        return self.fail(ln, &toks, 0, "Syntax error: CHAIN cannot be nested");
                    }
                    _ => {
                        return self.fail(
                            ln,
                            &toks,
                            0,
                            format!(
                            "Syntax error: expected FIELD, MODULE or ENDCHAIN in CHAIN, found {}",
                            found(&toks, 0)
                        ),
                        )
                    }
                }
                continue;
            }
            if in_value_request && !matches!(kw.as_str(), "ENDFLOW" | "PROCESS") {
                if kw != "FIELD" {
                    return self.fail(
                        ln,
                        &toks,
                        0,
                        format!(
                            "Syntax error: expected FIELD in PROCESS ON VALUE-REQUEST, found {}",
                            found(&toks, 0)
                        ),
                    );
                }
                let name = self.expect_word(ln, &toks, 1, "field name")?;
                if !toks.get(2).is_some_and(|t| t.is_kw("MODULE")) {
                    return self.fail(
                        ln,
                        &toks,
                        2,
                        format!("Syntax error: expected MODULE, found {}", found(&toks, 2)),
                    );
                }
                let module = self.expect_word(ln, &toks, 3, "module name")?;
                self.expect_end(ln, &toks, 4)?;
                if value_requests
                    .iter()
                    .any(|v| v.name.eq_ignore_ascii_case(&name))
                {
                    return self.fail(
                        ln,
                        &toks,
                        1,
                        format!(
                            "Syntax error: field '{name}' is in more than one FIELD line of PROCESS ON VALUE-REQUEST"
                        ),
                    );
                }
                value_requests.push(ValueRequest {
                    name,
                    module,
                    line: ln,
                });
                continue;
            }
            match kw.as_str() {
                "ENDFLOW" => {
                    self.expect_end(ln, &toks, 1)?;
                    return Ok((items, value_requests));
                }
                "PROCESS" => {
                    if toks.get(1).is_some_and(|t| t.is_kw("BEFORE"))
                        && toks.get(2).is_some_and(|t| t.is_kw("OUTPUT"))
                    {
                        self.expect_end(ln, &toks, 3)?;
                        phase = Some(Phase::Pbo);
                        in_value_request = false;
                    } else if toks.get(1).is_some_and(|t| t.is_kw("AFTER"))
                        && toks.get(2).is_some_and(|t| t.is_kw("INPUT"))
                    {
                        self.expect_end(ln, &toks, 3)?;
                        phase = Some(Phase::Pai);
                        in_value_request = false;
                    } else if toks.get(1).is_some_and(|t| t.is_kw("ON"))
                        && toks.get(2).is_some_and(|t| t.is_kw("VALUE-REQUEST"))
                    {
                        self.expect_end(ln, &toks, 3)?;
                        phase = None;
                        in_value_request = true;
                    } else {
                        return self.fail(
                            ln,
                            &toks,
                            1,
                            format!(
                                "Syntax error: expected BEFORE OUTPUT, AFTER INPUT or ON VALUE-REQUEST, found {}",
                                found(&toks, 1)
                            ),
                        );
                    }
                }
                "MODULE" => {
                    let Some(ph) = phase else {
                        return self.fail(
                            ln,
                            &toks,
                            0,
                            "MODULE without PROCESS BEFORE OUTPUT or PROCESS AFTER INPUT",
                        );
                    };
                    let module = self.expect_word(ln, &toks, 1, "module name")?;
                    let (cond, at_exit) = self.module_tail(ln, &toks, 2)?;
                    if at_exit && ph == Phase::Pbo {
                        return self.fail(
                            ln,
                            &toks,
                            2,
                            "AT EXIT-COMMAND is only allowed after PROCESS AFTER INPUT",
                        );
                    }
                    items.push(FlowItem::Module(FlowEntry {
                        phase: ph,
                        module,
                        at_exit,
                        cond,
                        line: ln,
                    }));
                }
                "FIELD" => {
                    if phase != Some(Phase::Pai) {
                        return self.fail(
                            ln,
                            &toks,
                            0,
                            "Syntax error: FIELD is only allowed after PROCESS AFTER INPUT",
                        );
                    }
                    let name = self.expect_word(ln, &toks, 1, "field name")?;
                    let (module, cond) = if toks.len() == 2 {
                        (None, Cond::Plain)
                    } else {
                        if !toks.get(2).is_some_and(|t| t.is_kw("MODULE")) {
                            return self.fail(
                                ln,
                                &toks,
                                2,
                                format!("Syntax error: expected MODULE, found {}", found(&toks, 2)),
                            );
                        }
                        let module = self.expect_word(ln, &toks, 3, "module name")?;
                        let (cond, at_exit) = self.module_tail(ln, &toks, 4)?;
                        if at_exit {
                            return self.fail(
                                ln,
                                &toks,
                                4,
                                "Syntax error: AT EXIT-COMMAND is only allowed for MODULE, not for FIELD",
                            );
                        }
                        (Some(module), cond)
                    };
                    items.push(FlowItem::Field(FieldFlow {
                        name,
                        module,
                        cond,
                        chain: None,
                        line: ln,
                    }));
                }
                "CHAIN" => {
                    if phase != Some(Phase::Pai) {
                        return self.fail(
                            ln,
                            &toks,
                            0,
                            "Syntax error: CHAIN is only allowed after PROCESS AFTER INPUT",
                        );
                    }
                    self.expect_end(ln, &toks, 1)?;
                    chains += 1;
                    open = Some(OpenChain {
                        number: chains,
                        fields: 0,
                        module: false,
                    });
                }
                "ENDCHAIN" => {
                    return self.fail(ln, &toks, 0, "Syntax error: ENDCHAIN without CHAIN");
                }
                _ => {
                    return self.fail(
                        ln,
                        &toks,
                        0,
                        format!("Syntax error: unknown keyword {} in FLOW", found(&toks, 0)),
                    )
                }
            }
        }
    }

    /// Rest einer `MODULE`- oder `FIELD ... MODULE`-Zeile ab Index `from`: nichts, `AT EXIT-COMMAND` oder `ON <bedingung>`.
    /// Gibt die Bedingung und das Exit-Kennzeichen zurück.
    fn module_tail(
        &self,
        ln: usize,
        toks: &[Tok],
        from: usize,
    ) -> Result<(Cond, bool), DynproError> {
        match toks.get(from) {
            None => Ok((Cond::Plain, false)),
            Some(t) if t.is_kw("AT") => {
                if toks.len() == from + 2 && toks[from + 1].is_kw("EXIT-COMMAND") {
                    Ok((Cond::Plain, true))
                } else {
                    self.fail(
                        ln,
                        toks,
                        from,
                        format!(
                            "Syntax error: expected AT EXIT-COMMAND, found {}",
                            found(toks, from)
                        ),
                    )
                }
            }
            Some(t) if t.is_kw("ON") => {
                let cond = match toks.get(from + 1) {
                    Some(c) if c.is_kw("INPUT") => Cond::Input,
                    Some(c) if c.is_kw("REQUEST") => Cond::Request,
                    Some(c) if c.is_kw("CHAIN-INPUT") => Cond::ChainInput,
                    Some(c) if c.is_kw("CHAIN-REQUEST") => Cond::ChainRequest,
                    _ => {
                        return self.fail(
                            ln,
                            toks,
                            from + 1,
                            format!(
                                "Syntax error: expected INPUT, REQUEST, CHAIN-INPUT or CHAIN-REQUEST after ON, found {}",
                                found(toks, from + 1)
                            ),
                        )
                    }
                };
                self.expect_end(ln, toks, from + 2)?;
                Ok((cond, false))
            }
            Some(_) => self.fail(
                ln,
                toks,
                from,
                format!(
                    "Syntax error: expected AT EXIT-COMMAND or ON, found {}",
                    found(toks, from)
                ),
            ),
        }
    }

    /// `GUISTATUS name` ... `ENDGUISTATUS`
    fn status(&mut self, line: usize, head: &[Tok]) -> Result<GuiStatus, DynproError> {
        let name = self.expect_word(line, head, 1, "GUI status name")?;
        self.expect_end(line, head, 2)?;
        let mut keys = Vec::new();
        let mut excludes = Vec::new();
        loop {
            let Some((ln, toks)) = self.take() else {
                return Err(self.eof("ENDGUISTATUS"));
            };
            match keyword(&toks).as_str() {
                "ENDGUISTATUS" => {
                    self.expect_end(ln, &toks, 1)?;
                    return Ok(GuiStatus {
                        name,
                        line,
                        keys,
                        excludes,
                    });
                }
                "FKEY" => keys.push(self.fkey(ln, &toks)?),
                "EXCLUDE" => {
                    // EXCLUDE funktionscode: Prüfung der Länge und des Bezugs in check.rs
                    let code = self.expect_word(ln, &toks, 1, "function code")?;
                    self.expect_end(ln, &toks, 2)?;
                    excludes.push(Exclude { code, line: ln });
                }
                _ => {
                    return self.fail(
                        ln,
                        &toks,
                        0,
                        format!(
                            "Syntax error: unknown keyword {} in GUISTATUS",
                            found(&toks, 0)
                        ),
                    )
                }
            }
        }
    }

    /// `FKEY F3 'Text' CODE funktionscode [ TYPE B | C | E ]`
    fn fkey(&self, line: usize, toks: &[Tok]) -> Result<FKey, DynproError> {
        let name = self.expect_word(line, toks, 1, "function key F1 to F24")?;
        let digits = name
            .strip_prefix(['F', 'f'])
            .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
        // F1 bis F24 sind definierbar (ABI und Beispiel 4.3 nutzen F15). Ausgelöst werden F13 bis F24
        // erst mit Shift+F (Phase 2).
        let key = match digits.and_then(|d| d.parse::<u32>().ok()) {
            Some(k) if (1..=24).contains(&k) => k,
            _ => {
                return self.fail(
                    line,
                    toks,
                    1,
                    format!("Syntax error: expected function key F1 to F24, found '{name}'"),
                )
            }
        };
        let text = self.expect_text(line, toks, 2, "key text")?;
        if !toks.get(3).is_some_and(|t| t.is_kw("CODE")) {
            return self.fail(
                line,
                toks,
                3,
                format!("Syntax error: expected CODE, found {}", found(toks, 3)),
            );
        }
        let code = self.expect_word(line, toks, 4, "function code")?;
        let kind = match toks.get(5) {
            None => FKeyType::None,
            Some(t) if t.is_kw("TYPE") => {
                let kind = match toks.get(6) {
                    Some(k) if k.is_kw("B") => FKeyType::Back,
                    Some(k) if k.is_kw("C") => FKeyType::Cancel,
                    Some(k) if k.is_kw("E") => FKeyType::Exit,
                    _ => {
                        return self.fail(
                            line,
                            toks,
                            6,
                            format!("Syntax error: expected B, C or E, found {}", found(toks, 6)),
                        )
                    }
                };
                self.expect_end(line, toks, 7)?;
                kind
            }
            Some(_) => {
                return self.fail(
                    line,
                    toks,
                    5,
                    format!("Syntax error: expected TYPE, found {}", found(toks, 5)),
                )
            }
        };
        Ok(FKey {
            key,
            text,
            code,
            kind,
            line,
        })
    }
}

/// Parst den Inhalt einer `.dynp`-Datei. `file` steht in den Meldungen.
pub fn parse_dynp(file: &str, src: &str) -> Result<DynproFile, DynproError> {
    let mut lines = Vec::new();
    for (i, text) in src.lines().enumerate() {
        let toks = tokenize(file, i + 1, text)?;
        if !toks.is_empty() {
            lines.push((i + 1, toks));
        }
    }
    let mut l = Lines {
        file,
        items: lines,
        pos: 0,
    };
    let mut items = Vec::new();
    while let Some((ln, toks)) = l.take() {
        match keyword(&toks).as_str() {
            "DYNPRO" => items.push(DynproItem::Dynpro(l.dynpro(ln, &toks)?)),
            "GUISTATUS" => items.push(DynproItem::Status(l.status(ln, &toks)?)),
            _ => {
                return l.fail(
                    ln,
                    &toks,
                    0,
                    format!(
                        "Syntax error: expected DYNPRO or GUISTATUS, found {}",
                        found(&toks, 0)
                    ),
                )
            }
        }
    }
    Ok(DynproFile {
        file: file.to_string(),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Beispiel aus Spezifikation 4.3 (`zkunde_demo.dynp`).
    const SPEC_EXAMPLE: &str = "\
* Dynpro 0100: Kunde anlegen
DYNPRO 0100
  SIZE 20 80
  TITLE 'Kunde anlegen'

  TEXT 2 2 'Kundennummer'
  FIELD ZKUNNR 2 20 INPUT REQUIRED
  TEXT 4 2 'Name'
  FIELD ZNAME1 4 20 INPUT
  TEXT 6 2 'Ort'
  FIELD ZORT 6 20 INPUT
  BUTTON 9 2 'Sichern' CODE SAVE

  FLOW
    PROCESS BEFORE OUTPUT
      MODULE STATUS_0100
    PROCESS AFTER INPUT
      MODULE USER_COMMAND_0100
      MODULE EXIT_0100 AT EXIT-COMMAND
  ENDFLOW
ENDDYNPRO

GUISTATUS STATUS_0100
  FKEY F3  'Back'    CODE BACK   TYPE B
  FKEY F12 'Cancel'  CODE CANCEL TYPE C
  FKEY F15 'Exit'    CODE EXIT   TYPE E
ENDGUISTATUS
";

    fn parse(src: &str) -> Result<DynproFile, DynproError> {
        parse_dynp("test.dynp", src)
    }

    fn only_dynpro(f: &DynproFile) -> &Dynpro {
        match &f.items[0] {
            DynproItem::Dynpro(d) => d,
            other => panic!("expected DYNPRO, found {other:?}"),
        }
    }

    /// Eintrag ohne Feldbezug (`MODULE`), sonst Absturz des Tests.
    fn entry(item: &FlowItem) -> &FlowEntry {
        match item {
            FlowItem::Module(e) => e,
            other => panic!("expected MODULE entry, found {other:?}"),
        }
    }

    /// Ablauflogik des ersten Dynpros der Quelle `flow_src` in eine Liste.
    fn flow_of(flow_src: &str) -> Vec<FlowItem> {
        let src = format!("DYNPRO 0100\nFLOW\n{flow_src}ENDFLOW\nENDDYNPRO\n");
        only_dynpro(&parse(&src).unwrap()).flow.clone()
    }

    /// Fehler der Ablauflogik für die Quelle `flow_src`.
    fn flow_error(flow_src: &str) -> DynproError {
        let src = format!("DYNPRO 0100\nFLOW\n{flow_src}ENDFLOW\nENDDYNPRO\n");
        parse(&src).unwrap_err()
    }

    #[test]
    fn dynpro_without_size_uses_default_20_by_80() {
        let f = parse("DYNPRO 100\nENDDYNPRO\n").unwrap();
        let d = only_dynpro(&f);
        assert_eq!((d.number, d.rows, d.cols), (100, 20, 80));
        assert!(!d.modal);
    }

    #[test]
    fn dynpro_kind_modal_is_read() {
        let f = parse("DYNPRO 0200 KIND MODAL\nENDDYNPRO\n").unwrap();
        assert!(only_dynpro(&f).modal);
    }

    #[test]
    fn spec_example_parses_elements_flow_and_status() {
        let f = parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(f.items.len(), 2);
        let d = only_dynpro(&f);
        assert_eq!(d.elements.len(), 7);
        assert_eq!(d.flow.len(), 3);
        assert_eq!(entry(&d.flow[0]).phase, Phase::Pbo);
        assert_eq!(entry(&d.flow[2]).phase, Phase::Pai);
        assert!(entry(&d.flow[2]).at_exit);
        assert!(!entry(&d.flow[1]).at_exit);
        let DynproItem::Status(s) = &f.items[1] else {
            panic!("expected GUISTATUS");
        };
        assert_eq!(s.keys.len(), 3);
        assert_eq!(s.keys[0].key, 3);
        assert_eq!(s.keys[0].kind, FKeyType::Back);
        assert_eq!(s.keys[1].kind, FKeyType::Cancel);
        assert_eq!(s.keys[2].kind, FKeyType::Exit);
    }

    #[test]
    fn field_without_input_or_output_defaults_to_input() {
        let f = parse("DYNPRO 0100\nFIELD ZA 1 1\nENDDYNPRO\n").unwrap();
        let ElementKind::Field { attrs, .. } = &only_dynpro(&f).elements[0].kind else {
            panic!("expected FIELD");
        };
        assert!(attrs.input && !attrs.output);
    }

    #[test]
    fn field_output_is_not_input() {
        let f = parse("DYNPRO 0100\nFIELD ZA 1 1 OUTPUT COLOR 4\nENDDYNPRO\n").unwrap();
        let ElementKind::Field { attrs, .. } = &only_dynpro(&f).elements[0].kind else {
            panic!("expected FIELD");
        };
        assert!(!attrs.input && attrs.output);
        assert_eq!(attrs.color, Some(4));
        assert_eq!(attrs.bits(), 2 | (4 << 8));
    }

    #[test]
    fn input_and_output_together_are_rejected() {
        let e = parse("DYNPRO 0100\nFIELD ZA 1 1 INPUT OUTPUT\nENDDYNPRO\n").unwrap_err();
        assert_eq!((e.line, e.warning), (2, false));
        assert!(e.message.contains("INPUT and OUTPUT cannot be combined"));
    }

    #[test]
    fn comment_lines_and_inline_comments_are_ignored() {
        let src = "* ganze Zeile\nDYNPRO 0100 \" Kommentar\n  TEXT 1 1 'a \" b'\nENDDYNPRO\n";
        let f = parse(src).unwrap();
        let ElementKind::Text { text, .. } = &only_dynpro(&f).elements[0].kind else {
            panic!("expected TEXT");
        };
        assert_eq!(text, "a \" b");
    }

    #[test]
    fn doubled_quote_in_text_is_unescaped() {
        let f = parse("DYNPRO 0100\nTEXT 1 1 'It''s'\nENDDYNPRO\n").unwrap();
        let ElementKind::Text { text, .. } = &only_dynpro(&f).elements[0].kind else {
            panic!("expected TEXT");
        };
        assert_eq!(text, "It's");
    }

    #[test]
    fn missing_column_is_reported_with_line_and_column() {
        let e = parse("DYNPRO 0100\n  FIELD ZKUNNR 2\nENDDYNPRO\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert_eq!(e.col, "  FIELD ZKUNNR 2".len() + 1);
        assert_eq!(
            e.message,
            "Syntax error: expected column number, found end of line"
        );
    }

    #[test]
    fn unterminated_string_is_reported() {
        let e = parse("DYNPRO 0100\nTEXT 1 1 'offen\nENDDYNPRO\n").unwrap_err();
        assert_eq!((e.line, e.col), (2, 10));
        assert_eq!(e.message, "Unterminated string literal");
    }

    #[test]
    fn password_attribute_is_accepted_and_sets_its_bit() {
        let f = parse("DYNPRO 0100\nFIELD ZA 1 1 PASSWORD\nENDDYNPRO\n").unwrap();
        let d = only_dynpro(&f);
        let ElementKind::Field { attrs, .. } = &d.elements[0].kind else {
            panic!("expected FIELD");
        };
        assert!(attrs.password);
        assert!(attrs.input, "without INPUT or OUTPUT the field is INPUT");
        assert_eq!(attrs.bits() & ATTR_PASSWORD, ATTR_PASSWORD);
        assert_eq!(ATTR_PASSWORD, 0x1_0000);
    }

    #[test]
    fn password_bit_is_independent_of_other_attributes() {
        let f = parse("DYNPRO 0100\nFIELD ZA 1 1 OUTPUT PASSWORD REQUIRED\nENDDYNPRO\n").unwrap();
        let ElementKind::Field { attrs, .. } = &only_dynpro(&f).elements[0].kind else {
            panic!("expected FIELD");
        };
        assert_eq!(attrs.bits(), ATTR_PASSWORD | 2 | 4);
    }

    #[test]
    fn exclude_in_guistatus_is_read_with_line() {
        let f = parse("GUISTATUS S\nFKEY F3 'Zurueck' CODE BACK\nEXCLUDE PRINT\nENDGUISTATUS\n")
            .unwrap();
        let DynproItem::Status(s) = &f.items[0] else {
            panic!("expected GUISTATUS");
        };
        assert_eq!(
            s.excludes,
            vec![Exclude {
                code: "PRINT".into(),
                line: 3
            }]
        );
    }

    #[test]
    fn exclude_without_code_is_rejected() {
        let e = parse("GUISTATUS S\nEXCLUDE\nENDGUISTATUS\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("expected function code"));
    }

    #[test]
    fn exclude_with_extra_token_is_rejected() {
        let e = parse("GUISTATUS S\nEXCLUDE PRINT X\nENDGUISTATUS\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("unexpected"));
    }

    #[test]
    fn exit_command_after_before_output_is_rejected() {
        let src = "DYNPRO 0100\nFLOW\nPROCESS BEFORE OUTPUT\nMODULE M AT EXIT-COMMAND\nENDFLOW\nENDDYNPRO\n";
        let e = parse(src).unwrap_err();
        assert_eq!(e.line, 4);
        assert!(e.message.contains("only allowed after PROCESS AFTER INPUT"));
    }

    #[test]
    fn function_keys_f1_to_f24_are_definable() {
        let f = parse("GUISTATUS S\nFKEY F15 'x' CODE X\nENDGUISTATUS\n").unwrap();
        let DynproItem::Status(s) = &f.items[0] else {
            panic!("expected GUISTATUS");
        };
        assert_eq!(s.keys[0].key, 15);
    }

    #[test]
    fn function_key_above_24_is_rejected() {
        let e = parse("GUISTATUS S\nFKEY F25 'x' CODE X\nENDGUISTATUS\n").unwrap_err();
        assert!(e.message.contains("expected function key F1 to F24"));
    }

    #[test]
    fn unknown_keyword_in_dynpro_names_the_dynpro() {
        let e = parse("DYNPRO 0100\nBOGUS 1 2\nENDDYNPRO\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert_eq!(
            e.message,
            "Syntax error: unknown keyword 'BOGUS' in DYNPRO 0100"
        );
    }

    #[test]
    fn dynpro_number_zero_is_rejected() {
        let e = parse("DYNPRO 0\nENDDYNPRO\n").unwrap_err();
        assert!(e.message.contains("Dynpro number must be 1 to 9999"));
    }

    #[test]
    fn missing_enddynpro_is_reported_at_end_of_file() {
        let e = parse("DYNPRO 0100\nTEXT 1 1 'x'\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("expected ENDDYNPRO"));
    }

    #[test]
    fn color_outside_1_to_7_is_rejected() {
        let e = parse("DYNPRO 0100\nTEXT 1 1 'x' COLOR 8\nENDDYNPRO\n").unwrap_err();
        assert!(e.message.contains("Color must be 1 to 7"));
    }

    #[test]
    fn button_and_frame_are_parsed_with_positions() {
        let f = parse(
            "DYNPRO 0100\nBUTTON 9 2 'Sichern' CODE SAVE\nFRAME 1 1 5 30 'Kopf'\nENDDYNPRO\n",
        )
        .unwrap();
        let d = only_dynpro(&f);
        assert_eq!((d.elements[0].row, d.elements[0].col), (9, 2));
        let ElementKind::Frame { row2, col2, title } = &d.elements[1].kind else {
            panic!("expected FRAME");
        };
        assert_eq!((*row2, *col2, title.as_str()), (5, 30, "Kopf"));
    }

    #[test]
    fn field_without_module_is_a_flow_item_without_module() {
        let items = flow_of("PROCESS AFTER INPUT\n  FIELD ZKUNNR\n");
        let [FlowItem::Field(f)] = items.as_slice() else {
            panic!("expected one FIELD item, found {items:?}");
        };
        assert_eq!(f.name, "ZKUNNR");
        assert_eq!(
            (f.module.as_deref(), f.cond, f.chain),
            (None, Cond::Plain, None)
        );
        assert_eq!(f.line, 4);
    }

    #[test]
    fn field_with_module_and_on_input_or_request_is_read() {
        let items = flow_of(
            "PROCESS AFTER INPUT\n  FIELD A MODULE CHK_A ON INPUT\n  FIELD B MODULE chk_b on request\n",
        );
        let [FlowItem::Field(a), FlowItem::Field(b)] = items.as_slice() else {
            panic!("expected two FIELD items, found {items:?}");
        };
        assert_eq!((a.module.as_deref(), a.cond), (Some("CHK_A"), Cond::Input));
        assert_eq!(
            (b.module.as_deref(), b.cond),
            (Some("chk_b"), Cond::Request)
        );
        assert_eq!(a.chain, None);
    }

    #[test]
    fn field_with_module_without_condition_is_plain() {
        let items = flow_of("PROCESS AFTER INPUT\n  FIELD A MODULE CHK_A\n");
        let FlowItem::Field(a) = &items[0] else {
            panic!("expected FIELD item");
        };
        assert_eq!((a.module.as_deref(), a.cond), (Some("CHK_A"), Cond::Plain));
    }

    #[test]
    fn chain_items_keep_source_order_and_chain_number() {
        let items = flow_of(
            "PROCESS AFTER INPUT\n  FIELD ZKUNNR MODULE CHECK_KUNNR ON REQUEST\n  CHAIN\n    FIELD ZWERK\n    FIELD ZLGORT\n    MODULE CHECK_LAGER ON CHAIN-REQUEST\n  ENDCHAIN\n  MODULE USER_COMMAND_0100\n",
        );
        assert_eq!(items.len(), 5);
        let FlowItem::Field(kunnr) = &items[0] else {
            panic!("expected FIELD first, found {items:?}");
        };
        assert_eq!(
            (kunnr.name.as_str(), kunnr.chain, kunnr.cond),
            ("ZKUNNR", None, Cond::Request)
        );
        let FlowItem::Field(werk) = &items[1] else {
            panic!("expected chain FIELD second");
        };
        assert_eq!((werk.name.as_str(), werk.chain), ("ZWERK", Some(1)));
        let FlowItem::Field(lgort) = &items[2] else {
            panic!("expected chain FIELD third");
        };
        assert_eq!((lgort.name.as_str(), lgort.chain), ("ZLGORT", Some(1)));
        let FlowItem::ChainModule(cm) = &items[3] else {
            panic!("expected chain MODULE last");
        };
        assert_eq!(
            (cm.chain, cm.module.as_str(), cm.cond, cm.line),
            (1, "CHECK_LAGER", Cond::ChainRequest, 8)
        );
        assert_eq!(entry(&items[4]).module, "USER_COMMAND_0100");
    }

    #[test]
    fn chain_with_input_module_and_numbers_restart_per_dynpro() {
        let items = flow_of(
            "PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    MODULE M1 ON CHAIN-INPUT\n  ENDCHAIN\n  CHAIN\n    FIELD B\n  ENDCHAIN\n",
        );
        let [FlowItem::Field(a), FlowItem::ChainModule(m1), FlowItem::Field(b)] = items.as_slice()
        else {
            panic!("unexpected items {items:?}");
        };
        assert_eq!((a.chain, m1.chain, m1.cond), (Some(1), 1, Cond::ChainInput));
        assert_eq!(b.chain, Some(2));
    }

    #[test]
    fn chain_without_module_has_no_chain_module_item() {
        let items = flow_of("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n  ENDCHAIN\n");
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], FlowItem::Field(f) if f.chain == Some(1)));
    }

    #[test]
    fn module_on_chain_outside_chain_is_read_for_the_checker() {
        // Die Prüfung (check.rs) weist das ab; der Parser nimmt die Zeile an.
        let items = flow_of("PROCESS AFTER INPUT\n  MODULE M ON CHAIN-INPUT\n");
        assert!(matches!(&items[0], FlowItem::Module(e) if e.cond == Cond::ChainInput));
    }

    #[test]
    fn module_without_on_or_exit_keeps_old_behaviour() {
        let items = flow_of("PROCESS AFTER INPUT\n  MODULE M\n  MODULE X AT EXIT-COMMAND\n");
        assert_eq!(entry(&items[0]).module, "M");
        assert!(!entry(&items[0]).at_exit);
        assert_eq!(entry(&items[0]).cond, Cond::Plain);
        assert!(entry(&items[1]).at_exit);
    }

    #[test]
    fn nested_chain_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    CHAIN\n");
        assert_eq!(e.line, 6);
        assert_eq!(e.message, "Syntax error: CHAIN cannot be nested");
    }

    #[test]
    fn line_other_than_field_or_module_inside_chain_is_a_syntax_error() {
        let e =
            flow_error("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    PROCESS BEFORE OUTPUT\n");
        assert_eq!(e.line, 6);
        assert!(e
            .message
            .contains("expected FIELD, MODULE or ENDCHAIN in CHAIN"));
        assert!(e.message.contains("'PROCESS'"));
    }

    #[test]
    fn field_with_module_inside_chain_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A MODULE M ON INPUT\n");
        assert_eq!(e.line, 5);
        assert!(e.message.contains("unexpected 'MODULE'"));
    }

    #[test]
    fn plain_module_inside_chain_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    MODULE M\n");
        assert_eq!(e.line, 6);
        assert!(e
            .message
            .contains("MODULE in CHAIN needs ON CHAIN-INPUT or ON CHAIN-REQUEST"));
    }

    #[test]
    fn module_on_input_inside_chain_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    MODULE M ON INPUT\n");
        assert!(e.message.contains("MODULE in CHAIN needs ON CHAIN-INPUT"));
    }

    #[test]
    fn two_chain_modules_in_one_chain_are_a_syntax_error() {
        let e = flow_error(
            "PROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n    MODULE M1 ON CHAIN-INPUT\n    MODULE M2 ON CHAIN-REQUEST\n",
        );
        assert_eq!(e.line, 7);
        assert!(e.message.contains("only one MODULE"));
    }

    #[test]
    fn chain_without_field_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  CHAIN\n  ENDCHAIN\n");
        assert_eq!(e.line, 5);
        assert!(e.message.contains("CHAIN without FIELD"));
    }

    #[test]
    fn endflow_inside_open_chain_is_a_syntax_error() {
        let src =
            "DYNPRO 0100\nFLOW\nPROCESS AFTER INPUT\n  CHAIN\n    FIELD A\nENDFLOW\nENDDYNPRO\n";
        let e = parse(src).unwrap_err();
        assert_eq!(e.line, 6);
        assert!(e
            .message
            .contains("expected FIELD, MODULE or ENDCHAIN in CHAIN"));
    }

    #[test]
    fn endchain_without_chain_is_a_syntax_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  ENDCHAIN\n");
        assert_eq!(e.line, 4);
        assert_eq!(e.message, "Syntax error: ENDCHAIN without CHAIN");
    }

    #[test]
    fn chain_before_process_after_input_is_an_error() {
        let e = flow_error("PROCESS BEFORE OUTPUT\n  CHAIN\n");
        assert!(e
            .message
            .contains("CHAIN is only allowed after PROCESS AFTER INPUT"));
    }

    #[test]
    fn field_in_flow_before_output_is_an_error() {
        let e = flow_error("PROCESS BEFORE OUTPUT\n  FIELD A\n");
        assert_eq!(e.line, 4);
        assert!(e
            .message
            .contains("FIELD is only allowed after PROCESS AFTER INPUT"));
    }

    #[test]
    fn field_in_flow_without_process_is_an_error() {
        let e = flow_error("  FIELD A\n");
        assert!(e
            .message
            .contains("FIELD is only allowed after PROCESS AFTER INPUT"));
    }

    #[test]
    fn on_without_module_after_field_is_an_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  FIELD A ON INPUT\n");
        assert_eq!(e.line, 4);
        assert!(e.message.contains("expected MODULE, found 'ON'"));
    }

    #[test]
    fn unknown_on_condition_is_an_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  FIELD A MODULE M ON CHANGE\n");
        assert!(e
            .message
            .contains("expected INPUT, REQUEST, CHAIN-INPUT or CHAIN-REQUEST after ON"));
    }

    #[test]
    fn exit_command_on_field_is_an_error() {
        let e = flow_error("PROCESS AFTER INPUT\n  FIELD A MODULE M AT EXIT-COMMAND\n");
        assert!(e
            .message
            .contains("AT EXIT-COMMAND is only allowed for MODULE"));
    }

    #[test]
    fn unterminated_chain_is_reported_at_end_of_file() {
        let e =
            parse("DYNPRO 0100\nFLOW\nPROCESS AFTER INPUT\n  CHAIN\n    FIELD A\n").unwrap_err();
        assert!(e.message.contains("expected ENDCHAIN"));
    }

    /// Wertehilfe-Zeilen des ersten Dynpros der Quelle `flow_src`.
    fn value_requests_of(flow_src: &str) -> Vec<ValueRequest> {
        let src = format!("DYNPRO 0100\nFLOW\n{flow_src}ENDFLOW\nENDDYNPRO\n");
        only_dynpro(&parse(&src).unwrap()).value_requests.clone()
    }

    #[test]
    fn value_request_block_is_read_apart_from_the_flow() {
        let src = "PROCESS AFTER INPUT\n  MODULE M\nPROCESS ON VALUE-REQUEST\n  FIELD ZLGORT MODULE F4_LGORT\n";
        assert_eq!(flow_of(src).len(), 1);
        assert_eq!(
            value_requests_of(src),
            vec![ValueRequest {
                name: "ZLGORT".into(),
                module: "F4_LGORT".into(),
                line: 6,
            }]
        );
    }

    #[test]
    fn value_request_block_ends_at_the_next_process_line() {
        let src = "PROCESS ON VALUE-REQUEST\n  FIELD A MODULE M\nPROCESS AFTER INPUT\n  FIELD B\n";
        let items = flow_of(src);
        assert!(matches!(&items[0], FlowItem::Field(f) if f.name == "B"));
        assert_eq!(value_requests_of(src).len(), 1);
    }

    #[test]
    fn value_request_line_other_than_field_is_a_syntax_error() {
        let e = flow_error("PROCESS ON VALUE-REQUEST\n  MODULE M\n");
        assert_eq!(e.line, 4);
        assert!(e
            .message
            .contains("expected FIELD in PROCESS ON VALUE-REQUEST, found 'MODULE'"));
    }

    #[test]
    fn chain_inside_value_request_block_is_a_syntax_error() {
        let e = flow_error("PROCESS ON VALUE-REQUEST\n  CHAIN\n");
        assert!(e
            .message
            .contains("expected FIELD in PROCESS ON VALUE-REQUEST, found 'CHAIN'"));
    }

    #[test]
    fn value_request_with_on_condition_is_a_syntax_error() {
        let e = flow_error("PROCESS ON VALUE-REQUEST\n  FIELD A MODULE M ON INPUT\n");
        assert_eq!(e.line, 4);
        assert!(e.message.contains("unexpected 'ON'"));
    }

    #[test]
    fn value_request_without_module_is_a_syntax_error() {
        let e = flow_error("PROCESS ON VALUE-REQUEST\n  FIELD A\n");
        assert!(e.message.contains("expected MODULE, found end of line"));
    }

    #[test]
    fn field_twice_in_value_request_block_is_a_syntax_error() {
        let e = flow_error("PROCESS ON VALUE-REQUEST\n  FIELD A MODULE M1\n  FIELD a MODULE M2\n");
        assert_eq!(e.line, 5);
        assert!(e.message.contains("more than one FIELD line"));
    }
}
