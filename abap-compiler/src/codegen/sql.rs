//! SQL-Anweisungen (Phasen 1 und 2): `SELECT` als Einzelsatz und als Cursor (`SELECT ... ENDSELECT`),
//! Schreibanweisungen (`abap_rt_sql_execute`), `COMMIT WORK` und `ROLLBACK WORK`, das Programmende mit `abap_rt_sql_finish`.
//! Die Laufzeit (`abap_rt_sql_*`) bekommt die IR als JSON-Konstante und die Hostvariablen als Zeigerfeld.
//! Nach jedem Aufruf stehen SY-SUBRC und SY-DBCNT. Spezifikation: `doc/sql-spezifikation.md`, Abschnitte 6 und 10.

use super::*;

/// Argumente, die jeder SQL-Aufruf braucht: IR-Text, Länge, Hostvariablen und deren Anzahl.
struct SqlArgs<'ctx> {
    text: PointerValue<'ctx>,
    len: IntValue<'ctx>,
    params: PointerValue<'ctx>,
    count: IntValue<'ctx>,
}

impl<'ctx> Codegen<'ctx> {
    /// `SELECT` ohne Cursor: Aufruf der Laufzeit, danach SY-SUBRC und SY-DBCNT.
    pub(super) fn emit_sql_select(
        &mut self,
        id: u32,
        params: &[String],
        target: &str,
        line: usize,
    ) -> CResult<()> {
        let args = self.sql_args(id, params, line)?;
        let target_ptr = self.sql_target(target, line)?;
        let dbcnt = llvm(self.alloca.build_alloca(self.i64t, "sql_dbcnt"))?;
        llvm(self.builder.build_store(dbcnt, self.i64t.const_zero()))?;
        let rc = self.call_int(
            self.rt.sql_select,
            &[
                self.i32t.const_int(id as u64, false).into(),
                args.text.into(),
                args.len.into(),
                args.params.into(),
                args.count.into(),
                target_ptr.into(),
                dbcnt.into(),
            ],
        )?;
        self.set_sy_subrc(rc)?;
        self.set_sy_dbcnt(dbcnt)
    }

    /// Schreibende Anweisung (`INSERT`, `UPDATE`, `DELETE`, `MODIFY`): Aufruf der Laufzeit, danach SY-SUBRC und
    /// SY-DBCNT. Ohne Zeilenquelle (`target` leer) ist das Zielbereich-Argument `null`.
    pub(super) fn emit_sql_write(
        &mut self,
        id: u32,
        params: &[String],
        target: &str,
        line: usize,
    ) -> CResult<()> {
        let args = self.sql_args(id, params, line)?;
        let target_ptr = if target.is_empty() {
            self.ptrt.const_null()
        } else {
            self.sql_target(target, line)?
        };
        let dbcnt = llvm(self.alloca.build_alloca(self.i64t, "sql_dbcnt"))?;
        llvm(self.builder.build_store(dbcnt, self.i64t.const_zero()))?;
        let rc = self.call_int(
            self.rt.sql_execute,
            &[
                self.i32t.const_int(id as u64, false).into(),
                args.text.into(),
                args.len.into(),
                args.params.into(),
                args.count.into(),
                target_ptr.into(),
                dbcnt.into(),
            ],
        )?;
        self.set_sy_subrc(rc)?;
        self.set_sy_dbcnt(dbcnt)
    }

    /// `COMMIT WORK` (`rollback` false) bzw. `ROLLBACK WORK`: Aufruf der Laufzeit. SY-SUBRC bleibt unverändert,
    /// weil der Standard für diese Anweisungen keinen Wert vorsieht (Entscheidung, siehe `zu-pruefen.txt`).
    /// Fehler beendet die Laufzeit selbst.
    pub(super) fn emit_sql_commit(&mut self, rollback: bool, _line: usize) -> CResult<()> {
        let function = if rollback {
            self.rt.sql_rollback
        } else {
            self.rt.sql_commit
        };
        self.call_int(function, &[])?;
        Ok(())
    }

