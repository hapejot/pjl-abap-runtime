//! Statements und Kontrollfluss.

use super::*;

impl<'ctx> Codegen<'ctx> {
    // --- Statements --------------------------------------------------------

    pub(super) fn emit_block(&mut self, stmts: &[Stmt]) -> CResult<()> {
        // Ein fehlerhaftes Statement wird gesammelt; die übrigen Statements werden weiter erzeugt.
        for s in stmts {
            let result = self.emit_stmt(s);
            self.collect(result);
        }
        Ok(())
    }

    /// Anweisung mit dynamischem Feldsymbol: je Typ der Typwelt ein Zweig mit festgelegtem Typ.
    /// Zur Laufzeit wählt der Typ-Tag den Zweig; passt keiner, meldet das Programm einen Typkonflikt.
    pub(super) fn emit_stmt(&mut self, s: &Stmt) -> CResult<()> {
        // Deklarationen erzeugen keinen Code und brauchen keinen Typzweig
        if matches!(s, Stmt::FieldSymbol { .. } | Stmt::Data { .. }) {
            return self.emit_stmt_plain(s);
        }
        // ASSIGN setzt den Tag des Ziels selbst; nur die Quelle muss zur Laufzeit geprüft werden
        if let Stmt::AssignFs { fs, source, .. } = s {
            let source_open = self.dynamic_fs_names().contains(source);
            let target_open = self.var(fs, 0).is_ok_and(|v| v.ty == Ty::Dynamic);
            if source_open && !target_open {
                return self.emit_specialized(source, s);
            }
            return self.emit_stmt_plain(s);
        }
        let text = format!("{s:?}");
        let used: Vec<String> = self
            .dynamic_fs_names()
            .into_iter()
            .filter(|n| text.contains(&format!("\"{n}\"")))
            .collect();
        match used.as_slice() {
            [] => self.emit_stmt_plain(s),
            [name] => self.emit_specialized(name, s),
            _ => err(0, format!("mehrere dynamische Feldsymbole in einer Anweisung ({}) noch nicht unterstützt", used.join(", "))),
        }
    }

    /// Dynamische Feldsymbole im Gültigkeitsbereich, deren Typ in dieser Anweisung noch offen ist.
    fn dynamic_fs_names(&self) -> Vec<String> {
        let names: std::collections::BTreeSet<String> = self
            .vars
            .iter()
            .chain(self.globals.iter())
            .filter(|(n, v)| v.fs && v.ty == Ty::Dynamic && !self.fs_override.contains_key(*n))
            .map(|(n, _)| n.clone())
            .collect();
        names.into_iter().collect()
    }

    fn emit_specialized(&mut self, name: &str, s: &Stmt) -> CResult<()> {
        let tag_slot = self.var(name, 0)?.tag.ok_or_else(|| CompileError {
            line: 0,
            message: format!("Feldsymbol '{name}' hat keinen Typ-Tag"),
        })?;
        let tag = llvm(self.builder.build_load(self.ptrt, tag_slot, "spec_tag"))?.into_pointer_value();
        let end = self.new_block("fs_spec_end")?;
        for ty in self.dynamic_types.clone() {
            let want = self.type_descriptor(ty, 0)?;
            let t = llvm(self.builder.build_ptr_to_int(tag, self.i64t, "spec_t"))?;
            let w = llvm(self.builder.build_ptr_to_int(want, self.i64t, "spec_w"))?;
            let eq = llvm(self.builder.build_int_compare(IntPredicate::EQ, t, w, "spec_eq"))?;
            let hit = self.new_block("fs_spec_hit")?;
            let next = self.new_block("fs_spec_next")?;
            llvm(self.builder.build_conditional_branch(eq, hit, next))?;
            self.builder.position_at_end(hit);
            self.fs_override.insert(name.to_string(), ty);
            let result = self.emit_stmt_plain(s);
            self.fs_override.remove(name);
            if result.is_err() {
                // Block abschließen, damit der Fehler allein gemeldet wird
                self.br_if_open(end)?;
            }
            result?;
            self.br_if_open(end)?;
            self.builder.position_at_end(next);
        }
        self.call_void(self.rt.type_mismatch, &[])?;
        llvm(self.builder.build_unreachable())?;
        self.builder.position_at_end(end);
        Ok(())
    }

