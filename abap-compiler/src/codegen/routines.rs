//! FORM, PERFORM, Methoden und Aufrufe.

use super::*;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn has_method(&self, class_idx: usize, method: &str) -> bool {
        let mut current = Some(class_idx);
        while let Some(index) = current {
            if self.classes[index]
                .methods
                .iter()
                .any(|candidate| candidate.name == method)
            {
                return true;
            }
            current = self.classes[index].parent;
        }
        false
    }

    /// Erzeugt den Rumpf einer FORM als LLVM-Funktion mit Zeigern als Parametern.
    pub(super) fn emit_form(&mut self, f: &Form) -> CResult<()> {
        let sig = match self.forms.get(&f.name) {
            Some(s) => s.clone(),
            None => return err(f.line, "FORM nicht deklariert"),
        };
        self.emit_routine(sig.f, &f.params, &f.body, f.line, None, None)
    }

    /// Gemeinsamer Rumpf für FORMs und Methoden. Methoden erhalten den Objektzeiger als ersten Parameter.
    pub(super) fn emit_routine(
        &mut self,
        func: FunctionValue<'ctx>,
        params: &[crate::lower::Param],
        body_stmts: &[Stmt],
        line: usize,
        self_class: Option<usize>,
        ret: Option<(&str, Ty)>,
    ) -> CResult<()> {
        let entry = self.ctx.append_basic_block(func, "entry");
        let body = self.ctx.append_basic_block(func, "body");
        let exit = self.ctx.append_basic_block(func, "exit");

        self.vars.clear();
        self.loops.clear();
        self.exception_target = None;
        self.in_form = true;
        self.body = Some(body);
        self.exit_fn = Some(exit);
        self.self_ctx = None;
        self.exception_target = None;

        self.alloca.position_at_end(entry);
        let mut offset = 0u32;
        if let Some(ci) = self_class {
            let obj = match func.get_nth_param(0) {
                Some(a) => a.into_pointer_value(),
                None => return err(line, "Objektzeiger fehlt"),
            };
            let slot = llvm(self.alloca.build_alloca(self.ptrt, "me"))?;
            llvm(self.alloca.build_store(slot, obj))?;
            self.self_ctx = Some((slot, ci));
            offset = 1;
        }
        for (i, p) in params.iter().enumerate() {
            let arg = match func.get_nth_param(i as u32 + offset) {
                Some(a) => a.into_pointer_value(),
                None => return err(line, "Parameter fehlt"),
            };
            // Parameter ist ein Zeiger auf den Speicher des Aufrufers; wie ein Feldsymbol abgelegt
            let slot = llvm(self.alloca.build_alloca(self.ptrt, &p.name))?;
            llvm(self.alloca.build_store(slot, arg))?;
            self.vars.insert(p.name.clone(), Var { slot, ty: p.ty, fs: true, tag: None });
        }
        if let Some((rname, rty)) = ret {
            let idx = params.len() as u32 + offset;
            let arg = match func.get_nth_param(idx) {
                Some(a) => a.into_pointer_value(),
                None => return err(line, "Rückgabeparameter fehlt"),
            };
            let slot = llvm(self.alloca.build_alloca(self.ptrt, rname))?;
            llvm(self.alloca.build_store(slot, arg))?;
            self.vars.insert(rname.to_string(), Var { slot, ty: rty, fs: true, tag: None });
        }

        self.hoist(body_stmts)?;
        self.builder.position_at_end(body);
        self.emit_block(body_stmts)?;
        self.br_if_open(exit)?;

        self.builder.position_at_end(exit);
        llvm(self.builder.build_return(None))?;
        llvm(self.alloca.build_unconditional_branch(body))?;

        self.in_form = false;
        self.vars.clear();
        self.self_ctx = None;
        Ok(())
    }

    /// Methodenrumpf einer Klasse.
    pub(super) fn emit_method(&mut self, ci: usize, m: &crate::lower::ClassMethod) -> CResult<()> {
        let func = match self.classes[ci].methods.iter().find(|x| x.name == m.name) {
            Some(mi) => mi.f,
            None => return err(m.line, "Methode nicht deklariert"),
        };
        let ret = m.ret.as_ref().map(|r| (r.name.as_str(), r.ty));
        self.emit_routine(
            func,
            &m.params,
            &m.body,
            m.line,
            (!m.is_static).then_some(ci),
            ret,
        )
    }

    /// PERFORM: Aktualparameter per Referenz übergeben; Literale werden in Temporäre kopiert.
    pub(super) fn emit_perform(&mut self, name: &str, args: &[Arg], line: usize) -> CResult<()> {
        let Some(sig) = self.forms.get(name).cloned() else {
            return err(line, format!("FORM '{name}' ist nicht definiert"));
        };
        let ptrs = self.arg_ptrs(args, &sig.params, name, line)?;
        self.call_void(sig.f, &ptrs)
    }

    /// Methodenaufruf als Statement: Rückgabewert (falls vorhanden) wird verworfen.
    /// Methodenaufruf als Statement. Ohne RECEIVING wird ein Rückgabewert verworfen.
    pub(super) fn emit_method_call(
        &mut self,
        recv: &str,
        method: &str,
        args: &[NamedArg],
        receiving: Option<&Target>,
        line: usize,
    ) -> CResult<()> {
        let v = self.emit_method_call_val(recv, method, args, line)?;
        match receiving {
            None => Ok(()),
            Some(t) => {
                let Some(v) = v else {
                    return err(line, format!("Methode '{method}' liefert keinen Wert"));
                };
                let dst = self.dest(t, line)?;
                self.store(dst, v, line)
            }
        }
    }

    /// Methodenaufruf mit Rückgabewert: Objektzeiger vorne, Rückgabe-Temporär hinten.
    pub(super) fn emit_method_call_val(
        &mut self,
        recv: &str,
        method: &str,
        args: &[NamedArg],
        line: usize,
    ) -> CResult<Option<Val<'ctx>>> {
        if recv.eq_ignore_ascii_case("super") {
            return Ok(None);
        }
        let static_call = self
            .classes
            .iter()
            .position(|class| class.name == recv)
            .and_then(|class_idx| {
                self.classes[class_idx]
                    .methods
                    .iter()
                    .find(|candidate| candidate.name == method && candidate.is_static)
                    .cloned()
                    .map(|method| (method, class_idx))
            });
        let (object, c, inherited) = if let Some((method, class_idx)) = &static_call {
            (None, *class_idx, Some(method.clone()))
        } else {
            let (object, class_idx) = self.object(recv, line)?;
            (Some(object), class_idx, None)
        };
        let resolved_method = self.classes[c]
            .aliases
            .get(method)
            .cloned()
            .unwrap_or_else(|| method.to_string());
        let method = resolved_method.as_str();
        let mut class = Some(c);
        let mut mi = inherited;
        while let Some(class_idx) = class {
            if let Some(found) = self.classes[class_idx]
                .methods
                .iter()
                .find(|candidate| candidate.name == method && candidate.is_static == static_call.is_some())
            {
                mi = Some(found.clone());
                break;
            }
            class = self.classes[class_idx].parent;
        }
        let Some(mi) = mi else {
            return err(line, format!("Methode '{method}' unbekannt"));
        };
        let mut ptrs: Vec<BasicMetadataValueEnum<'ctx>> = match object {
            Some(object) => vec![object.into()],
            None => Vec::new(),
        };
        ptrs.extend(self.bind_method_args(&mi, args, method, line)?);
        match mi.ret {
            None => {
                self.call_void(mi.f, &ptrs)?;
                Ok(None)
            }
            Some(rty) => {
                let tmp = llvm(self.builder.build_alloca(self.llvm_ty(rty), "ret_tmp"))?;
                self.init_value(&self.builder, tmp, rty)?;
                ptrs.push(tmp.into());
                self.call_void(mi.f, &ptrs)?;
                Ok(Some(self.load_val(tmp, rty, line)?))
            }
        }
    }

    /// Ordnet Aktualparameter den Formalparametern zu. Benannt (`p = a`) oder einzeln positional.
    /// Nicht belegte Parameter erhalten ihren Initialwert.
    pub(super) fn bind_method_args(
        &mut self,
        mi: &MethodInfo<'ctx>,
        args: &[NamedArg],
        method: &str,
        line: usize,
    ) -> CResult<Vec<BasicMetadataValueEnum<'ctx>>> {
        let n = mi.params.len();
        let mut slots: Vec<Option<(Expr, Option<bool>)>> = vec![None; n];

        let has_named = args.iter().any(|a| a.name.is_some());
        let has_positional = args.iter().any(|a| a.name.is_none());
        if has_named && has_positional {
            return err(line, format!("'{method}': positionale und benannte Parameter dürfen nicht gemischt werden"));
        }
        if has_positional {
            if args.len() > 1 || n != 1 || mi.params[0].1 {
                return err(line, format!("'{method}': Parameter müssen benannt werden (name = wert)"));
            }
            slots[0] = Some((args[0].expr.clone(), args[0].changing));
        }
        for a in args.iter().filter(|a| a.name.is_some()) {
            let pname = a.name.as_deref().unwrap_or("");
            let Some(i) = mi.pnames.iter().position(|x| x == pname) else {
                return err(line, format!("'{method}': Parameter '{pname}' unbekannt"));
            };
            if slots[i].is_some() {
                return err(line, format!("'{method}': Parameter '{pname}' doppelt angegeben"));
            }
            slots[i] = Some((a.expr.clone(), a.changing));
        }

        let mut ptrs = Vec::new();
        for (i, (ty, changing)) in mi.params.iter().enumerate() {
            match slots[i].clone() {
                Some((e, kw)) => {
                    if let Some(k) = kw {
                        if k != *changing {
                            return err(
                                line,
                                format!("'{method}': EXPORTING/CHANGING passt nicht zur Deklaration von '{}'", mi.pnames[i]),
                            );
                        }
                    }
                    ptrs.push(self.arg_ptr(&e, *ty, *changing, line)?.into());
                }
                None => {
                    // Nicht angegeben: Initialwert in einem Temporär
                    let tmp = llvm(self.builder.build_alloca(self.temp_type(*ty), "param_default"))?;
                    self.init_value(&self.builder, tmp, *ty)?;
                    ptrs.push(tmp.into());
                }
            }
        }
        Ok(ptrs)
    }

    /// Ein Aktualparameter als Zeiger: Variablen per Referenz, Ausdrücke in ein Temporär kopiert.
    pub(super) fn arg_ptr(&mut self, expr: &Expr, ty: Ty, changing: bool, line: usize) -> CResult<PointerValue<'ctx>> {
        match expr {
            Expr::Var(n) => {
                if n.contains("=>") && self.data(n, line).is_err() {
                    if changing {
                        return err(line, "externer statischer Wert ist kein CHANGING-Parameter");
                    }
                    let tmp = llvm(self.builder.build_alloca(self.temp_type(ty), "external_const"))?;
                    let value = self.eval(expr, line)?;
                    let destination = self.dest_of(ty, tmp, line)?;
                    self.store(destination, value, line)?;
                    return Ok(tmp);
                }
                let (p, t) = self.data(n, line)?;
                if t != ty {
                    return err(line, format!("Typ passt nicht zum Formalparameter: '{n}'"));
                }
                Ok(p)
            }
            other => {
                if changing {
                    return err(line, "CHANGING erwartet eine Variable");
                }
                let tmp = llvm(self.builder.build_alloca(self.temp_type(ty), "arg_tmp"))?;
                let v = self.eval(other, line)?;
                let dst = self.dest_of(ty, tmp, line)?;
                self.store(dst, v, line)?;
                Ok(tmp)
            }
        }
    }

    /// Aktualparameter als Zeiger: Variablen per Referenz, Ausdrücke in Temporäre kopiert.
    /// Aktualparameter einer FORM (positional, USING/CHANGING wie in der Deklaration).
    pub(super) fn arg_ptrs(
        &mut self,
        args: &[Arg],
        params: &[(Ty, bool)],
        name: &str,
        line: usize,
    ) -> CResult<Vec<BasicMetadataValueEnum<'ctx>>> {
        if args.len() != params.len() {
            return err(line, format!("'{name}': Anzahl der Parameter stimmt nicht"));
        }
        let mut ptrs: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
        for (arg, (ty, changing)) in args.iter().zip(params.iter()) {
            if arg.changing != *changing {
                return err(line, format!("'{name}': USING/CHANGING passt nicht zur Deklaration"));
            }
            ptrs.push(self.arg_ptr(&arg.expr, *ty, *changing, line)?.into());
        }
        Ok(ptrs)
    }
}