    /// `SELECT ... ENDSELECT`: Cursor öffnen; je Zeile läuft der Körper. Verlassen mit EXIT oder am Ende
    /// schließt den Cursor im Block `close`, dort wird SY-SUBRC gesetzt (0, wenn eine Zeile gelesen wurde).
    pub(super) fn emit_sql_loop(
        &mut self,
        id: u32,
        params: &[String],
        target: &str,
        body: &[Stmt],
        line: usize,
    ) -> CResult<()> {
        let args = self.sql_args(id, params, line)?;
        let target_ptr = self.sql_target(target, line)?;
        let cursor = llvm(self.alloca.build_alloca(self.ptrt, "sql_cursor"))?;
        let dbcnt = llvm(self.alloca.build_alloca(self.i64t, "sql_dbcnt"))?;
        llvm(self.builder.build_store(dbcnt, self.i64t.const_zero()))?;
        let handle = self.call_ptr(
            self.rt.sql_cursor_open,
            &[
                self.i32t.const_int(id as u64, false).into(),
                args.text.into(),
                args.len.into(),
                args.params.into(),
                args.count.into(),
            ],
        )?;
        llvm(self.builder.build_store(cursor, handle))?;
        // SY-DBCNT beginnt bei 0; die Schleife setzt es je Zeile und am Ende aus ihrem Zähler
        self.set_sy_dbcnt(dbcnt)?;

        let head = self.new_block("sql_head")?;
        let bodyb = self.new_block("sql_body")?;
        let close = self.new_block("sql_close")?;
        self.br_if_open(head)?;

        // Kopf: nächste Zeile holen. Zwischenwerte des vorigen Durchlaufs freigeben (wie WHILE).
        self.builder.position_at_end(head);
        if let Some(mark) = self.innermost_mark() {
            self.release_to(mark)?;
        }
        let current =
            llvm(self.builder.build_load(self.ptrt, cursor, "sql_handle"))?.into_pointer_value();
        let got = self.call_int(
            self.rt.sql_cursor_fetch,
            &[current.into(), target_ptr.into(), dbcnt.into()],
        )?;
        let has_row = llvm(self.builder.build_int_compare(
            IntPredicate::NE,
            got,
            self.i32t.const_zero(),
            "sql_has_row",
        ))?;
        llvm(self.builder.build_conditional_branch(has_row, bodyb, close))?;

        // Körper: SY-DBCNT zählt die bisher gelesenen Zeilen einschließlich der aktuellen
        self.builder.position_at_end(bodyb);
        self.set_sy_dbcnt(dbcnt)?;
        self.loops.push(LoopCtx {
            exit: close,
            next: head,
            cursor: Some(cursor),
        });
        let result = self.emit_block(body);
        self.loops.pop();
        result?;
        self.br_if_open(head)?;

        // Schließen: EXIT und das Ende der Daten landen hier
        self.builder.position_at_end(close);
        let handle = llvm(
            self.builder
                .build_load(self.ptrt, cursor, "sql_close_handle"),
        )?
        .into_pointer_value();
        let rc = self.call_int(self.rt.sql_cursor_close, &[handle.into()])?;
        self.set_sy_subrc(rc)?;
        // Nach dem Ende steht SY-DBCNT auf der Zahl der gelesenen Zeilen, auch wenn der Körper
        // weitere Datenbankanweisungen ausgeführt hat
        self.set_sy_dbcnt(dbcnt)
    }

    /// Schließt die Cursor der Schleifen ab Tiefe `depth`, innerste zuerst (für RETURN und Ausnahmen).
    /// Ohne offenen Block geschieht nichts.
    pub(super) fn close_cursors(&mut self, depth: usize) -> CResult<()> {
        if !self.current_open() {
            return Ok(());
        }
        let cursors: Vec<PointerValue<'ctx>> = self
            .loops
            .iter()
            .skip(depth)
            .rev()
            .filter_map(|lp| lp.cursor)
            .collect();
        for cursor in cursors {
            let handle = llvm(
                self.builder
                    .build_load(self.ptrt, cursor, "sql_leave_handle"),
            )?
            .into_pointer_value();
            self.call_void(self.rt.sql_cursor_close, &[handle.into()])?;
        }
        Ok(())
    }

