//! Typen, Strukturen, Klassen und Formalparameter.

use super::*;

impl Lowerer {
    /// CLASS ... DEFINITION. Schlägt die Definition fehl, wird der vorab eingetragene Klassenname
    /// wieder entfernt, damit spätere Verwendungen keinen ungültigen Klassenindex finden.
    pub(super) fn define_class(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let name = c.name.to_lowercase();
        let result = self.define_class_inner(c);
        if result.is_err() {
            self.names.remove(&name);
        }
        result
    }

    /// CLASS ... DEFINITION: Attribute werden zu einer Struktur, Methodensignaturen gemerkt.
    fn define_class_inner(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let line = c.head.line;
        let name = c.name.to_lowercase();
        if self.names.contains_key(&name) {
            return err(line, format!("Class '{name}' is already defined"));
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
        match &parent {
            Some(ClassParent::External(external)) => {
                self.external_parents.insert(name.clone(), external.clone());
            }
            // Erbt von einer internen Klasse: deren externe Wurzel gilt auch hier
            Some(ClassParent::Internal(_)) => {
                if let Some(root) = parent_name
                    .as_ref()
                    .and_then(|p| self.external_parents.get(p))
                    .cloned()
                {
                    self.external_parents.insert(name.clone(), root);
                }
            }
            None => {}
        }
        let mut fields = match parent {
            Some(ClassParent::Internal(parent_idx)) => {
                let parent_struct = self.classes[parent_idx].struct_idx;
                self.structs[parent_struct].fields.clone()
            }
            _ => Vec::new(),
        };
        let mut static_fields = Vec::new();
        let mut attr_visibility = Vec::new();
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
                        message: format!("Inline structure '{name}' not defined"),
                    })?;
                attr_visibility.push((name.clone(), m.visibility));
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
                MemberKind::Data if st.keyword() == "DATA" => {
                    let (field_name, ty) = self.field(st)?;
                    attr_visibility.push((field_name.clone(), m.visibility));
                    fields.push((field_name, ty));
                }
                MemberKind::Data if st.keyword() == "CLASS-DATA" => {
                    static_fields.push(self.field(st)?)
                }
                MemberKind::Method(sig) => {
                    let (params, ret) =
                        self.parse_method_params(&st.tokens[method_params_start(st)..], st.line)?;
                    let signature = MethodSig {
                        params,
                        ret,
                        is_static: sig.is_class,
                    };
                    let previous = sigs.insert(sig.name.to_lowercase(), signature);
                    if previous.is_some() && !st.has("REDEFINITION") {
                        return err(st.line, format!("Method '{}' declared twice", sig.name));
                    }
                }
                MemberKind::Interfaces => {
                    let Some(interface) = st.tokens.get(1).map(|token| token.text.to_lowercase())
                    else {
                        return err(st.line, "INTERFACES expects an interface name");
                    };
                    if let Some(methods) = self.interfaces.get(&interface) {
                        interfaces.push(InterfaceRef::Internal(interface.clone()));
                        if let Some(interface_fields) = self.interface_fields.get(&interface) {
                            for (name, ty) in interface_fields {
                                fields_push_unique(&mut fields, format!("{interface}~{name}"), *ty);
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
                    let Some(alias) = st.tokens.get(1).map(|token| token.text.to_lowercase())
                    else {
                        return err(st.line, "ALIAS without name");
                    };
                    let Some(for_index) = st.tokens.iter().position(|token| token.is_word("FOR"))
                    else {
                        return err(st.line, "ALIAS expects FOR");
                    };
                    let Some(interface) = st.tokens.get(for_index + 1) else {
                        return err(st.line, "ALIAS without interface");
                    };
                    let Some(method) = st.tokens.get(for_index + 3) else {
                        return err(st.line, "ALIAS without interface method");
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
                _ => {
                    return err(
                        st.line,
                        format!("Class component not supported yet: {}", st.keyword()),
                    )
                }
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
            attr_visibility,
            methods: Vec::new(),
        });
        self.sigs.push(sigs);
        self.scope = None;
        Ok(())
    }

    fn define_local_data_types(
        &mut self,
        members: &[crate::inspect::abap::Member],
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
            self.names
                .insert(self.qualified_type(&member.statement.tokens[1].text), ty);
            index += 1;
        }
        Ok(())
    }

    /// Registriert ein Interface als Methodenkatalog für implementierende Klassen.
    pub(super) fn define_interface(&mut self, c: &AbapClass) -> Result<(), CompileError> {
        let name = c.name.to_lowercase();
        if self.interfaces.contains_key(&name) {
            return err(
                c.head.line,
                format!("Interface '{name}' is already defined"),
            );
        }
        let mut methods = HashMap::new();
        let mut interface_fields = HashMap::new();
        let mut parents = Vec::new();
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
                    if let Some(&index) = self.iface_index.get(&parent) {
                        parents.push(index);
                    }
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
        // Methodentabelle der Codegen-Schicht: qualifizierte Namen, feste Reihenfolge
        let mut list: Vec<InterfaceMethod> = methods
            .iter()
            .map(|(method, sig)| InterfaceMethod {
                name: if method.contains('~') {
                    method.clone()
                } else {
                    format!("{name}~{method}")
                },
                is_static: sig.is_static,
                params: sig.params.clone(),
                ret: sig.ret.clone(),
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        self.iface_index.insert(name.clone(), self.iface_defs.len());
        self.iface_defs.push(InterfaceDef {
            name: name.clone(),
            methods: list,
            parents,
        });
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
            _ => return err(c.head.line, format!("Class '{name}' not defined")),
        };
        for m in &c.methods {
            self.add_implicit_interface_fields(idx, &m.body);
            let mname = m.name.to_lowercase();
            let Some(sig) = self.sigs[idx].get(&mname).cloned() else {
                return err(
                    m.head.line,
                    format!(
                        "Method '{}' is not declared in the class definition",
                        m.name
                    ),
                );
            };
            if self.classes[idx].methods.iter().any(|x| x.name == mname) {
                return err(
                    m.head.line,
                    format!("Method '{}' implemented twice", m.name),
                );
            }
            let mut body = Vec::new();
            self.scope = Some(name.clone());
            // Parameter der Methode sind im Rumpf als Variablen bekannt (z.B. für LIKE)
            let mut bindings: Vec<(String, Ty)> =
                sig.params.iter().map(|p| (p.name.clone(), p.ty)).collect();
            if let Some(ret) = &sig.ret {
                bindings.push((ret.name.clone(), ret.ty));
            }
            let saved: Vec<(String, Option<Ty>)> = bindings
                .into_iter()
                .map(|(name, ty)| (name.clone(), self.data_types.insert(name, ty)))
                .collect();
            let result = self.lower_nodes(&nodes(&m.body), &mut body);
            for (param, previous) in saved {
                match previous {
                    Some(ty) => self.data_types.insert(param, ty),
                    None => self.data_types.remove(&param),
                };
            }
            result?;
            self.scope = None;
            if let Some((param, line)) = importing_write(&sig.params, &body) {
                return err(
                    line,
                    format!("Importing parameter '{param}' is read-only and cannot be changed"),
                );
            }
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
            let ty = self.external_type(&format!(
                "{}=>{}",
                interface,
                name.split_once('~').unwrap().1
            ));
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
                    return err(
                        0,
                        format!(
                            "Method '{}' of class '{}' has no implementation",
                            m, class.name
                        ),
                    );
                }
            }
        }
        Ok(())
    }

