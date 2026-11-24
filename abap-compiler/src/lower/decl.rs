//! Typen, Strukturen, Klassen und Formalparameter.

use super::*;

impl Lowerer {
    /// CLASS ... DEFINITION: Attribute werden zu einer Struktur, Methodensignaturen gemerkt.
    pub(super) fn define_class(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let line = c.head.line;
        let name = c.name.to_lowercase();
        if self.names.contains_key(&name) {
            return err(line, format!("Klasse '{name}' ist bereits definiert"));
        }

        // Name vorab registrieren, damit Attribute und Methoden die eigene Klasse referenzieren können
        let idx = self.classes.len();
        self.names.insert(name.clone(), Ty::Ref(idx));
        let parent_name = c
            .head
            .tokens
            .iter()
            .position(|token| token.is_word("INHERITING"))
            .and_then(|index| c.head.tokens.get(index + 2))
            .filter(|token| token.kind == Kind::Word)
            .map(|token| token.text.to_lowercase());
        let parent = parent_name.as_ref().map(|parent_name| {
            self.names
                .get(parent_name)
                .and_then(|ty| match ty {
                    Ty::Ref(index) => Some(ClassParent::Internal(*index)),
                    _ => None,
                })
                .unwrap_or_else(|| ClassParent::External(parent_name.clone()))
        });
        let mut fields = match parent {
            Some(ClassParent::Internal(parent_idx)) => {
                let parent_struct = self.classes[parent_idx].struct_idx;
                self.structs[parent_struct].fields.clone()
            }
            _ => Vec::new(),
        };
        let mut static_fields = Vec::new();
        let mut sigs: HashMap<String, MethodSig> = HashMap::new();
        let mut interfaces = Vec::new();
        let mut aliases = Vec::new();
        self.scope = Some(name.clone());
        self.define_local_type_aliases(&c.members)?;
        self.define_local_data_types(&c.members)?;
        self.define_local_structures(&c.members)?;
        self.define_local_constants(&c.members)?;
        let mut in_data_structure = false;
        for m in &c.members {
            let st = &m.statement;
            if matches!(m.kind, MemberKind::Data)
                && st.tokens.get(1).is_some_and(|token| token.is_word("BEGIN"))
            {
                let name = word(&st.tokens[3]);
                let ty = self
                    .names
                    .get(&self.qualified_type(&name))
                    .copied()
                    .ok_or_else(|| CompileError {
                        line: st.line,
                        message: format!("Inline-Struktur '{name}' nicht definiert"),
                    })?;
                fields.push((name, ty));
                in_data_structure = true;
                continue;
            }
            if in_data_structure {
                if st.tokens.get(1).is_some_and(|token| token.is_word("END")) {
                    in_data_structure = false;
                }
                continue;
            }
            match &m.kind {
                MemberKind::Data if st.keyword() == "DATA" => fields.push(self.field(st)?),
                MemberKind::Data if st.keyword() == "CLASS-DATA" => {
                    static_fields.push(self.field(st)?)
                }
                MemberKind::Method(sig) => {
                    let (params, ret) = self.parse_method_params(
                        &st.tokens[method_params_start(st)..],
                        st.line,
                    )?;
                    let signature = MethodSig {
                        params,
                        ret,
                        is_static: sig.is_class,
                    };
                    let previous = sigs.insert(sig.name.to_lowercase(), signature);
                    if previous.is_some() && !st.has("REDEFINITION")
                    {
                        return err(st.line, format!("Methode '{}' doppelt", sig.name));
                    }
                }
                MemberKind::Interfaces => {
                    let Some(interface) = st.tokens.get(1).map(|token| token.text.to_lowercase()) else {
                        return err(st.line, "INTERFACES erwartet einen Interfacenamen");
                    };
                    if let Some(methods) = self.interfaces.get(&interface) {
                        interfaces.push(InterfaceRef::Internal(interface.clone()));
                        if let Some(interface_fields) = self.interface_fields.get(&interface) {
                            for (name, ty) in interface_fields {
                                fields_push_unique(
                                    &mut fields,
                                    format!("{interface}~{name}"),
                                    *ty,
                                );
                            }
                        }
                        for (method, sig) in methods {
                            let qualified = if method.contains('~') {
                                method.clone()
                            } else {
                                format!("{interface}~{method}")
                            };
                            sigs.insert(qualified, sig.clone());
                        }
                    } else {
                        interfaces.push(InterfaceRef::External(interface));
                    }
                }
                MemberKind::Aliases => {
                    let Some(alias) = st.tokens.get(1).map(|token| token.text.to_lowercase()) else {
                        return err(st.line, "ALIAS ohne Namen");
                    };
                    let Some(for_index) = st.tokens.iter().position(|token| token.is_word("FOR")) else {
                        return err(st.line, "ALIAS erwartet FOR");
                    };
                    let Some(interface) = st.tokens.get(for_index + 1) else {
                        return err(st.line, "ALIAS ohne Interface");
                    };
                    let Some(method) = st.tokens.get(for_index + 3) else {
                        return err(st.line, "ALIAS ohne Interface-Methode");
                    };
                    aliases.push((
                        alias,
                        format!(
                            "{}~{}",
                            interface.text.to_lowercase(),
                            method.text.to_lowercase()
                        ),
                    ));
                }
                MemberKind::Types => {}
                MemberKind::Constants => {}
                MemberKind::Other if st.keyword() == "INCLUDE" => {}
                _ => return err(st.line, format!("Klassenbestandteil wird noch nicht unterstützt: {}", st.keyword())),
            }
        }
        let struct_idx = self.define_struct(&format!("class {name}"), fields, line)?;
        self.classes.push(ClassDef {
            name: name.clone(),
            struct_idx,
            parent,
            interfaces,
            aliases,
            is_abstract: c.head.has("ABSTRACT"),
            static_fields,
            methods: Vec::new(),
        });
        self.sigs.push(sigs);
        self.scope = None;
        Ok(())
    }