    fn emit_stmt_plain(&mut self, s: &Stmt) -> CResult<()> {
        match s {
            // Allokation erfolgt in `hoist`.
            Stmt::Data { .. } | Stmt::FieldSymbol { .. } => Ok(()),
            Stmt::Perform { name, args, line } => self.emit_perform(name, args, *line),
            Stmt::MethodCall {
                recv,
                method,
                args,
                receiving,
                line,
            } => self.emit_method_call(recv, method, args, receiving.as_ref(), *line),
            Stmt::ExternalFunctionCall { name, .. } => {
                let function = self.lit(name);
                self.call_void(
                    self.rt.external_call,
                    &[
                        function.into(),
                        self.len(name.encode_utf16().count()).into(),
                    ],
                )
            }
            Stmt::ExternalOperation { name, .. } => {
                let operation = self.lit(name);
                self.call_void(
                    self.rt.external_call,
                    &[
                        operation.into(),
                        self.len(name.encode_utf16().count()).into(),
                    ],
                )
            }
            Stmt::AssignFs { fs, source, line } => {
                let target = self.var(fs, *line)?.clone();
                if !target.fs {
                    return err(*line, format!("ASSIGN-Ziel '{fs}' ist kein Feldsymbol"));
                }
                let Some(tag_slot) = target.tag else {
                    return err(*line, format!("Feldsymbol '{fs}' hat keinen Typ-Tag"));
                };
                let (p, ty) = self.data(source, *line)?;
                if ty == Ty::Dynamic {
                    // Quelle ist ein dynamisches Feldsymbol: Zeiger und Tag werden übernommen
                    if target.ty != Ty::Dynamic {
                        return err(
                            *line,
                            format!("ASSIGN: dynamische Quelle '{source}' für typisiertes Feldsymbol '{fs}' braucht eine Laufzeitprüfung (noch nicht unterstützt)"),
                        );
                    }
                    let src = self.var(source, *line)?.clone();
                    let Some(src_tag) = src.tag else {
                        return err(*line, format!("Quelle '{source}' hat keinen Typ-Tag"));
                    };
                    let tag = llvm(self.builder.build_load(self.ptrt, src_tag, "src_tag"))?.into_pointer_value();
                    llvm(self.builder.build_store(target.slot, p))?;
                    llvm(self.builder.build_store(tag_slot, tag))?;
                    return Ok(());
                }
                if target.ty != Ty::Dynamic && target.ty != ty && self.fs_override.contains_key(source) {
                    // Spezialisierter Zweig mit anderem Typ: nur bei Erreichen zur Laufzeit ein Fehler
                    self.call_void(self.rt.type_mismatch, &[])?;
                    llvm(self.builder.build_unreachable())?;
                    let cont = self.new_block("fs_assign_cont")?;
                    self.builder.position_at_end(cont);
                    return Ok(());
                }
                if target.ty != Ty::Dynamic && target.ty != ty {
                    return err(
                        *line,
                        format!("ASSIGN: '{source}' passt nicht zu Feldsymbol '{fs}'"),
                    );
                }
                let descriptor = self.type_descriptor(ty, *line)?;
                llvm(self.builder.build_store(target.slot, p))?;
                llvm(self.builder.build_store(tag_slot, descriptor))?;
                Ok(())
            }
            Stmt::ExprStmt { expr, line } => {
                // Rückgabewert wird verworfen
                self.eval(expr, *line)?;
                Ok(())
            }
            Stmt::Raise { exception, line } => {
                let (pointer, ty) = self.data(exception, *line)?;
                let Ty::Ref(class_idx) = ty else {
                    return err(
                        *line,
                        format!("RAISE EXCEPTION erwartet eine Objektreferenz: '{exception}'"),
                    );
                };
                let object = llvm(
                    self.builder
                        .build_load(self.ptrt, pointer, "exception_object"),
                )?
                .into_pointer_value();
                self.call_void(
                    self.rt.exception_raise,
                    &[
                        object.into(),
                        self.i64t.const_int(class_idx as u64 + 1, false).into(),
                    ],
                )?;
                let Some(target) = self.exception_target.or(self.exit_fn) else {
                    return err(*line, "RAISE EXCEPTION ohne Rücksprungziel");
                };
                llvm(self.builder.build_unconditional_branch(target))?;
                self.open_dead_block()
            }
            Stmt::Try {
                body,
                catches,
                cleanup,
                line,
            } => self.emit_try(body, catches, cleanup, *line),
            Stmt::Return { .. } => {
                let Some(exit) = self.exit_fn else {
                    return err(0, "RETURN ohne Routine");
                };
                llvm(self.builder.build_unconditional_branch(exit))?;
                self.open_dead_block()
            }

            Stmt::Create { name, args, line } => {
                let (slot, ty) = self.data(name, *line)?;
                let Ty::Ref(c) = ty else {
                    return err(
                        *line,
                        format!("CREATE OBJECT erwartet eine Objektreferenz: '{name}'"),
                    );
                };
                let st = self.classes[c].struct_idx;
                let size = self.len(self.size_of(Ty::Struct(st)) as usize);
                let obj = self.call_ptr(self.rt.obj_new, &[size.into()])?;
                self.init_value(&self.builder, obj, Ty::Struct(st))?;
                llvm(self.builder.build_store(slot, obj))?;
                if self.has_method(c, "constructor") {
                    self.emit_method_call(name, "constructor", args, None, *line)?;
                } else if !args.is_empty() {
                    return err(
                        *line,
                        format!("CREATE OBJECT: Klasse hat keinen Konstruktor für '{name}'"),
                    );
                }
                Ok(())
            }
            Stmt::Assign {
                target,
                value,
                line,
            } => self.assign_stmt(target, value, *line),
            Stmt::AssignComponent {
                component,
                structure,
                target,
                line,
            } => {
                let (base, ty) = self.data(structure, *line)?;
                if !matches!(ty, Ty::Struct(_)) {
                    let target_symbol = self.var(target, *line)?.clone();
                    if !target_symbol.fs {
                        return err(*line, format!("ASSIGN-Ziel '{target}' ist kein Feldsymbol"));
                    }
                    llvm(
                        self.builder
                            .build_store(target_symbol.slot, self.ptrt.const_null()),
                    )?;
                    return Ok(());
                }
                let Ty::Struct(index) = ty else {
                    return err(
                        *line,
                        format!("'{structure}' ist keine statisch bekannte Struktur"),
                    );
                };
                let layout = self.layouts[index].clone();
                let Some(field_index) =
                    layout.fields.iter().position(|(name, _)| name == component)
                else {
                    return err(
                        *line,
                        format!("Komponente '{component}' unbekannt in '{structure}'"),
                    );
                };
                let target_symbol = self.var(target, *line)?.clone();
                if !target_symbol.fs {
                    return err(*line, format!("ASSIGN-Ziel '{target}' ist kein Feldsymbol"));
                }
                let field_ty = layout.fields[field_index].1;
                if target_symbol.ty != field_ty {
                    return err(
                        *line,
                        format!(
                            "Komponente '{component}' hat einen anderen Typ als Feldsymbol '{target}'"
                        ),
                    );
                }
                let field = llvm(self.builder.build_struct_gep(
                    layout.st,
                    base,
                    field_index as u32,
                    "assigned_component",
                ))?;
                llvm(self.builder.build_store(target_symbol.slot, field))?;
                Ok(())
            }
            Stmt::ReadTable {
                table,
                target,
                index,
                line,
            } => {
                let (handle, element) = self.table(table, *line)?;
                let target_symbol = self.var(target, *line)?.clone();
                if !target_symbol.fs || target_symbol.ty != element.ty() {
                    return err(
                        *line,
                        format!("READ TABLE-Ziel '{target}' hat nicht den Zeilentyp"),
                    );
                }
                let index = self.int_operand(index, *line)?;
                let zero_based = llvm(self.builder.build_int_sub(
                    index,
                    self.i32t.const_int(1, false),
                    "read_index",
                ))?;
                let zero_based = llvm(self.builder.build_int_z_extend(
                    zero_based,
                    self.i64t,
                    "read_index_i64",
                ))?;
                let row = self.call_ptr(self.rt.table_row, &[handle.into(), zero_based.into()])?;
                llvm(self.builder.build_store(target_symbol.slot, row))?;
                Ok(())
            }
            Stmt::Clear { target, line } => {
                if let Target::Var(name) = target {
                    if let Ok((h, _)) = self.table(name, *line) {
                        return self.call_void(self.rt.table_clear, &[h.into()]);
                    }
                }
                let dst = self.dest(target, *line)?;
                // CLEAR: Zahl 0, Zeichen Leerzeichen, String leer
                match dst {
                    Dest::Ref(p, _) => {
                        llvm(self.builder.build_store(p, self.ptrt.const_null()))?;
                    }
                    Dest::Mem(p, i) => self.init_value(&self.builder, p, Ty::Struct(i))?,
                    Dest::Int(p) => {
                        llvm(self.builder.build_store(p, self.i32t.const_zero()))?;
                    }
                    Dest::Chars(p, n) => {
                        self.call_void(
                            self.rt.chars_move,
                            &[
                                p.into(),
                                self.len(n).into(),
                                self.ptrt.const_null().into(),
                                self.len(0).into(),
                            ],
                        )?;
                    }
                    Dest::Str(p) => {
                        let empty = self.empty_string_now()?;
                        llvm(self.builder.build_store(p, empty))?;
                    }
                }
                Ok(())
            }
            Stmt::Write {
                newline,
                value,
                line,
            } => {
                if *newline {
                    self.call_void(self.rt.newline, &[])?;
                } else {
                    self.call_void(self.rt.newfield, &[])?;
                }
                let Some(value) = value else {
                    return Ok(());
                };
                let v = self.eval(value, *line)?;
                self.write_value(v, *line)
            }
            Stmt::If {
                branches,
                otherwise,
                line,
            } => self.emit_if(branches, otherwise, *line),
            Stmt::Case {
                subject,
                whens,
                other,
                line,
            } => self.emit_case(subject, whens, other, *line),
            Stmt::Do { times, body, line } => self.emit_do(times.as_ref(), body, *line),
            Stmt::While { cond, body, line } => self.emit_while(cond, body, *line),
            Stmt::Exit { line } => {
                let Some(lp) = self.loops.last().copied() else {
                    return err(*line, "EXIT außerhalb einer Schleife");
                };
                llvm(self.builder.build_unconditional_branch(lp.exit))?;
                self.open_dead_block()
            }
            Stmt::Continue { line } => {
                let Some(lp) = self.loops.last().copied() else {
                    return err(*line, "CONTINUE außerhalb einer Schleife");
                };
                llvm(self.builder.build_unconditional_branch(lp.next))?;
                self.open_dead_block()
            }
            Stmt::Check { cond, line } => {
                let c = self.cond(cond, *line)?;
                // Nicht erfüllt: nächster Durchlauf (Schleife) bzw. Programmende (außerhalb)
                let target = match self.loops.last().copied() {
                    Some(lp) => lp.next,
                    None => self.exit_fn.expect("exit-Block fehlt"),
                };
                let cont = self.new_block("check_ok")?;
                llvm(self.builder.build_conditional_branch(c, cont, target))?;
                self.builder.position_at_end(cont);
                Ok(())
            }
            Stmt::Assert { cond, line } => {
                let condition = self.cond(cond, *line)?;
                let success = self.new_block("assert_ok")?;
                let failure = self.new_block("assert_failed")?;
                llvm(
                    self.builder
                        .build_conditional_branch(condition, success, failure),
                )?;
                self.builder.position_at_end(failure);
                self.call_void(self.rt.unbound, &[])?;
                llvm(self.builder.build_unreachable())?;
                self.builder.position_at_end(success);
                Ok(())
            }
            Stmt::Append { table, value, line } => {
                let (h, e) = self.table(table, *line)?;
                // Wert in einen temporären Speicherplatz bringen, dann die Bytes der Zeile anhängen
                let (tmp, tmp_dst) = self.temp(e)?;
                let v = self.eval(value, *line)?;
                self.store(tmp_dst, v, *line)?;
                self.call_void(
                    self.rt.table_append,
                    &[
                        h.into(),
                        tmp.into(),
                        self.len(self.elem_size(e) as usize).into(),
                    ],
                )
            }
            Stmt::Loop {
                table,
                target,
                filter,
                body,
                line,
            } => self.emit_loop(table, target, filter.as_ref(), body, *line),
            Stmt::Describe {
                table,
                target,
                line,
            } => {
                let (h, _) = self.table(table, *line)?;
                let n = self.call_int(self.rt.table_count, &[h.into()])?;
                let n32 = llvm(self.builder.build_int_truncate(n, self.i32t, "lines"))?;
                let (p, ty) = self.data(target, *line)?;
                if ty != Ty::Int {
                    return err(
                        *line,
                        format!("DESCRIBE LINES erwartet ein ganzzahliges Feld: '{target}'"),
                    );
                }
                llvm(self.builder.build_store(p, n32))?;
                Ok(())
            }
            Stmt::NotSupported { tokens } => {
                let keyword = tokens.first().map(|t| t.text.to_uppercase()).unwrap_or_default();
                let line = tokens.first().map_or(0, |t| t.line);
                err(line, format!("Nicht unterstütztes Statement: {keyword}"))
            }
        }
    }

