//! Typen, Strukturen, Klassen und Formalparameter.

use super::*;

impl Lowerer {
    /// CLASS ... DEFINITION: Attribute werden zu einer Struktur, Methodensignaturen gemerkt.
    pub(super) fn define_class(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let line = c.head.line;
        let name = c.name.to_lowercase();
        if c.head.has("INHERITING") {
            return err(line, "Vererbung wird noch nicht unterstützt");
        }
        if self.names.contains_key(&name) {
            return err(line, format!("Klasse '{name}' ist bereits definiert"));
        }
        // Name vorab registrieren, damit Attribute und Methoden die eigene Klasse referenzieren können
        let idx = self.classes.len();
        self.names.insert(name.clone(), Ty::Ref(idx));
        let mut fields = Vec::new();
        let mut sigs: HashMap<String, MethodSig> = HashMap::new();
        for m in &c.members {
            let st = &m.statement;
            match &m.kind {
                MemberKind::Data if st.keyword() == "DATA" => fields.push(self.field(st)?),
                MemberKind::Method(sig) if !sig.is_class => {
                    let (params, ret) = self.parse_method_params(&st.tokens[2.min(st.tokens.len())..], st.line)?;
                    if sigs.insert(sig.name.to_lowercase(), MethodSig { params, ret }).is_some() {
                        return err(st.line, format!("Methode '{}' doppelt", sig.name));
                    }
                }
                MemberKind::Data | MemberKind::Method(_) => {
                    return err(st.line, "CLASS-DATA und CLASS-METHODS werden noch nicht unterstützt")
                }
                _ => return err(st.line, format!("Klassenbestandteil wird noch nicht unterstützt: {}", st.keyword())),
            }
        }
        let struct_idx = self.define_struct(&format!("class {name}"), fields, line)?;
        self.classes.push(ClassDef { name: name.clone(), struct_idx, methods: Vec::new() });
        self.sigs.push(sigs);
        Ok(())
    }

    /// CLASS ... IMPLEMENTATION: Methodenrümpfe zur Deklaration zuordnen.
    pub(super) fn implement_class(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let name = c.name.to_lowercase();
        let idx = match self.names.get(&name) {
            Some(Ty::Ref(i)) => *i,
            _ => return err(c.head.line, format!("Klasse '{name}' nicht definiert")),
        };
        for m in &c.methods {
            let mname = m.name.to_lowercase();
            let Some(sig) = self.sigs[idx].get(&mname).cloned() else {
                return err(m.head.line, format!("Methode '{}' ist nicht in der Klassendefinition deklariert", m.name));
            };
            if self.classes[idx].methods.iter().any(|x| x.name == mname) {
                return err(m.head.line, format!("Methode '{}' doppelt implementiert", m.name));
            }
            let mut body = Vec::new();
            self.lower_nodes(&nodes(&m.body), &mut body)?;
            self.classes[idx].methods.push(ClassMethod {
                name: mname,
                params: sig.params,
                ret: sig.ret,
                body,
                line: m.head.line,
            });
        }
        Ok(())
    }

    /// Jede deklarierte Methode braucht eine Implementierung.
    pub(super) fn check_all_implemented(&self) -> Result<(), CompileError> {
        for (idx, class) in self.classes.iter().enumerate() {
            for m in self.sigs[idx].keys() {
                if !class.methods.iter().any(|x| &x.name == m) {
                    return err(0, format!("Methode '{}' der Klasse '{}' ohne Implementierung", m, class.name));
                }
            }
        }
        Ok(())
    }

    /// Parameter einer Methode: `IMPORTING p TYPE t CHANGING q TYPE t` (IMPORTING = Übergabe wie USING).
    pub(super) fn parse_method_params(&self, toks: &[Token], line: usize) -> Result<(Vec<Param>, Option<RetParam>), CompileError> {
        // RETURNING abtrennen; davor IMPORTING/CHANGING wie FORM-Parameter, danach RETURNING.
        let ret_pos = toks.iter().position(|t| t.is_word("RETURNING"));
        let (before, after) = match ret_pos {
            Some(p) => (&toks[..p], Some(&toks[p + 1..])),
            None => (toks, None),
        };
        let mapped: Vec<Token> = before
            .iter()
            .map(|t| {
                let mut t = t.clone();
                if t.is_word("IMPORTING") {
                    t.text = "USING".into();
                }
                t
            })
            .collect();
        let params = self.parse_params(&mapped, line)?;
        let ret = match after {
            None => None,
            Some(rest) => {
                // RAISING/EXCEPTIONS-Teil wird ignoriert
                let end = rest
                    .iter()
                    .position(|t| t.is_word("RAISING") || t.is_word("EXCEPTIONS"))
                    .unwrap_or(rest.len());
                let r = &rest[..end];
                // Form: VALUE(name) TYPE typ  oder  name TYPE typ
                let (name, tail) = match r {
                    [v, p, n, c, tail @ ..] if v.is_word("VALUE") && p.is_punct("(") && c.is_punct(")") => {
                        (word(n), tail)
                    }
                    [n, tail @ ..] if n.kind == Kind::Word => (word(n), tail),
                    _ => return err(line, "RETURNING erwartet 'VALUE(name) TYPE typ'"),
                };
                let Some(t) = tail.first().filter(|t| t.is_word("TYPE")) else {
                    return err(line, "RETURNING ohne TYPE wird nicht unterstützt");
                };
                let _ = t;
                let ty = self.ty_of(&tail[1..], line)?;
                Some(RetParam { name, ty })
            }
        };
        Ok((params, ret))
    }