    fn define_local_data_types(
        &mut self,
        members: &[abapgit_inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut index = 0;
        while index < members.len() {
            let member = &members[index];
            if matches!(member.kind, MemberKind::Data)
                && member
                    .statement
                    .tokens
                    .get(1)
                    .is_some_and(|token| token.is_word("BEGIN"))
            {
                let begin = &member.statement;
                let name = word(&begin.tokens[3]);
                let mut fields = Vec::new();
                index += 1;
                while index < members.len() {
                    let component = &members[index].statement;
                    if component
                        .tokens
                        .get(1)
                        .is_some_and(|token| token.is_word("END"))
                    {
                        index += 1;
                        break;
                    }
                    if matches!(members[index].kind, MemberKind::Data) {
                        fields.push(self.field(component)?);
                    }
                    index += 1;
                }
                let qualified = self.qualified_type(&name);
                let structure = self.define_struct(&qualified, fields, begin.line)?;
                self.names.insert(qualified, Ty::Struct(structure));
                continue;
            }
            if !matches!(member.kind, MemberKind::Data)
                || member.statement.keyword() != "DATA"
                || member.statement.tokens.len() < 4
            {
                index += 1;
                continue;
            }
            let type_index = match member.statement.tokens[2].is_word("TYPE")
                || member.statement.tokens[2].is_word("LIKE")
            {
                true => 3,
                false => continue,
            };
            let ty = self.ty_of(
                &member.statement.tokens[type_index..],
                member.statement.line,
            )?;
            self.names.insert(
                self.qualified_type(&member.statement.tokens[1].text),
                ty,
            );
            index += 1;
        }
        Ok(())
    }

    /// Registriert ein Interface als Methodenkatalog für implementierende Klassen.
    pub(super) fn define_interface(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let name = c.name.to_lowercase();
        if self.interfaces.contains_key(&name) {
            return err(c.head.line, format!("Interface '{name}' ist bereits definiert"));
        }
        let mut methods = HashMap::new();
        let mut interface_fields = HashMap::new();
        self.scope = Some(name.clone());
        self.define_local_type_aliases(&c.members)?;
        self.define_local_data_types(&c.members)?;
        self.define_local_structures(&c.members)?;
        self.define_local_constants(&c.members)?;
        for member in &c.members {
            match &member.kind {
                MemberKind::Method(signature) => {
                        let start = method_params_start(&member.statement);
                        let (params, ret) = self.parse_method_params(
                            &member.statement.tokens[start..],
                            member.statement.line,
                        )?;
                    methods.insert(
                        signature.name.to_lowercase(),
                        MethodSig {
                            params,
                            ret,
                            is_static: signature.is_class,
                        },
                    );
                }
                MemberKind::Data => {
                    let (name, ty) = self.field(&member.statement)?;
                    interface_fields.insert(name, ty);
                }
                MemberKind::Interfaces => {
                    let parent = member.statement.tokens[1].text.to_lowercase();
                    if let Some(parent_methods) = self.interfaces.get(&parent).cloned() {
                        for (method, signature) in parent_methods {
                            methods.insert(
                                if method.contains('~') {
                                    method
                                } else {
                                    format!("{parent}~{method}")
                                },
                                signature,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        self.interfaces.insert(name, methods);
        self.interface_fields
            .insert(c.name.to_lowercase(), interface_fields);
        self.scope = None;
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
            self.add_implicit_interface_fields(idx, &m.body);
            let mname = m.name.to_lowercase();
            let Some(sig) = self.sigs[idx].get(&mname).cloned() else {
                return err(m.head.line, format!("Methode '{}' ist nicht in der Klassendefinition deklariert", m.name));
            };
            if self.classes[idx].methods.iter().any(|x| x.name == mname) {
                return err(m.head.line, format!("Methode '{}' doppelt implementiert", m.name));
            }
            let mut body = Vec::new();
            self.scope = Some(name.clone());
            self.lower_nodes(&nodes(&m.body), &mut body)?;
            self.scope = None;
            self.classes[idx].methods.push(ClassMethod {
                name: mname,
                is_static: sig.is_static,
                params: sig.params,
                ret: sig.ret,
                body,
                line: m.head.line,
            });
        }
        Ok(())
    }

    fn add_implicit_interface_fields(&mut self, class_idx: usize, items: &[Item]) {
        let mut names = Vec::new();
        collect_interface_components(items, &mut names);
        let declared_interfaces: BTreeSet<String> = self.classes[class_idx]
            .interfaces
            .iter()
            .map(|reference| match reference {
                InterfaceRef::Internal(name) | InterfaceRef::External(name) => name.clone(),
            })
            .collect();
        let struct_idx = self.classes[class_idx].struct_idx;
        for name in names {
            let Some((interface, _)) = name.split_once('~') else {
                continue;
            };
            if !declared_interfaces.contains(interface)
                || self.structs[struct_idx]
                    .fields
                    .iter()
                    .any(|(field, _)| field == &name)
            {
                continue;
            }
            let ty = self.external_type(&format!("{}=>{}", interface, name.split_once('~').unwrap().1));
            self.structs[struct_idx].fields.push((name, ty));
        }
    }

    /// Jede deklarierte Methode braucht eine Implementierung.
    pub(super) fn check_all_implemented(&self) -> Result<(), CompileError> {
        for (idx, class) in self.classes.iter().enumerate() {
            if class.is_abstract {
                continue;
            }
            for m in self.sigs[idx].keys() {
                let mut implemented = false;
                let mut current = Some(idx);
                while let Some(class_idx) = current {
                    if self.classes[class_idx]
                        .methods
                        .iter()
                        .any(|method| &method.name == m)
                    {
                        implemented = true;
                        break;
                    }
                    current = match self.classes[class_idx].parent {
                        Some(ClassParent::Internal(parent)) => Some(parent),
                        _ => None,
                    };
                }
                if !implemented {
                    return err(0, format!("Methode '{}' der Klasse '{}' ohne Implementierung", m, class.name));
                }
            }
        }
        Ok(())
    }

    /// Parameter einer Methode: `IMPORTING p TYPE t CHANGING q TYPE t` (IMPORTING = Übergabe wie USING).
    pub(super) fn parse_method_params(&mut self, toks: &[Token], line: usize) -> Result<(Vec<Param>, Option<RetParam>), CompileError> {
        let tokens: Vec<Token> = toks
            .iter()
            .filter(|token| {
                !matches!(
                    token.text.to_uppercase().as_str(),
                    "ABSTRACT" | "FINAL" | "REDEFINITION" | "IGNORE"
                )
            })
            .cloned()
            .collect();
        let toks = tokens.as_slice();
        // RETURNING abtrennen; davor IMPORTING/CHANGING wie FORM-Parameter, danach RETURNING.
        let ret_pos = toks.iter().position(|t| t.is_word("RETURNING"));
        let (before, after) = match ret_pos {
            Some(p) => (&toks[..p], Some(&toks[p + 1..])),
            None => (toks, None),
        };
        let mut mapped = Vec::new();
        let mut index = 0;
        while index < before.len() {
            if before[index].is_word("VALUE")
                && before
                    .get(index + 1)
                    .is_some_and(|token| token.is_punct("("))
                && before.get(index + 2).is_some_and(|token| token.kind == Kind::Word)
                && before
                    .get(index + 3)
                    .is_some_and(|token| token.is_punct(")"))
            {
                mapped.push(before[index + 2].clone());
                index += 4;
                continue;
            }
            let mut token = before[index].clone();
            if token.is_word("IMPORTING") || token.is_word("EXPORTING") {
                token.text = "USING".into();
            }
            mapped.push(token);
            index += 1;
        }
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
                let Some(t) = tail
                    .first()
                    .filter(|t| t.is_word("TYPE") || t.is_word("LIKE"))
                else {
                    return err(line, "RETURNING ohne TYPE/LIKE wird nicht unterstützt");
                };
                let _ = t;
                let ty = self.ty_of(&tail[1..], line)?;
                Some(RetParam { name, ty })
            }
        };
        Ok((params, ret))
    }

    /// Komponente `name TYPE typ` innerhalb einer Struktur.
    pub(super) fn field(&mut self, f: &Statement) -> Result<(String, Ty), CompileError> {
        let line = f.line;
        let toks = &f.tokens;
        if toks.len() >= 6
            && toks[1].kind == Kind::Word
            && toks[2].is_punct("(")
            && toks[4].is_punct(")")
            && (toks[5].is_word("TYPE") || toks[5].is_word("LIKE"))
        {
            let _declared_length: usize = toks[3]
                .text
                .parse()
                .or_else(|_| err(line, "ungültige Komponentenlänge"))?;
            let ty = if toks[5].is_word("LIKE") {
                self.ty_of_like(&toks[6..], line)?
            } else {
                self.ty_of(&toks[6..], line)?
            };
            return Ok((word(&toks[1]), ty));
        }
        if toks.len() < 4
            || toks[1].kind != Kind::Word
            || !(toks[2].is_word("TYPE") || toks[2].is_word("LIKE"))
        {
            return err(line, "Komponente erwartet 'name TYPE/LIKE typ'");
        }
        let ty = if toks[2].is_word("LIKE") {
            self.ty_of_like(&toks[3..], line)?
        } else {
            self.ty_of(&toks[3..], line)?
        };
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

    fn define_local_type_aliases(
        &mut self,
        members: &[abapgit_inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut structure_depth = 0usize;
        for member in members {
            let statement = &member.statement;
            if statement.tokens.get(1).is_some_and(|token| token.is_word("BEGIN")) {
                structure_depth += 1;
                continue;
            }
            if statement.tokens.get(1).is_some_and(|token| token.is_word("END")) {
                structure_depth = structure_depth.saturating_sub(1);
                continue;
            }
            if structure_depth > 0 {
                continue;
            }
            if matches!(member.kind, MemberKind::Types)
                && statement.tokens.get(2).is_some_and(|token| token.is_word("TYPE"))
            {
                let name = statement.tokens[1].text.to_lowercase();
                let ty = self.ty_of(&statement.tokens[3..], statement.line)?;
                self.names.insert(self.qualified_type(&name), ty);
            }
        }
        Ok(())
    }

    fn define_local_constants(
        &mut self,
        members: &[abapgit_inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut structure: Option<String> = None;
        for member in members {
            if !matches!(member.kind, MemberKind::Constants) {
                continue;
            }
            let statement = &member.statement;
            if statement.tokens.get(1).is_some_and(|token| token.is_word("BEGIN")) {
                structure = statement.tokens.get(3).map(|token| token.text.to_lowercase());
                continue;
            }
            if statement.tokens.get(1).is_some_and(|token| token.is_word("END")) {
                structure = None;
                continue;
            }
            let Some(value_index) = statement.tokens.iter().rposition(|token| token.is_word("VALUE")) else {
                continue;
            };
            let value_tokens = &statement.tokens[value_index + 1..];
            let end = value_tokens
                .iter()
                .position(|token| token.is_punct("#"))
                .unwrap_or(value_tokens.len());
            let value = super::stmt::parse_constant_expr(
                &value_tokens[..end],
                statement.line,
            )?;
            let name = statement.tokens[1].text.to_lowercase();
            let full_name = structure
                .as_ref()
                .map(|structure| format!("{structure}-{name}"))
                .unwrap_or(name);
            self.constants
                .insert(self.qualified_type(&full_name), value);
        }
        Ok(())
    }

    fn define_local_structures(
        &mut self,
        members: &[abapgit_inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut index = 0;
        while index < members.len() {
            let statement = &members[index].statement;
            if !matches!(members[index].kind, MemberKind::Types) {
                index += 1;
                continue;
            }
            if statement.tokens.get(1).is_some_and(|token| token.is_word("BEGIN")) {
                let (name, ty, next) = self.define_local_structure(members, index)?;
                self.names.insert(name, ty);
                index = next;
                continue;
            }
            index += 1;
        }
        Ok(())
    }

    fn define_local_structure(
        &mut self,
        members: &[abapgit_inspect::abap::Member],
        begin_index: usize,
    ) -> Result<(String, Ty, usize), CompileError> {
        let begin = &members[begin_index].statement;
        let name = begin
            .tokens
            .get(3)
            .ok_or_else(|| CompileError {
                line: begin.line,
                message: "TYPES BEGIN OF ohne Namen".into(),
            })?
            .text
            .to_lowercase();
        let qualified = self.qualified_type(&name);
        let mut fields = Vec::new();
        let mut index = begin_index + 1;
        while index < members.len() {
            let member = &members[index];
            let statement = &member.statement;
            if statement.tokens.get(1).is_some_and(|token| token.is_word("END")) {
                let struct_index = self.define_struct(&qualified, fields, begin.line)?;
                return Ok((qualified, Ty::Struct(struct_index), index + 1));
            }
            if !matches!(member.kind, MemberKind::Types) {
                index += 1;
                continue;
            }
            if statement.tokens.get(1).is_some_and(|token| token.is_word("BEGIN")) {
                let (_, nested, next) = self.define_local_structure(members, index)?;
                let nested_name = statement
                    .tokens
                    .get(3)
                    .map(|token| token.text.to_lowercase())
                    .unwrap_or_default();
                fields.push((nested_name, nested));
                index = next;
            } else {
                if statement.tokens.get(1).is_some_and(|token| token.is_word("INCLUDE")) {
                    let Some(type_token) = statement.tokens.get(3) else {
                        return err(statement.line, "INCLUDE TYPE ohne Typ");
                    };
                    let ty = self.ty_of(&statement.tokens[3..], statement.line)?;
                    if let Ty::Struct(structure) = ty {
                        fields.extend(self.structs[structure].fields.clone());
                    } else {
                        let _ = type_token;
                    }
                    index += 1;
                    continue;
                }
                fields.push(self.field(statement)?);
                index += 1;
            }
        }
        err(begin.line, format!("END OF {name} fehlt"))
    }

    /// Typ aus Tokens: Tabelle, Elementtyp oder Typname.
    pub(super) fn ty_of(&mut self, toks: &[Token], line: usize) -> Result<Ty, CompileError> {
        let end = toks
            .iter()
            .position(|token| {
                token.is_word("READ-ONLY")
                    || token.is_word("OPTIONAL")
                    || token.is_word("DEFAULT")
                    || token.is_word("VALUE")
                    || token.is_word("PREFERRED")
                    || token.is_word("RAISING")
                    || token.is_word("EXCEPTIONS")
                    || token.is_punct("#")
            })
            .unwrap_or(toks.len());
        let toks = &toks[..end];
        if toks.len() >= 2
            && toks.get(1).is_some_and(|token| token.is_word("TABLE"))
            && !toks.iter().any(|token| token.is_word("OF"))
        {
            return Ok(Ty::Table(Elem::External(
                self.external_type_index("table=>any"),
            )));
        }
        if toks.len() >= 3 && toks[0].is_word("RANGE") && toks[1].is_word("OF") {
            let source = &toks[2..];
            let name = format!("range=>{}", source.first().map(word).unwrap_or_default());
            let external = self.external_type_index(&name);
            return Ok(Ty::Table(Elem::External(external)));
        }
        if toks.len() >= 3 && toks[0].is_word("LINE") && toks[1].is_word("OF") {
            let source = word(&toks[2]);
            let qualified = self.qualified_type(&source);
            return match self
                .names
                .get(&qualified)
                .or_else(|| self.names.get(&source))
                .copied()
            {
                Some(Ty::Table(element)) => Ok(element.ty()),
                Some(other) => err(line, format!("'{source}' ist keine interne Tabelle: {other:?}")),
                None => Ok(self.external_type(&format!("line-of=>{qualified}"))),
            };
        }
        if toks.len() == 3
            && (toks[1].is_punct("=>") || toks[1].is_punct("~"))
        {
            let qualified = format!("{}=>{}", word(&toks[0]), word(&toks[2]));
            return match self.names.get(&qualified).copied() {
                Some(ty) => Ok(ty),
                None => Ok(self.external_type(&qualified)),
            };
        }
        if toks.len() >= 3 && toks[0].is_word("REF") && toks[1].is_word("TO") {
            let referenced = word(&toks[2]);
            let qualified = self.qualified_type(&referenced);
            return Ok(match self
                .names
                .get(&referenced)
                .or_else(|| self.names.get(&qualified))
                .copied()
            {
                Some(Ty::Ref(i)) => Ty::Ref(i),
                Some(_) => self.external_type(&format!("ref=>{referenced}")),
                None if self.interfaces.contains_key(&referenced) => {
                    self.external_type(&format!("ref=>{referenced}"))
                }
                None if referenced == "data" || referenced == "object" => {
                    self.external_type(&format!("ref=>{referenced}"))
                }
                None if ["if_", "zif_", "zcl_", "cl_", "cx_", "zcx_", "trwbo_", "/"]
                    .iter()
                    .any(|prefix| referenced.starts_with(prefix))
                    || referenced.starts_with("ty_")
                    || referenced == "devclass" =>
                {
                    self.external_type(&format!("ref=>{referenced}"))
                }
                None => return err(line, format!("Klasse '{referenced}' nicht definiert")),
            });
        }
        if let Some(elem) = table_elem_tokens(toks) {
            let end = elem
                .iter()
                .position(|token| {
                    token.is_word("WITH") || token.is_word("KEY") || token.is_word("DEFAULT")
                })
                .unwrap_or(elem.len());
            if elem.len() >= 3 && elem[0].is_word("REF") && elem[1].is_word("TO") {
                let referenced = word(&elem[2]);
                return Ok(Ty::Table(Elem::External(
                    self.external_type_index(&format!("ref=>{referenced}")),
                )));
            }
            return Ok(Ty::Table(self.elem_of(&elem[..end], line)?));
        }
        if let [token] = toks {
            let name = word(token);
            let qualified = self.qualified_type(&name);
            if let Some(ty) = self
                .names
                .get(&name)
                .or_else(|| self.names.get(&qualified))
                .copied()
            {
                return Ok(ty);
            }
            if !matches!(
                name.as_str(),
                "i" | "int1" | "int2" | "int4" | "int8" | "p" | "f" | "string"
                    | "c" | "n" | "d" | "t" | "x" | "abap_bool" | "any"
                    | "clike" | "csequence"
                    | "data" | "xstring" | "symsgid" | "symsgno" | "symsgv"
                    | "scx_attrname"
            ) && !self.names.contains_key(&name)
                && !self.names.contains_key(&qualified)
            {
                return Ok(self.external_type(&qualified));
            }
        }
        Ok(self.elem_of(toks, line)?.ty())
    }

    pub(super) fn ty_of_like(
        &mut self,
        toks: &[Token],
        line: usize,
    ) -> Result<Ty, CompileError> {
        let end = toks
            .iter()
            .position(|token| {
                token.is_word("OPTIONAL")
                    || token.is_word("DEFAULT")
                    || token.is_word("PREFERRED")
            })
            .unwrap_or(toks.len());
        let toks = &toks[..end];
        if toks.first().is_some_and(|token| token.is_word("LINE"))
            && toks.get(1).is_some_and(|token| token.is_word("OF"))
        {
            if toks.iter().any(|token| token.is_punct("->")) {
                return Ok(self.external_type("line-of=>object-attribute"));
            }
            if toks.len() == 3 {
                let source = word(&toks[2]);
                return match self
                    .data_types
                    .get(&source)
                    .or_else(|| self.names.get(&self.qualified_type(&source)))
                    .copied()
                {
                    Some(Ty::Table(element)) => Ok(element.ty()),
                    Some(_) => Ok(self.external_type(&format!("line-of=>{source}"))),
                    None => Ok(self.external_type(&format!("line-of=>{source}"))),
                };
            }
        }
        if let [owner, arrow, attribute] = toks {
            if arrow.is_punct("=>") || arrow.is_punct("~") || arrow.is_punct("->") {
                let qualified = format!("{}=>{}", word(owner), word(attribute));
                return Ok(self
                    .names
                    .get(&qualified)
                    .copied()
                    .unwrap_or_else(|| self.external_type(&qualified)));
            }
        }
        if let [name] = toks {
            let qualified = self.qualified_type(&word(name));
            if let Some(ty) = self.names.get(&word(name)).copied() {
                return Ok(ty);
            }
            if let Some(ty) = self.names.get(&qualified).copied() {
                return Ok(ty);
            }
            return Ok(self.external_type(&qualified));
        }
        self.ty_of(toks, line)
    }

    pub(super) fn field_symbol_like_type(
        &mut self,
        toks: &[Token],
        line: usize,
    ) -> Result<Ty, CompileError> {
        if toks.len() == 3 && toks[0].is_word("LINE") && toks[1].is_word("OF") {
            let source = word(&toks[2]);
            let qualified = self.qualified_type(&source);
            let source_type = self
                .data_types
                .get(&source)
                .or_else(|| self.names.get(&source))
                .or_else(|| self.names.get(&qualified))
                .copied();
            return match source_type {
                Some(Ty::Table(element)) => Ok(element.ty()),
                Some(Ty::External(_)) => Ok(self.external_type(&format!("line-of=>{qualified}"))),
                Some(_) => err(line, format!("'{source}' ist keine interne Tabelle")),
                None => Ok(self.external_type(&format!("line-of=>{qualified}"))),
            };
        }
        if toks.first().is_some_and(|token| token.is_word("LINE"))
            && toks.get(1).is_some_and(|token| token.is_word("OF"))
            && toks.iter().any(|token| token.is_punct("->"))
        {
            return Ok(self.external_type("line-of=>object-attribute"));
        }
        self.ty_of_like(toks, line)
    }

    /// Elementtyp: `i`, `string`, `c`, `c LENGTH n` oder ein bekannter Typname.
    pub(super) fn elem_of(&mut self, toks: &[Token], line: usize) -> Result<Elem, CompileError> {
        if let Some([owner, arrow, type_name]) = toks.get(..3) {
            if arrow.is_punct("=>") || arrow.is_punct("~") {
                let qualified = format!("{}=>{}", word(owner), word(type_name));
                return self.elem_named(&qualified, line);
            }
        }
        match toks {
            [t] if t.is_word("i") => Ok(Elem::Int),
            [t] if matches!(
                t.text.to_uppercase().as_str(),
                "INT1" | "INT2" | "INT4" | "INT8" | "F" | "P"
            ) => Ok(Elem::Int),
            [t] if t.is_word("string") => Ok(Elem::Str),
            [t] if matches!(
                t.text.to_uppercase().as_str(),
                "ANY" | "CLIKE" | "CSEQUENCE" | "DATA" | "XSTRING"
            ) => Ok(Elem::Str),
            [t] if t.is_word("c") => Ok(Elem::Char(1)),
            [t] if t.is_word("n") => Ok(Elem::Char(1)),
            [t] if t.is_word("d") => Ok(Elem::Char(8)),
            [t] if t.is_word("t") => Ok(Elem::Char(6)),
            [t] if t.is_word("x") => Ok(Elem::Char(1)),
            [t] if t.is_word("abap_bool") => Ok(Elem::Char(1)),
            [t] if t.is_word("symsgid") || t.is_word("scx_attrname") => Ok(Elem::Char(20)),
            [t] if t.is_word("symsgno") => Ok(Elem::Char(3)),
            [t] if t.is_word("symsgv") => Ok(Elem::Char(50)),
            [t, l, n] if t.is_word("c") && l.is_word("LENGTH") => {
                let len: usize = n.text.parse().or_else(|_| err(line, format!("ungültige Länge: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Länge muss größer 0 sein");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n] if t.is_word("n") && l.is_word("LENGTH") => {
                let len: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("ungültige Länge: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Länge muss größer 0 sein");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n] if t.is_word("x") && l.is_word("LENGTH") => {
                let len: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("ungültige Länge: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Länge muss größer 0 sein");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n, d, digits] if t.is_word("p") && l.is_word("LENGTH") && d.is_word("DECIMALS") => {
                let _: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("ungültige Länge: {}", n.text)))?;
                let _: usize = digits
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("ungültige Dezimalstellen: {}", digits.text)))?;
                Ok(Elem::Int)
            }
            [t] => match self.names.get(&word(t)) {
                Some(Ty::Int) => Ok(Elem::Int),
                Some(Ty::Str) => Ok(Elem::Str),
                Some(Ty::Char(n)) => Ok(Elem::Char(*n)),
                Some(Ty::Struct(i)) => Ok(Elem::Struct(*i)),
                Some(Ty::Table(_)) => err(line, format!("Tabelle '{}' kann nicht Element einer Tabelle sein", t.text)),
                Some(Ty::Ref(_)) => err(line, "Objektreferenzen als Tabellenelement werden noch nicht unterstützt"),
                Some(Ty::External(i)) => Ok(Elem::External(*i)),
                None => {
                    let name = self.qualified_type(&word(t));
                    match self.names.get(&name).copied() {
                        Some(Ty::Int) => Ok(Elem::Int),
                        Some(Ty::Str) => Ok(Elem::Str),
                        Some(Ty::Char(n)) => Ok(Elem::Char(n)),
                        Some(Ty::Struct(i)) => Ok(Elem::Struct(i)),
                        Some(Ty::External(i)) => Ok(Elem::External(i)),
                        Some(Ty::Table(_)) => err(
                            line,
                            format!("Tabelle '{}' kann nicht Element einer Tabelle sein", t.text),
                        ),
                        Some(Ty::Ref(_)) => err(
                            line,
                            "Objektreferenzen als Tabellenelement werden noch nicht unterstützt",
                        ),
                        None if word(t).len() == 1 => {
                            err(line, format!("Typ '{}' wird noch nicht unterstützt", t.text))
                        }
                        None => Ok(Elem::External(self.external_type_index(&name))),
                    }
                }
            },
            [t, ..] => err(line, format!("Typ '{}' wird noch nicht unterstützt", t.text)),
            [] => err(line, "Typangabe fehlt"),
        }
    }

    fn elem_named(&mut self, name: &str, line: usize) -> Result<Elem, CompileError> {
        match self.names.get(&name.to_lowercase()) {
            Some(Ty::Int) => Ok(Elem::Int),
            Some(Ty::Str) => Ok(Elem::Str),
            Some(Ty::Char(n)) => Ok(Elem::Char(*n)),
            Some(Ty::Struct(i)) => Ok(Elem::Struct(*i)),
            Some(Ty::Table(_)) => err(line, format!("Tabelle '{name}' kann nicht Element einer Tabelle sein")),
            Some(Ty::Ref(_)) => err(line, "Objektreferenzen als Tabellenelement werden noch nicht unterstützt"),
            Some(Ty::External(i)) => Ok(Elem::External(*i)),
            None => Ok(Elem::External(self.external_type_index(name))),
        }
    }

    /// Formalparameter aus dem Kopf einer FORM: `USING p TYPE t ... CHANGING q TYPE t ...`.
    pub(super) fn parse_params(&mut self, toks: &[Token], line: usize) -> Result<Vec<Param>, CompileError> {
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
            if t.is_word("DEFAULT") {
                i += 1;
                while i < toks.len() {
                    let boundary = matches!(
                        toks[i].text.to_uppercase().as_str(),
                        "USING" | "CHANGING" | "OPTIONAL" | "PREFERRED"
                            | "RETURNING" | "RAISING" | "EXCEPTIONS" | "FOR"
                    );
                    let next_parameter = toks[i].kind == Kind::Word
                        && toks
                            .get(i + 1)
                            .is_some_and(|token| token.is_word("TYPE") || token.is_word("LIKE"));
                    if boundary || next_parameter {
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if t.is_word("RAISING") || t.is_word("EXCEPTIONS") || t.is_word("FOR") {
                break;
            }
            if t.is_word("OPTIONAL") {
                i += 1;
                continue;
            }
            if t.is_word("PREFERRED") {
                i += if toks.get(i + 1).is_some_and(|token| token.is_word("PARAMETER")) {
                    3
                } else {
                    1
                };
                continue;
            }
            if t.kind != Kind::Word {
                return err(line, format!("unerwartetes Token im FORM-Kopf: '{}'", t.text));
            }
            let name = word(t);
            if !toks
                .get(i + 1)
                .is_some_and(|x| x.is_word("TYPE") || x.is_word("LIKE"))
            {
                return err(line, format!("Parameter '{name}' ohne TYPE/LIKE wird nicht unterstützt"));
            }
            // Typ reicht bis zum nächsten Parameter (name TYPE) oder USING/CHANGING
            let start = i + 2;
            let mut end = start;
            while end < toks.len() {
                let x = &toks[end];
                let is_switch = matches!(
                    x.text.to_uppercase().as_str(),
                    "USING" | "CHANGING" | "OPTIONAL" | "DEFAULT" | "PREFERRED"
                        | "RAISING" | "EXCEPTIONS" | "FOR"
                );
                let is_next_param = x.kind == Kind::Word
                    && toks
                        .get(end + 1)
                        .is_some_and(|n| n.is_word("TYPE") || n.is_word("LIKE"));
                if is_switch || is_next_param {
                    break;
                }
                end += 1;
            }
            if start >= end {
                return err(line, format!("Typ für Parameter '{name}' fehlt"));
            }
            let type_kind = &toks[i + 1];
            let ty = if type_kind.is_word("LIKE") {
                self.ty_of_like(&toks[start..end], line)?
            } else {
                self.ty_of(&toks[start..end], line)?
            };
            if out.iter().any(|p| p.name == name) {
                return err(line, format!("Parameter '{name}' doppelt"));
            }
            out.push(Param { name, ty, changing });
            i = end;
        }
        Ok(out)
    }
}

fn fields_push_unique(fields: &mut Vec<(String, Ty)>, name: String, ty: Ty) {
    if !fields.iter().any(|(field, _)| field == &name) {
        fields.push((name, ty));
    }
}

fn collect_interface_components(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        let statement = match item {
            Item::Statement(statement) => Some(statement),
            Item::Block { head, body } => {
                collect_interface_components(body, out);
                Some(head)
            }
        };
        let Some(statement) = statement else {
            continue;
        };
        for tokens in statement.tokens.windows(3) {
            if tokens[0].kind == Kind::Word
                && tokens[1].is_punct("~")
                && tokens[2].kind == Kind::Word
            {
                out.push(format!("{}~{}", word(&tokens[0]), word(&tokens[2])));
            }
        }

    }
}

fn method_params_start(statement: &Statement) -> usize {
    if statement.tokens.get(2).is_some_and(|token| token.is_punct("~")) {
        4
    } else {
        2
    }
}