    /// Schreibt einen Wert in ein Ziel, mit ABAP-Konvertierung.
    /// Typdeskriptor zur Laufzeit: {Art, Länge}. Art: 1 Ganzzahl, 2 Zeichen, 3 String, 4 Struktur.
    pub(super) fn type_descriptor(&self, ty: Ty, line: usize) -> CResult<PointerValue<'ctx>> {
        let (kind, len): (u64, u64) = match ty {
            Ty::Int => (1, 4),
            Ty::Char(n) => (2, n as u64),
            Ty::Str => (3, 0),
            Ty::Struct(_) => (4, self.size_of(ty)),
            _ => return err(line, "ASSIGN auf diesen Typ wird noch nicht unterstützt"),
        };
        let name = format!("abap_td_{kind}_{len}");
        if let Some(global) = self.module.get_global(&name) {
            return Ok(global.as_pointer_value());
        }
        let descriptor_ty = self.i32t.array_type(2);
        let global = self.module.add_global(descriptor_ty, None, &name);
        global.set_constant(true);
        global.set_initializer(&self.i32t.const_array(&[
            self.i32t.const_int(kind, false),
            self.i32t.const_int(len, false),
        ]));
        Ok(global.as_pointer_value())
    }

    /// Gibt einen Wert aus; Objekte, Strukturen und Tabellen sind nicht ausgabefähig.
    pub(super) fn write_value(&mut self, v: Val<'ctx>, line: usize) -> CResult<()> {
        match v {
                    Val::Ref(..) => {
                        return err(line, "Objektreferenz kann nicht ausgegeben werden")
                    }
                    Val::Mem(..) => {
                        return err(
                            line,
                            "Struktur kann nicht ausgegeben werden (nur Komponenten)",
                        )
                    }
                    Val::Table(_) => {
                        return err(line, "Interne Tabelle kann nicht ausgegeben werden")
                    }
                    Val::Int(i) => self.call_void(self.rt.write_int, &[i.into()])?,
                    Val::Chars(p, n) => {
                        self.call_void(self.rt.write_chars, &[p.into(), self.len(n).into()])?
                    }
                    Val::Str(h) => self.call_void(self.rt.write_str, &[h.into()])?,
        }
        Ok(())
    }

    /// Zuweisung. Dynamische Feldsymbole werden zur Laufzeit gegen ihren Typ-Tag geprüft.
    pub(super) fn assign_stmt(&mut self, target: &Target, value: &Expr, line: usize) -> CResult<()> {
        let dst = self.dest(target, line)?;
        let v = self.eval(value, line)?;
        self.store(dst, v, line)
    }

    pub(super) fn store(&mut self, dst: Dest<'ctx>, v: Val<'ctx>, line: usize) -> CResult<()> {
        match dst {
            Dest::Ref(slot, c) => match v {
                Val::Ref(obj, c2) if c == c2 => {
                    llvm(self.builder.build_store(slot, obj))?;
                    Ok(())
                }
                _ => err(line, "Objektreferenz passt nicht zum Zieltyp"),
            },
            Dest::Mem(p, i) => match v {
                // Struktur-Zuweisung kopiert alle Komponenten (gleicher Typ)
                Val::Mem(src, j) if i == j => {
                    let val = llvm(self.builder.build_load(
                        self.llvm_ty(Ty::Struct(i)),
                        src,
                        "cp",
                    ))?;
                    llvm(self.builder.build_store(p, val))?;
                    Ok(())
                }
                Val::Mem(..) => err(
                    line,
                    "Struktur kann nur einer Struktur desselben Typs zugewiesen werden",
                ),
                Val::Str(_) | Val::Chars(_, _) => Ok(()),
                _ => err(line, "Wert kann nicht einer Struktur zugewiesen werden"),
            },
            Dest::Int(p) => match v {
                Val::Int(i) => {
                    llvm(self.builder.build_store(p, i))?;
                    Ok(())
                }
                Val::Mem(..) => err(
                    line,
                    "Struktur kann nicht einem ganzzahligen Feld zugewiesen werden",
                ),
                _ => err(
                    line,
                    "Text kann nicht einem ganzzahligen Feld zugewiesen werden",
                ),
            },
            Dest::Chars(p, n) => match v {
                Val::Ref(..) => err(
                    line,
                    "Objektreferenz kann keinem Zeichenfeld zugewiesen werden",
                ),
                Val::Table(_) => err(line, "Tabelle kann keinem Zeichenfeld zugewiesen werden"),
                // Zuweisung kürzt bzw. füllt mit Leerzeichen auf
                Val::Chars(sp, sl) => self.call_void(
                    self.rt.chars_move,
                    &[p.into(), self.len(n).into(), sp.into(), self.len(sl).into()],
                ),
                Val::Str(h) => self.call_void(
                    self.rt.str_to_chars,
                    &[p.into(), self.len(n).into(), h.into()],
                ),
                Val::Int(_) => err(
                    line,
                    "Zahl kann nicht einem Zeichenfeld zugewiesen werden (noch nicht unterstützt)",
                ),
                Val::Mem(..) => err(
                    line,
                    "Struktur kann nicht einem Zeichenfeld zugewiesen werden",
                ),
            },
            Dest::Str(slot) => {
                let h = match v {
                    Val::Str(h) => h,
                    // Zeichenfeld -> String: nachgestellte Leerzeichen entfallen
                    Val::Chars(sp, sl) => {
                        self.call_ptr(self.rt.str_from_chars, &[sp.into(), self.len(sl).into()])?
                    }
                    Val::Int(_) => return err(
                        line,
                        "Zahl kann nicht einem String zugewiesen werden (noch nicht unterstützt)",
                    ),
                    Val::Mem(..) => {
                        return err(line, "Struktur kann nicht einem String zugewiesen werden")
                    }
                    Val::Ref(..) => {
                        return err(
                            line,
                            "Objektreferenz kann nicht einem String zugewiesen werden",
                        )
                    }
                    Val::Table(_) => {
                        return err(line, "Tabelle kann nicht einem String zugewiesen werden")
                    }
                };
                llvm(self.builder.build_store(slot, h))?;
                Ok(())
            }
        }
    }

    pub(super) fn emit_if(
        &mut self,
        branches: &[(Cond, Vec<Stmt>)],
        otherwise: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        let merge = self.new_block("endif")?;
        for (cond, body) in branches {
            let c = self.cond(cond, line)?;
            let then = self.new_block("then")?;
            let next = self.new_block("elif")?;
            llvm(self.builder.build_conditional_branch(c, then, next))?;
            self.builder.position_at_end(then);
            self.emit_block(body)?;
            self.br_if_open(merge)?;
            self.builder.position_at_end(next);
        }
        self.emit_block(otherwise)?;
        self.br_if_open(merge)?;
        self.builder.position_at_end(merge);
        Ok(())
    }

    pub(super) fn emit_case(
        &mut self,
        subject: &Expr,
        whens: &[(Vec<Expr>, Vec<Stmt>)],
        other: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        let subj = self.eval(subject, line)?;
        let merge = self.new_block("endcase")?;
        for (values, body) in whens {
            // Bedingung: subject = v1 OR subject = v2 ...
            let mut any: Option<IntValue<'ctx>> = None;
            for v in values {
                let rhs = self.eval(v, line)?;
                let eq = self.compare(CmpOp::Eq, subj, rhs, line)?;
                any = Some(match any {
                    None => eq,
                    Some(prev) => llvm(self.builder.build_or(prev, eq, "when_or"))?,
                });
            }
            let c = any.expect("WHEN ohne Werte wird im Lowering abgefangen");
            let then = self.new_block("when")?;
            let next = self.new_block("when_next")?;
            llvm(self.builder.build_conditional_branch(c, then, next))?;
            self.builder.position_at_end(then);
            self.emit_block(body)?;
            self.br_if_open(merge)?;
            self.builder.position_at_end(next);
        }
        self.emit_block(other)?;
        self.br_if_open(merge)?;
        self.builder.position_at_end(merge);
        Ok(())
    }

    pub(super) fn emit_do(
        &mut self,
        times: Option<&Expr>,
        body: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        // Anzahl wird einmal ausgewertet und in einem versteckten Zähler gehalten.
        let counter_limit = match times {
            Some(e) => {
                let n = match self.eval(e, line)? {
                    Val::Int(i) => i,
                    _ => return err(line, "Anzahl bei DO muss ganzzahlig sein"),
                };
                let limit = llvm(self.alloca.build_alloca(self.i32t, "do_limit"))?;
                let cnt = llvm(self.alloca.build_alloca(self.i32t, "do_counter"))?;
                // Zähler bei jedem Eintritt zurücksetzen (wichtig bei Verschachtelung)
                llvm(self.builder.build_store(cnt, self.i32t.const_zero()))?;
                llvm(self.builder.build_store(limit, n))?;
                Some((cnt, limit))
            }
            None => None,
        };

        let head = self.new_block("do_head")?;
        let bodyb = self.new_block("do_body")?;
        let latch = self.new_block("do_latch")?;
        let exit = self.new_block("do_exit")?;
        self.br_if_open(head)?;

        self.builder.position_at_end(head);
        match counter_limit {
            Some((cnt, limit)) => {
                let cur = llvm(self.builder.build_load(self.i32t, cnt, "cnt"))?.into_int_value();
                let lim = llvm(self.builder.build_load(self.i32t, limit, "lim"))?.into_int_value();
                let more =
                    llvm(
                        self.builder
                            .build_int_compare(IntPredicate::SLT, cur, lim, "more"),
                    )?;
                llvm(self.builder.build_conditional_branch(more, bodyb, exit))?;
            }
            None => {
                llvm(self.builder.build_unconditional_branch(bodyb))?;
            }
        }

        self.builder.position_at_end(bodyb);
        self.loops.push(LoopCtx { exit, next: latch });
        self.emit_block(body)?;
        self.loops.pop();
        self.br_if_open(latch)?;

        self.builder.position_at_end(latch);
        if let Some((cnt, _)) = counter_limit {
            let cur = llvm(self.builder.build_load(self.i32t, cnt, "cnt"))?.into_int_value();
            let inc = llvm(
                self.builder
                    .build_int_add(cur, self.i32t.const_int(1, false), "inc"),
            )?;
            llvm(self.builder.build_store(cnt, inc))?;
        }
        llvm(self.builder.build_unconditional_branch(head))?;

        self.builder.position_at_end(exit);
        Ok(())
    }

    pub(super) fn emit_while(&mut self, cond: &Cond, body: &[Stmt], line: usize) -> CResult<()> {
        let head = self.new_block("while_head")?;
        let bodyb = self.new_block("while_body")?;
        let exit = self.new_block("while_exit")?;
        self.br_if_open(head)?;

        self.builder.position_at_end(head);
        let c = self.cond(cond, line)?;
        llvm(self.builder.build_conditional_branch(c, bodyb, exit))?;

        self.builder.position_at_end(bodyb);
        self.loops.push(LoopCtx { exit, next: head });
        self.emit_block(body)?;
        self.loops.pop();
        self.br_if_open(head)?;

        self.builder.position_at_end(exit);
        Ok(())
    }

    /// LOOP AT tabelle INTO wa | ASSIGNING <fs>: Index läuft von 0 bis zur aktuellen Zeilenanzahl.
    pub(super) fn emit_loop(
        &mut self,
        table: &str,
        target: &LoopTarget,
        filter: Option<&LoopFilter>,
        body: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        let (h, e) = self.table(table, line)?;
        let idx = llvm(self.alloca.build_alloca(self.i64t, "loop_idx"))?;
        llvm(self.builder.build_store(idx, self.i64t.const_zero()))?;
        // Anzahl der Zeilen wird beim Start festgelegt; in der Schleife angehängte Zeilen werden nicht durchlaufen
        let limit = llvm(self.alloca.build_alloca(self.i64t, "loop_limit"))?;
        let n0 = self.call_int(self.rt.table_count, &[h.into()])?;
        llvm(self.builder.build_store(limit, n0))?;

        let head = self.new_block("loop_head")?;
        let bodyb = self.new_block("loop_body")?;
        let latch = self.new_block("loop_latch")?;
        let exit = self.new_block("loop_exit")?;
        self.br_if_open(head)?;

        self.builder.position_at_end(head);
        let cur = llvm(self.builder.build_load(self.i64t, idx, "li"))?.into_int_value();
        let cnt = llvm(self.builder.build_load(self.i64t, limit, "lim"))?.into_int_value();
        let more = llvm(
            self.builder
                .build_int_compare(IntPredicate::SLT, cur, cnt, "more"),
        )?;
        llvm(self.builder.build_conditional_branch(more, bodyb, exit))?;

        self.builder.position_at_end(bodyb);
        let cur_b = llvm(self.builder.build_load(self.i64t, idx, "lb"))?.into_int_value();
        let row = self.call_ptr(self.rt.table_row, &[h.into(), cur_b.into()])?;
        match target {
            LoopTarget::Into(wa) => {
                let (wp, wty) = self.data(wa, line)?;
                if wty != e.ty() {
                    return err(
                        line,
                        format!("Arbeitsbereich passt nicht zum Tabellentyp: '{wa}'"),
                    );
                }
                let v = self.read_row(row, e)?;
                let dst = self.dest_of(wty, wp, line)?;
                self.store(dst, v, line)?;
            }
            LoopTarget::Assigning(fs) => {
                let v = self.var(fs, line)?;
                if !v.fs {
                    return err(line, format!("ASSIGNING erwartet ein Feldsymbol: '{fs}'"));
                }
                if v.ty != e.ty() {
                    return err(
                        line,
                        format!("Feldsymbol passt nicht zum Tabellentyp: '{fs}'"),
                    );
                }
                llvm(self.builder.build_store(v.slot, row))?;
            }
        }
        if let Some(filter) = filter {
            let Elem::Struct(structure) = e else {
                return err(line, "LOOP WHERE benötigt strukturierte Tabellenzeilen");
            };
            let layout = self.layouts[structure].clone();
            let Some(field_index) = layout
                .fields
                .iter()
                .position(|(name, _)| name == &filter.component)
            else {
                return err(
                    line,
                    format!("LOOP WHERE: Komponente '{}' unbekannt", filter.component),
                );
            };
            let field = llvm(self.builder.build_struct_gep(
                layout.st,
                row,
                field_index as u32,
                "loop_filter_field",
            ))?;
            let actual = self.load_val(field, layout.fields[field_index].1, line)?;
            let expected = self.eval(&filter.value, line)?;
            let matches = self.compare(CmpOp::Eq, actual, expected, line)?;
            let filtered_body = self.new_block("loop_filter_body")?;
            llvm(
                self.builder
                    .build_conditional_branch(matches, filtered_body, latch),
            )?;
            self.builder.position_at_end(filtered_body);
        }
        self.loops.push(LoopCtx { exit, next: latch });
        self.emit_block(body)?;
        self.loops.pop();
        self.br_if_open(latch)?;

        self.builder.position_at_end(latch);
        let cur_l = llvm(self.builder.build_load(self.i64t, idx, "ll"))?.into_int_value();
        let inc = llvm(
            self.builder
                .build_int_add(cur_l, self.i64t.const_int(1, false), "linc"),
        )?;
        llvm(self.builder.build_store(idx, inc))?;
        llvm(self.builder.build_unconditional_branch(head))?;

        self.builder.position_at_end(exit);
        Ok(())
    }

    pub(super) fn emit_try(
        &mut self,
        body: &[Stmt],
        catches: &[CatchClause],
        cleanup: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        let dispatch = self.new_block("exception_dispatch")?;
        let continuation = self.new_block("try_done")?;
        let outer_target = self.exception_target;
        self.exception_target = Some(dispatch);

        for statement in body {
            self.emit_stmt(statement)?;
            if self.builder.get_insert_block().is_none() {
                break;
            }
            let next = self.new_block("try_next")?;
            let pending = self.call_int(self.rt.exception_pending, &[])?;
            let condition = llvm(self.builder.build_int_compare(
                IntPredicate::NE,
                pending,
                self.i32t.const_zero(),
                "exception_pending",
            ))?;
            llvm(
                self.builder
                    .build_conditional_branch(condition, dispatch, next),
            )?;
            self.builder.position_at_end(next);
        }
        self.br_if_open(continuation)?;
        self.builder.position_at_end(dispatch);

        let pending_class = self.call_int(self.rt.exception_class, &[])?;
        let mut next_test = dispatch;
        for (catch_index, catch) in catches.iter().enumerate() {
            let match_block = self.new_block("catch_match")?;
            let fallback = self.new_block("catch_test_next")?;
            self.builder.position_at_end(next_test);
            let ids = self.catch_class_ids(&catch.class);
            if ids.is_empty() {
                llvm(self.builder.build_unconditional_branch(fallback))?;
            } else {
                let mut matches = self.i32t.const_zero();
                for id in ids {
                    let equal = llvm(self.builder.build_int_compare(
                        IntPredicate::EQ,
                        pending_class,
                        self.i64t.const_int(id, false),
                        "catch_class",
                    ))?;
                    let extended = llvm(self.builder.build_int_z_extend(
                        equal,
                        self.i32t,
                        "catch_bit",
                    ))?;
                    matches = llvm(self.builder.build_or(matches, extended, "catch_any"))?;
                }
                let condition = llvm(self.builder.build_int_compare(
                    IntPredicate::NE,
                    matches,
                    self.i32t.const_zero(),
                    "catch_condition",
                ))?;
                llvm(
                    self.builder
                        .build_conditional_branch(condition, match_block, fallback),
                )?;
            }

            self.builder.position_at_end(match_block);
            if let Some(target) = &catch.target {
                let (slot, ty) = self.data(target, catch.line)?;
                let Ty::Ref(_) = ty else {
                    return err(
                        catch.line,
                        format!("CATCH INTO erwartet Objektreferenz: '{target}'"),
                    );
                };
                let object = self.call_ptr(self.rt.exception_object, &[])?;
                llvm(self.builder.build_store(slot, object))?;
            }
            self.call_void(self.rt.exception_clear, &[])?;
            self.emit_block(&catch.body)?;
            self.br_if_open(continuation)?;
            next_test = fallback;
            if catch_index + 1 == catches.len() {
                self.builder.position_at_end(fallback);
            }
        }

        if catches.is_empty() {
            self.builder.position_at_end(dispatch);
        } else {
            self.builder.position_at_end(next_test);
        }
        self.exception_target = outer_target;
        self.emit_block(cleanup)?;
        let target = outer_target.or(self.exit_fn).ok_or_else(|| CompileError {
            line,
            message: "Unbehandelte Exception ohne Rücksprungziel".into(),
        })?;
        self.br_if_open(target)?;
        self.builder.position_at_end(continuation);
        Ok(())
    }

    fn catch_class_ids(&self, name: &str) -> Vec<u64> {
        self.classes
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                let mut current = Some(*index);
                while let Some(class_index) = current {
                    let candidate = &self.classes[class_index];
                    if candidate.name == name || candidate.external_parent.as_deref() == Some(name)
                    {
                        return true;
                    }
                    current = candidate.parent;
                }
                false
            })
            .map(|(index, _)| index as u64 + 1)
            .collect()
    }
}

