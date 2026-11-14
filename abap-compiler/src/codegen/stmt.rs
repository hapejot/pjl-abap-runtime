//! Statements und Kontrollfluss.

use super::*;

impl<'ctx> Codegen<'ctx> {
    // --- Statements --------------------------------------------------------

    pub(super) fn emit_block(&mut self, stmts: &[Stmt]) -> CResult<()> {
        for s in stmts {
            self.emit_stmt(s)?;
        }
        Ok(())
    }

    pub(super) fn emit_stmt(&mut self, s: &Stmt) -> CResult<()> {
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
            Stmt::Return { .. } => {
                let Some(exit) = self.exit_fn else {
                    return err(0, "RETURN ohne Routine");
                };
                llvm(self.builder.build_unconditional_branch(exit))?;
                self.open_dead_block()
            }
            Stmt::Create { name, line } => {
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
                Ok(())
            }
            Stmt::Assign {
                target,
                value,
                line,
            } => {
                let dst = self.dest(target, *line)?;
                let v = self.eval(value, *line)?;
                self.store(dst, v, *line)
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
            Stmt::Write { newline, value, line } => {
                if *newline {
                    self.call_void(self.rt.newline, &[])?;
                }
                else {
                    self.call_void(self.rt.newfield, &[])?;
                }
                let Some(value) = value else {
                    return Ok(());
                };
                let v = self.eval(value, *line)?;
                match v {
                    Val::Ref(..) => {
                        return err(*line, "Objektreferenz kann nicht ausgegeben werden")
                    }
                    Val::Mem(..) => {
                        return err(
                            *line,
                            "Struktur kann nicht ausgegeben werden (nur Komponenten)",
                        )
                    }
                    Val::Int(i) => self.call_void(self.rt.write_int, &[i.into()])?,
                    Val::Chars(p, n) => {
                        self.call_void(self.rt.write_chars, &[p.into(), self.len(n).into()])?
                    }
                    Val::Str(h) => self.call_void(self.rt.write_str, &[h.into()])?,
                }
                Ok(())
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
                body,
                line,
            } => self.emit_loop(table, target, body, *line),
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
        }
    }

    /// Schreibt einen Wert in ein Ziel, mit ABAP-Konvertierung.
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
}