    /// IR-Konstante, Länge und Zeigerfeld der Hostvariablen. Ohne Hostvariablen ist das Feld `null`.
    fn sql_args(&self, id: u32, params: &[String], line: usize) -> CResult<SqlArgs<'ctx>> {
        let Some(text) = self.sql_texts.get(id as usize) else {
            return err(line, format!("internal error: SQL request {id} is missing"));
        };
        let bytes = text.as_bytes();
        let constant = self.ctx.const_string(bytes, false);
        let global = self.module.add_global(constant.get_type(), None, "sql_ir");
        global.set_initializer(&constant);
        global.set_constant(true);
        let json = global.as_pointer_value();
        let len = self.i64t.const_int(bytes.len() as u64, false);
        if params.is_empty() {
            return Ok(SqlArgs {
                text: json,
                len,
                params: self.ptrt.const_null(),
                count: self.i64t.const_zero(),
            });
        }
        // Zeigerfeld als Struktur aus Zeigern: gepackt, Elemente liegen hintereinander
        let fields: Vec<inkwell::types::BasicTypeEnum<'ctx>> = vec![self.ptrt.into(); params.len()];
        let array_ty = self.ctx.struct_type(&fields, false);
        let array = llvm(self.alloca.build_alloca(array_ty, "sql_params"))?;
        for (index, name) in params.iter().enumerate() {
            let (pointer, _) = self.data(name, line)?;
            let slot =
                llvm(
                    self.builder
                        .build_struct_gep(array_ty, array, index as u32, "sql_param"),
                )?;
            llvm(self.builder.build_store(slot, pointer))?;
        }
        Ok(SqlArgs {
            text: json,
            len,
            params: array,
            count: self.i64t.const_int(params.len() as u64, false),
        })
    }

    /// Zieladresse: Tabellen-Handle für `INTO TABLE`, sonst der Zeiger auf den Arbeitsbereich bzw. das Feld.
    fn sql_target(&self, target: &str, line: usize) -> CResult<PointerValue<'ctx>> {
        let (p, ty) = self.data(target, line)?;
        // Tabellenziel: das Handle der Tabelle, auch für Tabellenparameter (Feldsymbole)
        if matches!(ty, Ty::Table(_)) {
            return Ok(self.table(target, line)?.0);
        }
        Ok(p)
    }

    fn sy_slot(&self, name: &str) -> CResult<PointerValue<'ctx>> {
        match self.globals.get(name) {
            Some(v) => Ok(v.slot),
            None => err(0, format!("internal error: system field {name} is missing")),
        }
    }

    fn set_sy_subrc(&self, rc: IntValue<'ctx>) -> CResult<()> {
        let slot = self.sy_slot("sy-subrc")?;
        llvm(self.builder.build_store(slot, rc))?;
        Ok(())
    }

    /// SY-DBCNT: der 64-Bit-Zähler der Laufzeit wird auf 32 Bit gekürzt (wie `sy-subrc`).
    fn set_sy_dbcnt(&self, dbcnt: PointerValue<'ctx>) -> CResult<()> {
        let count = llvm(self.builder.build_load(self.i64t, dbcnt, "sql_count"))?.into_int_value();
        let count = llvm(
            self.builder
                .build_int_truncate(count, self.i32t, "sql_count32"),
        )?;
        let slot = self.sy_slot("sy-dbcnt")?;
        llvm(self.builder.build_store(slot, count))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::lower::parse_source;
    use inkwell::context::Context;

    const FLUG: &str = "TYPES: BEGIN OF zflug,\n\
          carrid TYPE c LENGTH 3,\n\
          connid TYPE n LENGTH 4,\n\
          seatsmax TYPE i,\n\
        END OF zflug.\n\
        DATA ls_flug TYPE zflug.\n";

    /// LLVM-IR des übersetzten Programms als Text; ein Übersetzungsfehler bricht den Test ab.
    fn ir_text(src: &str) -> String {
        let ir = parse_source(src).unwrap_or_else(|e| panic!("{e:?}"));
        let ctx = Context::create();
        let mut cg = crate::codegen::Codegen::new(&ctx);
        cg.emit_program(&ir).unwrap_or_else(|e| panic!("{e:?}"));
        cg.module.print_to_string().to_string()
    }

    /// Text einer Funktion von `header` bis zu ihrer schließenden Klammer in der Zeile davor.
    fn function_text<'a>(text: &'a str, header: &str) -> &'a str {
        let start = text
            .find(header)
            .unwrap_or_else(|| panic!("no function {header} in IR"));
        let end = text[start..]
            .find("\n}\n")
            .map_or(text.len(), |e| start + e);
        &text[start..end]
    }

    #[test]
    fn sql_runtime_functions_are_declared_with_abi_signatures() {
        let text = ir_text(&format!("{FLUG}START-OF-SELECTION.\n  WRITE 'x'.\n"));
        assert!(
            text.contains("declare i32 @abap_rt_sql_select(i32, ptr, i64, ptr, i64, ptr, ptr)"),
            "{text}"
        );
        assert!(
            text.contains("declare ptr @abap_rt_sql_cursor_open(i32, ptr, i64, ptr, i64)"),
            "{text}"
        );
        assert!(
            text.contains("declare i32 @abap_rt_sql_cursor_fetch(ptr, ptr, ptr)"),
            "{text}"
        );
        assert!(
            text.contains("declare i32 @abap_rt_sql_cursor_close(ptr)"),
            "{text}"
        );
    }

    #[test]
    fn sy_subrc_and_sy_dbcnt_are_program_globals_starting_at_zero() {
        let text = ir_text(&format!("{FLUG}START-OF-SELECTION.\n  WRITE 'x'.\n"));
        assert!(text.contains("@abap_g_sy-subrc = global i32 0"), "{text}");
        assert!(text.contains("@abap_g_sy-dbcnt = global i32 0"), "{text}");
        // Der Einstieg setzt beide auf 0, bevor das Programm läuft
        assert!(text.contains("store i32 0, ptr @abap_g_sy-subrc"), "{text}");
    }

    #[test]
    fn select_single_calls_runtime_with_request_id_params_and_target() {
        let text = ir_text(&format!(
            "{FLUG}DATA lv_c TYPE c LENGTH 3.\nSTART-OF-SELECTION.\n  \
             SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = @lv_c.\n"
        ));
        assert!(
            text.contains("call i32 @abap_rt_sql_select(i32 0, ptr @"),
            "{text}"
        );
        // Ein Hostparameter: Zeigerfeld mit einem Element, Anzahl 1
        assert!(text.contains("{ ptr }"), "{text}");
        assert!(text.contains(", i64 1, ptr "), "{text}");
        // Rückgabe nach SY-SUBRC, Zähler nach SY-DBCNT
        assert!(text.contains("store i32 %"), "{text}");
        assert!(text.contains("trunc i64"), "{text}");
    }

    #[test]
    fn return_inside_cursor_loop_closes_the_cursor_before_leaving() {
        let text = ir_text(&format!(
            "{FLUG}FORM f.\n  SELECT * FROM zflug INTO ls_flug.\n    RETURN.\n  ENDSELECT.\nENDFORM.\n\
             START-OF-SELECTION.\n  PERFORM f.\n"
        ));
        let form = function_text(&text, "define void @abap_form_f(");
        // Einmal im Schließblock der Schleife, einmal auf dem Weg aus RETURN
        assert!(
            form.matches("call i32 @abap_rt_sql_cursor_close(").count() >= 2,
            "{form}"
        );
    }

    #[test]
    fn raise_inside_try_closes_cursors_of_loops_inside_the_try() {
        let text = ir_text(&format!(
            "CLASS failure DEFINITION INHERITING FROM cx_static_check.\n\
             ENDCLASS.\n\
             CLASS failure IMPLEMENTATION.\n\
             ENDCLASS.\n\
             {FLUG}DATA exception TYPE REF TO failure.\n\
             START-OF-SELECTION.\n\
             TRY.\n\
               SELECT * FROM zflug INTO ls_flug.\n\
                 CREATE OBJECT exception.\n\
                 RAISE EXCEPTION exception.\n\
               ENDSELECT.\n\
             CATCH cx_static_check INTO exception.\n\
               WRITE 'caught'.\n\
             ENDTRY.\n"
        ));
        let main = function_text(&text, "define i32 @main(");
        // Zwischen dem Auslösen und dem nächsten Sprung muss der Cursor geschlossen werden
        let raise = main
            .find("@abap_rt_exception_raise(")
            .unwrap_or_else(|| panic!("no raise call in IR:\n{main}"));
        let jump = main[raise..]
            .find("br label")
            .map(|i| raise + i)
            .unwrap_or_else(|| panic!("no jump after raise:\n{main}"));
        assert!(
            main[raise..jump].contains("@abap_rt_sql_cursor_close("),
            "{main}"
        );
    }

    #[test]
    fn declaration_inside_endselect_body_is_hoisted_to_a_global() {
        let text = ir_text(&format!(
            "{FLUG}START-OF-SELECTION.\n  SELECT * FROM zflug INTO ls_flug.\n    \
             SELECT SINGLE carrid FROM zflug INTO @DATA(lv_c) WHERE connid = '0400'.\n  \
             ENDSELECT.\n"
        ));
        assert!(text.contains("abap_g_lv_c"), "{text}");
    }

    #[test]
    fn write_runtime_functions_are_declared_with_abi_signatures() {
        let text = ir_text(&format!("{FLUG}START-OF-SELECTION.\n  WRITE 'x'.\n"));
        assert!(
            text.contains("declare i32 @abap_rt_sql_execute(i32, ptr, i64, ptr, i64, ptr, ptr)"),
            "{text}"
        );
        assert!(text.contains("declare i32 @abap_rt_sql_commit()"), "{text}");
        assert!(
            text.contains("declare i32 @abap_rt_sql_rollback()"),
            "{text}"
        );
        assert!(
            text.contains("declare void @abap_rt_sql_finish()"),
            "{text}"
        );
    }

    #[test]
    fn insert_from_work_area_passes_the_work_area_as_target() {
        let text = ir_text(&format!(
            "{FLUG}START-OF-SELECTION.\n  INSERT INTO zflug VALUES ls_flug.\n"
        ));
        let main = function_text(&text, "define i32 @main(");
        assert!(
            main.contains("call i32 @abap_rt_sql_execute(i32 0, ptr @")
                && main.contains("ptr null, i64 0, ptr @abap_g_ls_flug, ptr %"),
            "{main}"
        );
        // Rückgabe nach SY-SUBRC, Zähler nach SY-DBCNT
        assert!(main.contains("trunc i64"), "{main}");
    }

    #[test]
    fn delete_by_set_passes_a_null_target_and_host_parameters() {
        let text = ir_text(&format!(
            "{FLUG}DATA lv_c TYPE c LENGTH 3.\nSTART-OF-SELECTION.\n  \
             DELETE FROM zflug WHERE carrid = @lv_c.\n"
        ));
        let main = function_text(&text, "define i32 @main(");
        assert!(
            main.contains("call i32 @abap_rt_sql_execute(i32 0, ptr @"),
            "{main}"
        );
        // Ein Hostparameter, keine Zeilenquelle
        assert!(main.contains("{ ptr }"), "{main}");
        assert!(main.contains(", i64 1, ptr "), "{main}");
        assert!(main.contains(", ptr null, ptr %"), "{main}");
    }

    #[test]
    fn commit_and_rollback_call_the_runtime_and_leave_sy_subrc_unchanged() {
        let text = ir_text(&format!(
            "{FLUG}START-OF-SELECTION.\n  COMMIT WORK.\n  ROLLBACK WORK.\n"
        ));
        let main = function_text(&text, "define i32 @main(");
        let commit = main
            .find("call i32 @abap_rt_sql_commit()")
            .unwrap_or_else(|| panic!("no commit call:\n{main}"));
        let rollback = main
            .find("call i32 @abap_rt_sql_rollback()")
            .unwrap_or_else(|| panic!("no rollback call:\n{main}"));
        assert!(commit < rollback, "{main}");
        assert!(
            !main[commit..rollback].contains("@abap_g_sy-subrc"),
            "COMMIT WORK must not change SY-SUBRC:\n{main}"
        );
    }

    #[test]
    fn program_with_sql_requests_finishes_the_runtime_before_returning() {
        let with_sql = ir_text(&format!(
            "{FLUG}START-OF-SELECTION.\n  INSERT INTO zflug VALUES ls_flug.\n"
        ));
        let main = function_text(&with_sql, "define i32 @main(");
        let finish = main
            .find("call void @abap_rt_sql_finish()")
            .unwrap_or_else(|| panic!("no finish call:\n{main}"));
        let ret = main.find("ret i32 0").expect("main returns 0");
        assert!(finish < ret, "{main}");
    }

    #[test]
    fn program_without_sql_requests_keeps_main_free_of_the_finish_call() {
        let without_sql = ir_text(&format!("{FLUG}START-OF-SELECTION.\n  WRITE 'x'.\n"));
        let main = function_text(&without_sql, "define i32 @main(");
        assert!(!main.contains("@abap_rt_sql_finish()"), "{main}");
    }
}