    /// Parameter einer Methode: `IMPORTING p TYPE t CHANGING q TYPE t` (IMPORTING = Übergabe wie USING).
    pub(super) fn parse_method_params(
        &mut self,
        toks: &[Token],
        line: usize,
    ) -> Result<(Vec<Param>, Option<RetParam>), CompileError> {
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
                && before
                    .get(index + 2)
                    .is_some_and(|token| token.kind == Kind::Word)
                && before
                    .get(index + 3)
                    .is_some_and(|token| token.is_punct(")"))
            {
                mapped.push(before[index + 2].clone());
                index += 4;
                continue;
            }
            // IMPORTING und EXPORTING bleiben erhalten; `parse_params` kennt beide Abschnitte
            mapped.push(before[index].clone());
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
                    [v, p, n, c, tail @ ..]
                        if v.is_word("VALUE") && p.is_punct("(") && c.is_punct(")") =>
                    {
                        (word(n), tail)
                    }
                    [n, tail @ ..] if n.kind == Kind::Word => (word(n), tail),
                    _ => return err(line, "RETURNING expects 'VALUE(name) TYPE type'"),
                };
                let Some(t) = tail
                    .first()
                    .filter(|t| t.is_word("TYPE") || t.is_word("LIKE"))
                else {
                    return err(line, "RETURNING without TYPE/LIKE is not supported");
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
                .or_else(|_| err(line, "Invalid component length"))?;
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
            return err(line, "Component expects 'name TYPE/LIKE type'");
        }
        let ty = if toks[2].is_word("LIKE") {
            self.ty_of_like(&toks[3..], line)?
        } else {
            self.ty_of(&toks[3..], line)?
        };
        Ok((word(&toks[1]), ty))
    }

    pub(super) fn define_struct(
        &mut self,
        name: &str,
        fields: Vec<(String, Ty)>,
        line: usize,
    ) -> Result<usize, CompileError> {
        let mut seen = std::collections::HashSet::new();
        for (f, _) in &fields {
            if !seen.insert(f.clone()) {
                return err(
                    line,
                    format!("Component '{f}' declared twice in structure '{name}'"),
                );
            }
        }
        self.structs.push(StructDef {
            name: name.to_lowercase(),
            fields,
        });
        Ok(self.structs.len() - 1)
    }

    fn define_local_type_aliases(
        &mut self,
        members: &[crate::inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut structure_depth = 0usize;
        for member in members {
            let statement = &member.statement;
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("BEGIN"))
            {
                structure_depth += 1;
                continue;
            }
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("END"))
            {
                structure_depth = structure_depth.saturating_sub(1);
                continue;
            }
            if structure_depth > 0 {
                continue;
            }
            if matches!(member.kind, MemberKind::Types)
                && statement
                    .tokens
                    .get(2)
                    .is_some_and(|token| token.is_word("TYPE"))
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
        members: &[crate::inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut structure: Option<String> = None;
        for member in members {
            if !matches!(member.kind, MemberKind::Constants) {
                continue;
            }
            let statement = &member.statement;
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("BEGIN"))
            {
                structure = statement
                    .tokens
                    .get(3)
                    .map(|token| token.text.to_lowercase());
                continue;
            }
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("END"))
            {
                structure = None;
                continue;
            }
            let Some(value_index) = statement
                .tokens
                .iter()
                .rposition(|token| token.is_word("VALUE"))
            else {
                continue;
            };
            let value_tokens = &statement.tokens[value_index + 1..];
            let end = value_tokens
                .iter()
                .position(|token| token.is_punct("#"))
                .unwrap_or(value_tokens.len());
            let value = super::stmt::parse_constant_expr(&value_tokens[..end], statement.line)?;
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
        members: &[crate::inspect::abap::Member],
    ) -> Result<(), CompileError> {
        let mut index = 0;
        while index < members.len() {
            let statement = &members[index].statement;
            if !matches!(members[index].kind, MemberKind::Types) {
                index += 1;
                continue;
            }
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("BEGIN"))
            {
                let (name, ty, next) = self.define_local_structure(members, index)?;
                self.names.insert(name, ty);
                index = next;
                continue;
            }
            index += 1;
        }
        Ok(())
    }

    /// Felder aus `INCLUDE TYPE name` (eigene Anweisung, Name einfach oder qualifiziert).
    /// Liefert None, wenn die Anweisung kein INCLUDE ist (z.B. ein Feld namens include).
    pub(super) fn include_type_fields(
        &mut self,
        statement: &crate::inspect::abap::Statement,
    ) -> Result<Option<Vec<(String, Ty)>>, CompileError> {
        if statement
            .tokens
            .first()
            .is_none_or(|t| !t.is_word("INCLUDE"))
        {
            return Ok(None);
        }
        let start = 2;
        let len = if statement
            .tokens
            .get(start + 1)
            .is_some_and(|t| t.is_punct("=>") || t.is_punct("~"))
        {
            3
        } else {
            1
        };
        let Some(name_tokens) = statement.tokens.get(start..start + len) else {
            return err(statement.line, "INCLUDE TYPE without type");
        };
        let label: String = name_tokens.iter().map(|t| t.text.as_str()).collect();
        match self.ty_of(name_tokens, statement.line)? {
            Ty::Struct(structure) => Ok(Some(self.structs[structure].fields.clone())),
            _ => err(
                statement.line,
                format!("INCLUDE {label}: type is not a structure"),
            ),
        }
    }

    fn define_local_structure(
        &mut self,
        members: &[crate::inspect::abap::Member],
        begin_index: usize,
    ) -> Result<(String, Ty, usize), CompileError> {
        let begin = &members[begin_index].statement;
        let name = begin
            .tokens
            .get(3)
            .ok_or_else(|| CompileError {
                line: begin.line,
                message: "TYPES BEGIN OF without name".into(),
            })?
            .text
            .to_lowercase();
        let qualified = self.qualified_type(&name);
        let mut fields = Vec::new();
        let mut index = begin_index + 1;
        while index < members.len() {
            let member = &members[index];
            let statement = &member.statement;
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("END"))
            {
                let struct_index = self.define_struct(&qualified, fields, begin.line)?;
                return Ok((qualified, Ty::Struct(struct_index), index + 1));
            }
            // INCLUDE TYPE gehört zur Struktur, auch wenn der Parser es nicht als TYPES einordnet
            if !matches!(member.kind, MemberKind::Types) && statement.keyword() != "INCLUDE" {
                index += 1;
                continue;
            }
            if statement
                .tokens
                .get(1)
                .is_some_and(|token| token.is_word("BEGIN"))
            {
                let (_, nested, next) = self.define_local_structure(members, index)?;
                let nested_name = statement
                    .tokens
                    .get(3)
                    .map(|token| token.text.to_lowercase())
                    .unwrap_or_default();
                fields.push((nested_name, nested));
                index = next;
            } else {
                // INCLUDE TYPE name als eigene Anweisung
                if let Some(included) = self.include_type_fields(statement)? {
                    fields.extend(included);
                    index += 1;
                    continue;
                }
                fields.push(self.field(statement)?);
                index += 1;
            }
        }
        err(begin.line, format!("END OF {name} missing"))
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
                Some(other) => err(
                    line,
                    format!("'{source}' is not an internal table: {other:?}"),
                ),
                None => Ok(self.external_type(&format!("line-of=>{qualified}"))),
            };
        }
        if toks.len() == 3 && (toks[1].is_punct("=>") || toks[1].is_punct("~")) {
            let qualified = format!("{}=>{}", word(&toks[0]), word(&toks[2]));
            return match self.names.get(&qualified).copied() {
                Some(ty) => Ok(ty),
                None => Ok(self.external_type(&qualified)),
            };
        }
        if toks.len() >= 3 && toks[0].is_word("REF") && toks[1].is_word("TO") {
            let referenced = word(&toks[2]);
            if let Some(&index) = self.iface_index.get(&referenced) {
                return Ok(Ty::Iface(index));
            }
            let qualified = self.qualified_type(&referenced);
            return Ok(
                match self
                    .names
                    .get(&referenced)
                    .or_else(|| self.names.get(&qualified))
                    .copied()
                {
                    Some(Ty::Ref(i)) => Ty::Ref(i),
                    Some(_) => self.external_type(&referenced),
                    None if self.interfaces.contains_key(&referenced) => {
                        self.external_type(&referenced)
                    }
                    None if referenced == "data" || referenced == "object" => {
                        self.external_type(&referenced)
                    }
                    None if ["if_", "zif_", "zcl_", "cl_", "cx_", "zcx_", "trwbo_", "/"]
                        .iter()
                        .any(|prefix| referenced.starts_with(prefix))
                        || referenced.starts_with("ty_")
                        || referenced == "devclass" =>
                    {
                        self.external_type(&referenced)
                    }
                    None => return err(line, format!("Class '{referenced}' not defined")),
                },
            );
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
                    self.external_type_index(&referenced),
                )));
            }
            return Ok(Ty::Table(self.elem_of(&elem[..end], line)?));
        }
        if let [token] = toks {
            let name = word(token);
            // Systemfelder haben festen Typ, auch als `TYPE sy-ucomm` (sonst passt der Formalparameter nicht)
            if let Some(ty) = system_field_type(&name) {
                return Ok(ty);
            }
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
                "i" | "int1"
                    | "int2"
                    | "int4"
                    | "int8"
                    | "p"
                    | "f"
                    | "string"
                    | "c"
                    | "n"
                    | "d"
                    | "t"
                    | "x"
                    | "abap_bool"
                    | "any"
                    | "clike"
                    | "csequence"
                    | "data"
                    | "xstring"
                    | "symsgid"
                    | "symsgno"
                    | "symsgv"
                    | "scx_attrname"
            ) && !self.names.contains_key(&name)
                && !self.names.contains_key(&qualified)
            {
                return Ok(self.external_type(&qualified));
            }
        }
        Ok(self.elem_of(toks, line)?.ty())
    }

    /// Typ eines Attributs der laufenden Klasse, auch Komponente (`attr-komp`). Die Struktur der Klasse
    /// enthält die Attribute samt geerbter Felder.
    pub(super) fn class_attribute_type(&self, name: &str) -> Option<Ty> {
        let scope = self.scope.as_ref()?;
        let Some(Ty::Ref(index)) = self.names.get(scope) else {
            return None;
        };
        let class = self.classes.get(*index)?;
        let fields = &self.structs[class.struct_idx].fields;
        match name.split_once('-') {
            None => fields.iter().find(|(f, _)| f == name).map(|(_, ty)| *ty),
            Some((base, component)) => {
                let (_, ty) = fields.iter().find(|(f, _)| f == base)?;
                match ty {
                    Ty::Struct(i) => self.structs[*i]
                        .fields
                        .iter()
                        .find(|(f, _)| f == component)
                        .map(|(_, ty)| *ty),
                    _ => None,
                }
            }
        }
    }

    pub(super) fn ty_of_like(&mut self, toks: &[Token], line: usize) -> Result<Ty, CompileError> {
        let end = toks
            .iter()
            .position(|token| {
                token.is_word("OPTIONAL") || token.is_word("DEFAULT") || token.is_word("PREFERRED")
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
            // Systemfelder mit festem Typ
            if let Some(ty) = system_field_type(&word(name)) {
                return Ok(ty);
            }
            // Attribut der eigenen Klasse (auch geerbt)
            if let Some(ty) = self.class_attribute_type(&word(name)) {
                return Ok(ty);
            }
            // Komponente einer bekannten Struktur: LIKE a-b
            if let Some((base, component)) = word(name).split_once('-') {
                if let Some(Ty::Struct(index)) = self.data_types.get(base).copied() {
                    if let Some((_, ty)) = self.structs[index]
                        .fields
                        .iter()
                        .find(|(f, _)| f == component)
                    {
                        return Ok(*ty);
                    }
                }
            }
            // Lokale Variable oder Parameter der laufenden Verarbeitung
            if let Some(ty) = self.data_types.get(&word(name)).copied() {
                return Ok(ty);
            }
            if let Some(ty) = self.names.get(&word(name)).copied() {
                return Ok(ty);
            }
            if let Some(ty) = self.names.get(&qualified).copied() {
                return Ok(ty);
            }
            // Unqualifizierter Name, den die Klasse nicht kennt: Attribut der externen Oberklasse
            let parent = self
                .scope
                .as_ref()
                .and_then(|scope| self.external_parents.get(scope))
                .cloned();
            return match parent {
                Some(parent) => Ok(self.external_type(&format!("{parent}=>{}", word(name)))),
                None => err(line, format!("LIKE {}: name is unknown", word(name))),
            };
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
                Some(_) => err(line, format!("'{source}' is not an internal table")),
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
            ) =>
            {
                Ok(Elem::Int)
            }
            [t] if t.is_word("string") => Ok(Elem::Str),
            [t] if matches!(
                t.text.to_uppercase().as_str(),
                "ANY" | "CLIKE" | "CSEQUENCE" | "DATA" | "XSTRING"
            ) =>
            {
                Ok(Elem::Str)
            }
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
                let len: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("Invalid length: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Length must be greater than 0");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n] if t.is_word("n") && l.is_word("LENGTH") => {
                let len: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("Invalid length: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Length must be greater than 0");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n] if t.is_word("x") && l.is_word("LENGTH") => {
                let len: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("Invalid length: {}", n.text)))?;
                if len == 0 {
                    return err(line, "Length must be greater than 0");
                }
                Ok(Elem::Char(len))
            }
            [t, l, n, d, digits]
                if t.is_word("p") && l.is_word("LENGTH") && d.is_word("DECIMALS") =>
            {
                let _: usize = n
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("Invalid length: {}", n.text)))?;
                let _: usize = digits
                    .text
                    .parse()
                    .or_else(|_| err(line, format!("Invalid decimal places: {}", digits.text)))?;
                Ok(Elem::Int)
            }
            [t] => match self.names.get(&word(t)) {
                Some(Ty::Int) => Ok(Elem::Int),
                Some(Ty::Str) => Ok(Elem::Str),
                Some(Ty::Char(n)) => Ok(Elem::Char(*n)),
                Some(Ty::Struct(i)) => Ok(Elem::Struct(*i)),
                Some(Ty::Table(_)) => err(
                    line,
                    format!("Table '{}' cannot be an element of a table", t.text),
                ),
                Some(Ty::Ref(_) | Ty::Iface(_)) => err(
                    line,
                    "Object references as table elements not supported yet",
                ),
                Some(Ty::External(i)) => Ok(Elem::External(*i)),
                Some(Ty::Dynamic) => err(
                    line,
                    "Dynamically typed field symbol cannot be an element of a table",
                ),
                Some(Ty::Packed { .. }) => {
                    err(line, "DEC fields as table elements not supported yet")
                }
                None => {
                    let name = self.qualified_type(&word(t));
                    match self.names.get(&name).copied() {
                        Some(Ty::Int) => Ok(Elem::Int),
                        Some(Ty::Str) => Ok(Elem::Str),
                        Some(Ty::Char(n)) => Ok(Elem::Char(n)),
                        Some(Ty::Struct(i)) => Ok(Elem::Struct(i)),
                        Some(Ty::External(i)) => Ok(Elem::External(i)),
                        Some(Ty::Dynamic) => err(
                            line,
                            "Dynamically typed field symbol cannot be an element of a table",
                        ),
                        Some(Ty::Packed { .. }) => {
                            err(line, "DEC fields as table elements not supported yet")
                        }
                        Some(Ty::Table(_)) => err(
                            line,
                            format!("Table '{}' cannot be an element of a table", t.text),
                        ),
                        Some(Ty::Ref(_) | Ty::Iface(_)) => err(
                            line,
                            "Object references as table elements not supported yet",
                        ),
                        None if word(t).len() == 1 => {
                            err(line, format!("Type '{}' is not supported yet", t.text))
                        }
                        None => Ok(Elem::External(self.external_type_index(&name))),
                    }
                }
            },
            [t, ..] => err(line, format!("Type '{}' is not supported yet", t.text)),
            [] => err(line, "Type specification missing"),
        }
    }

    fn elem_named(&mut self, name: &str, line: usize) -> Result<Elem, CompileError> {
        match self.names.get(&name.to_lowercase()) {
            Some(Ty::Int) => Ok(Elem::Int),
            Some(Ty::Str) => Ok(Elem::Str),
            Some(Ty::Char(n)) => Ok(Elem::Char(*n)),
            Some(Ty::Struct(i)) => Ok(Elem::Struct(*i)),
            Some(Ty::Table(_)) => err(
                line,
                format!("Table '{name}' cannot be an element of a table"),
            ),
            Some(Ty::Ref(_) | Ty::Iface(_)) => err(
                line,
                "Object references as table elements not supported yet",
            ),
            Some(Ty::External(i)) => Ok(Elem::External(*i)),
            Some(Ty::Dynamic) => err(
                line,
                "Dynamically typed field symbol cannot be an element of a table",
            ),
            Some(Ty::Packed { .. }) => err(line, "DEC fields as table elements not supported yet"),
            None => Ok(Elem::External(self.external_type_index(name))),
        }
    }

    /// Formalparameter aus dem Kopf einer FORM: `USING p TYPE t ... CHANGING q TYPE t ...`.
    pub(super) fn parse_params(
        &mut self,
        toks: &[Token],
        line: usize,
    ) -> Result<Vec<Param>, CompileError> {
        let mut out: Vec<Param> = Vec::new();
        let mut changing = false;
        let mut importing = false;
        let mut i = 0;
        while i < toks.len() {
            let t = &toks[i];
            if t.is_word("USING") || t.is_word("IMPORTING") || t.is_word("EXPORTING") {
                changing = false;
                importing = t.is_word("IMPORTING");
                i += 1;
                continue;
            }
            if t.is_word("CHANGING") {
                changing = true;
                importing = false;
                i += 1;
                continue;
            }
            if t.is_word("DEFAULT") {
                i += 1;
                while i < toks.len() {
                    let boundary = matches!(
                        toks[i].text.to_uppercase().as_str(),
                        "USING"
                            | "CHANGING"
                            | "OPTIONAL"
                            | "PREFERRED"
                            | "RETURNING"
                            | "RAISING"
                            | "EXCEPTIONS"
                            | "FOR"
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
                i += if toks
                    .get(i + 1)
                    .is_some_and(|token| token.is_word("PARAMETER"))
                {
                    3
                } else {
                    1
                };
                continue;
            }
            if t.kind != Kind::Word {
                return err(
                    line,
                    format!("Unexpected token in FORM header: '{}'", t.text),
                );
            }
            let name = word(t);
            if !toks
                .get(i + 1)
                .is_some_and(|x| x.is_word("TYPE") || x.is_word("LIKE"))
            {
                return err(
                    line,
                    format!("Parameter '{name}' without TYPE/LIKE is not supported"),
                );
            }
            // Typ reicht bis zum nächsten Parameter (name TYPE) oder USING/CHANGING
            let start = i + 2;
            let mut end = start;
            while end < toks.len() {
                let x = &toks[end];
                let is_switch = matches!(
                    x.text.to_uppercase().as_str(),
                    "USING"
                        | "IMPORTING"
                        | "EXPORTING"
                        | "CHANGING"
                        | "OPTIONAL"
                        | "DEFAULT"
                        | "PREFERRED"
                        | "RAISING"
                        | "EXCEPTIONS"
                        | "FOR"
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
                return err(line, format!("Type for parameter '{name}' missing"));
            }
            let type_kind = &toks[i + 1];
            let ty = if type_kind.is_word("LIKE") {
                self.ty_of_like(&toks[start..end], line)?
            } else {
                self.ty_of(&toks[start..end], line)?
            };
            if out.iter().any(|p| p.name == name) {
                return err(line, format!("Parameter '{name}' declared twice"));
            }
            out.push(Param {
                name,
                ty,
                changing,
                importing,
            });
            i = end;
        }
        Ok(out)
    }
}

/// Fester Typ der Systemfelder (SAP-Standard), wenn ein Typname `sy-...` ist. Deklaration (`DATA`),
/// Formalparameter (`IMPORTING`) und `LIKE` verwenden dieselbe Tabelle, damit die Typen übereinstimmen.
fn system_field_type(name: &str) -> Option<Ty> {
    match name {
        "sy-subrc" | "sy-tabix" | "sy-index" | "sy-dbcnt" | "sy-cucol" | "sy-curow"
        | "sy-fdpos" | "sy-tfill" => Some(Ty::Int),
        "sy-repid" => Some(Ty::Char(40)),
        "sy-uname" => Some(Ty::Char(12)),
        "sy-ucomm" => Some(Ty::Char(crate::dynpro::UCOMM_LENGTH)),
        "sy-dynnr" => Some(Ty::Char(4)),
        "sy-langu" | "sy-msgty" => Some(Ty::Char(1)),
        _ => None,
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

/// Erster Schreibzugriff im Rumpf auf einen IMPORTING-Parameter: Name und Zeile.
/// Erkannt werden Zuweisungen, Löschen, Anfügen, Zeilen- und Ergebnisziele, Aufrufe mit CHANGING und
/// SQL-Ziele. Komponenten `s-feld` gehören zum Parameter `s`. Nicht erkannt: Schreiben über ein
/// Feldsymbol, das auf den Parameter zeigt (`LOOP ... ASSIGNING`).
fn importing_write(params: &[Param], body: &[Stmt]) -> Option<(String, usize)> {
    let names: Vec<&str> = params
        .iter()
        .filter(|p| p.importing)
        .map(|p| p.name.as_str())
        .collect();
    if names.is_empty() {
        return None;
    }
    body_write(body, &names)
}

/// Name des geschriebenen Ziels; Attribute `obj->attr` gehören nicht zu einem Parameter.
fn target_base(target: &Target) -> Option<&str> {
    match target {
        Target::Var(name) => Some(name.as_str()),
        Target::Slice { name, .. } => Some(name.as_str()),
        Target::Attr { .. } => None,
    }
}

fn body_write(body: &[Stmt], names: &[&str]) -> Option<(String, usize)> {
    body.iter().find_map(|s| written_in(s, names))
}

fn written_in(stmt: &Stmt, names: &[&str]) -> Option<(String, usize)> {
    let hit = |name: &str, line: usize| -> Option<(String, usize)> {
        let base = name.split('-').next().unwrap_or(name);
        names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(base))
            .then(|| (base.to_string(), line))
    };
    match stmt {
        Stmt::Assign { target, line, .. } | Stmt::Clear { target, line } => {
            target_base(target).and_then(|n| hit(n, *line))
        }
        Stmt::AssignComponent { target, line, .. }
        | Stmt::ReadTable { target, line, .. }
        | Stmt::Describe { target, line, .. }
        | Stmt::Sql { target, line, .. }
        | Stmt::Create {
            name: target, line, ..
        } => hit(target, *line),
        Stmt::Append { table, line, .. } => hit(table, *line),
        Stmt::Loop {
            target: LoopTarget::Into(name),
            body,
            line,
            ..
        } => hit(name, *line).or_else(|| body_write(body, names)),
        Stmt::Loop { body, .. } => body_write(body, names),
        Stmt::SqlLoop {
            target, body, line, ..
        } => hit(target, *line).or_else(|| body_write(body, names)),
        Stmt::MethodCall {
            args,
            receiving,
            line,
            ..
        } => receiving
            .as_ref()
            .and_then(target_base)
            .and_then(|n| hit(n, *line))
            .or_else(|| {
                args.iter().find_map(|a| match &a.expr {
                    Expr::Var(n) if a.changing == Some(true) => hit(n, *line),
                    _ => None,
                })
            }),
        Stmt::Perform { args, line, .. } => args.iter().find_map(|a| match &a.expr {
            Expr::Var(n) if a.changing => hit(n, *line),
            _ => None,
        }),
        Stmt::If {
            branches,
            otherwise,
            ..
        } => branches
            .iter()
            .find_map(|(_, b)| body_write(b, names))
            .or_else(|| body_write(otherwise, names)),
        Stmt::Case { whens, other, .. } => whens
            .iter()
            .find_map(|(_, b)| body_write(b, names))
            .or_else(|| body_write(other, names)),
        Stmt::Do { body, .. } | Stmt::While { body, .. } => body_write(body, names),
        Stmt::Try {
            body,
            catches,
            cleanup,
            ..
        } => body_write(body, names)
            .or_else(|| catches.iter().find_map(|c| body_write(&c.body, names)))
            .or_else(|| body_write(cleanup, names)),
        _ => None,
    }
}

fn method_params_start(statement: &Statement) -> usize {
    if statement
        .tokens
        .get(2)
        .is_some_and(|token| token.is_punct("~"))
    {
        4
    } else {
        2
    }
}