    /// Komponente `name TYPE typ` innerhalb einer Struktur.
    pub(super) fn field(&self, f: &Statement) -> Result<(String, Ty), CompileError> {
        let line = f.line;
        let toks = &f.tokens;
        if toks.len() < 4 || toks[1].kind != Kind::Word || !toks[2].is_word("TYPE") {
            return err(line, "Komponente erwartet 'name TYPE typ'");
        }
        let ty = self.ty_of(&toks[3..], line)?;
        if matches!(ty, Ty::Table(_) | Ty::Struct(_)) {
            return err(line, "Tabellen und Strukturen als Komponente werden nicht unterstützt");
        }
        Ok((word(&toks[1]), ty))
    }

    pub(super) fn define_struct(&mut self, name: &str, fields: Vec<(String, Ty)>, line: usize) -> Result<usize, CompileError> {
        let mut seen = std::collections::HashSet::new();
        for (f, _) in &fields {
            if !seen.insert(f.clone()) {
                return err(line, format!("Komponente '{f}' doppelt in Struktur '{name}'"));
            }
        }
        self.structs.push(StructDef { name: name.to_lowercase(), fields });
        Ok(self.structs.len() - 1)
    }

    /// Typ aus Tokens: Tabelle, Elementtyp oder Typname.
    pub(super) fn ty_of(&self, toks: &[Token], line: usize) -> Result<Ty, CompileError> {
        if toks.len() == 3 && toks[0].is_word("REF") && toks[1].is_word("TO") {
            return match self.names.get(&word(&toks[2])) {
                Some(Ty::Ref(i)) => Ok(Ty::Ref(*i)),
                _ => err(line, format!("Klasse '{}' nicht definiert", toks[2].text)),
            };
        }
        if let Some(elem) = table_elem_tokens(toks) {
            return Ok(Ty::Table(self.elem_of(elem, line)?));
        }
        Ok(self.elem_of(toks, line)?.ty())
    }

    /// Elementtyp: `i`, `string`, `c`, `c LENGTH n` oder ein bekannter Typname.
    pub(super) fn elem_of(&self, toks: &[Token], line: usize) -> Result<Elem, CompileError> {
        match toks {
            [t] if t.is_word("i") => Ok(Elem::Int),
            [t] if t.is_word("string") => Ok(Elem::Str),
            [t] if t.is_word("c") => Ok(Elem::Char(1)),
            [t, l, n] if t.is_word("c") && l.is_word("LENGTH") => {
                let len: usize = n.text.parse().or_else(|_| err(line, format!("ungültige Länge: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Länge muss größer 0 sein");
                }
                Ok(Elem::Char(len))
            }
            [t] => match self.names.get(&word(t)) {
                Some(Ty::Int) => Ok(Elem::Int),
                Some(Ty::Str) => Ok(Elem::Str),
                Some(Ty::Char(n)) => Ok(Elem::Char(*n)),
                Some(Ty::Struct(i)) => Ok(Elem::Struct(*i)),
                Some(Ty::Table(_)) => err(line, format!("Tabelle '{}' kann nicht Element einer Tabelle sein", t.text)),
                Some(Ty::Ref(_)) => err(line, "Objektreferenzen als Tabellenelement werden noch nicht unterstützt"),
                None => err(line, format!("Typ '{}' wird noch nicht unterstützt", t.text)),
            },
            [t, ..] => err(line, format!("Typ '{}' wird noch nicht unterstützt", t.text)),
            [] => err(line, "Typangabe fehlt"),
        }
    }

    /// Formalparameter aus dem Kopf einer FORM: `USING p TYPE t ... CHANGING q TYPE t ...`.
    pub(super) fn parse_params(&self, toks: &[Token], line: usize) -> Result<Vec<Param>, CompileError> {
        let mut out: Vec<Param> = Vec::new();
        let mut changing = false;
        let mut i = 0;
        while i < toks.len() {
            let t = &toks[i];
            if t.is_word("USING") {
                changing = false;
                i += 1;
                continue;
            }
            if t.is_word("CHANGING") {
                changing = true;
                i += 1;
                continue;
            }
            if t.kind != Kind::Word {
                return err(line, format!("unerwartetes Token im FORM-Kopf: '{}'", t.text));
            }
            let name = word(t);
            if !toks.get(i + 1).is_some_and(|x| x.is_word("TYPE")) {
                return err(line, format!("Parameter '{name}' ohne TYPE wird nicht unterstützt"));
            }
            // Typ reicht bis zum nächsten Parameter (name TYPE) oder USING/CHANGING
            let start = i + 2;
            let mut end = start;
            while end < toks.len() {
                let x = &toks[end];
                let is_switch = x.is_word("USING") || x.is_word("CHANGING");
                let is_next_param = x.kind == Kind::Word && toks.get(end + 1).is_some_and(|n| n.is_word("TYPE"));
                if is_switch || is_next_param {
                    break;
                }
                end += 1;
            }
            if start >= end {
                return err(line, format!("Typ für Parameter '{name}' fehlt"));
            }
            let ty = self.ty_of(&toks[start..end], line)?;
            if out.iter().any(|p| p.name == name) {
                return err(line, format!("Parameter '{name}' doppelt"));
            }
            out.push(Param { name, ty, changing });
            i = end;
        }
        Ok(out)
    }
}
