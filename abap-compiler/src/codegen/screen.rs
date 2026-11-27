//! Dynpro-Aufrufe (Spec 5, 8.2): Prolog aus der `.dynp` und die Übersetzung der Screen-Anweisungen des Programms.
//! Alle Zeichenketten gehen als UTF-16 mit Länge in Zeichen (`lit` und `len`), Zahlen als `u32` (i32 in LLVM).

use super::*;
use crate::dynpro::{
    color_bits, Cond, Dynpro, DynproFile, DynproItem, ElementKind, FieldFlow, FieldType, FlowItem,
    GuiStatus, Phase,
};
use crate::lower::{Ir, ScreenOp};

/// Kennung der Ablaufphase für `abap_rt_screen_module`: 0 PBO, 1 PAI.
fn phase_code(phase: Phase) -> u32 {
    match phase {
        Phase::Pbo => 0,
        Phase::Pai => 1,
    }
}

impl<'ctx> Codegen<'ctx> {
    /// Zahl als LLVM-`i32` (Rust `u32`).
    fn u32c(&self, v: u32) -> IntValue<'ctx> {
        self.i32t.const_int(u64::from(v), false)
    }

    /// Länge einer Zeichenkette in UTF-16-Einheiten.
    fn utf16_len(&self, s: &str) -> IntValue<'ctx> {
        self.len(s.encode_utf16().count())
    }

    /// Speicherplatz einer Programmvariable (Global), für die Feldbindung.
    fn global_ptr(&self, name: &str, line: usize) -> CResult<PointerValue<'ctx>> {
        match self.globals.get(name) {
            Some(v) => Ok(v.slot),
            None => err(line, format!("Field '{name}' is not declared")),
        }
    }

    /// Funktion eines MODULE-Blocks, für `abap_rt_screen_module`.
    fn module_fn(&self, phase: Phase, name: &str, line: usize) -> CResult<FunctionValue<'ctx>> {
        match self.module_fns.get(&(phase, name.to_lowercase())) {
            Some(f) => Ok(*f),
            None => err(
                line,
                format!("MODULE '{name}' is not defined for this phase"),
            ),
        }
    }

    /// Prolog (Spec 8.2): zuerst die SY-Felder, dann Dynpros und GUI-Status in der Reihenfolge der Datei.
    pub(super) fn emit_screen_prolog(&mut self, file: &DynproFile, ir: &Ir) -> CResult<()> {
        let ucomm = self.global_ptr("sy-ucomm", 0)?;
        let dynnr = self.global_ptr("sy-dynnr", 0)?;
        self.call_void(
            self.rt.screen_sy,
            &[
                ucomm.into(),
                self.len(UCOMM_LENGTH).into(),
                dynnr.into(),
                self.len(4).into(),
            ],
        )?;
        for item in &file.items {
            match item {
                DynproItem::Dynpro(d) => self.emit_dynpro(d, ir)?,
                DynproItem::Status(s) => self.emit_status(s)?,
            }
        }
        Ok(())
    }

    /// Typ und Speicherort eines Dynpro-Feldes. Ein Name ohne `-` ist eine Programmvariable. `struktur-komponente`
    /// ist die Komponente einer globalen Struktur: Adresse per GEP auf das Layout der Struktur (Spec 8.2).
    fn field_binding(
        &self,
        ir: &Ir,
        name: &str,
        line: usize,
    ) -> CResult<(FieldType, PointerValue<'ctx>)> {
        let key = name.to_lowercase();
        let Some((base, _)) = key.split_once('-') else {
            let Some(ty) = ir.field_types.get(&key).copied() else {
                return err(line, format!("Field '{name}' is not declared"));
            };
            return Ok((ty, self.global_ptr(&key, line)?));
        };
        let (idx, comp, ty) = match crate::dynpro::component(ir, name) {
            Ok(found) => found,
            Err(message) => return err(line, message),
        };
        let base_ptr = self.global_ptr(base, line)?;
        let st = self.layouts[idx].st;
        let fp = llvm(
            self.builder
                .build_struct_gep(st, base_ptr, comp as u32, "dynp_comp"),
        )?;
        Ok((ty, fp))
    }

    /// Ein Dynpro: Kopf, Elemente in Dateireihenfolge, Ablauflogik, Ende.
    fn emit_dynpro(&mut self, d: &Dynpro, ir: &Ir) -> CResult<()> {
        let no = self.u32c(d.number);
        self.call_void(
            self.rt.screen_begin,
            &[
                no.into(),
                self.u32c(u32::from(d.modal)).into(),
                self.u32c(d.rows as u32).into(),
                self.u32c(d.cols as u32).into(),
                self.lit(&d.title).into(),
                self.utf16_len(&d.title).into(),
            ],
        )?;
        for el in &d.elements {
            let row = self.u32c(el.row as u32);
            let col = self.u32c(el.col as u32);
            match &el.kind {
                ElementKind::Text {
                    text,
                    color,
                    intensified,
                } => {
                    let attrs = color_bits(*color) | if *intensified { 32 } else { 0 };
                    self.call_void(
                        self.rt.screen_text,
                        &[
                            row.into(),
                            col.into(),
                            self.lit(text).into(),
                            self.utf16_len(text).into(),
                            self.u32c(attrs).into(),
                        ],
                    )?;
                }
                ElementKind::Field { name, attrs } => {
                    let (ty, data) = self.field_binding(ir, name, el.line)?;
                    let (Some(code), Some(length)) = (ty.abi_code(), ty.abi_length()) else {
                        return err(
                            el.line,
                            format!("Type of dynpro field '{name}' is not supported"),
                        );
                    };
                    self.call_void(
                        self.rt.screen_field,
                        &[
                            row.into(),
                            col.into(),
                            self.lit(name).into(),
                            self.utf16_len(name).into(),
                            data.into(),
                            self.u32c(code).into(),
                            self.u32c(length as u32).into(),
                            self.u32c(ty.abi_decimals() as u32).into(),
                            self.u32c(attrs.bits()).into(),
                        ],
                    )?;
                }
                ElementKind::Button { text, code } => {
                    self.call_void(
                        self.rt.screen_button,
                        &[
                            row.into(),
                            col.into(),
                            self.lit(text).into(),
                            self.utf16_len(text).into(),
                            self.lit(code).into(),
                            self.utf16_len(code).into(),
                            self.u32c(0).into(),
                        ],
                    )?;
                }
                ElementKind::Frame { row2, col2, title } => {
                    self.call_void(
                        self.rt.screen_frame,
                        &[
                            row.into(),
                            col.into(),
                            self.u32c(*row2 as u32).into(),
                            self.u32c(*col2 as u32).into(),
                            self.lit(title).into(),
                            self.utf16_len(title).into(),
                        ],
                    )?;
                }
            }
        }
        // Ablauflogik in Quelltextfolge (Spec 12a): MODULE ohne Feldbezug, FIELD und Kettenmodule der PAI
        for item in &d.flow {
            match item {
                FlowItem::Module(fe) => {
                    if fe.cond != Cond::Plain {
                        return err(
                            fe.line,
                            format!("{} is only allowed for FIELD", fe.cond.text()),
                        );
                    }
                    let func = self.module_fn(fe.phase, &fe.module, fe.line)?;
                    self.call_void(
                        self.rt.screen_module,
                        &[
                            no.into(),
                            self.u32c(phase_code(fe.phase)).into(),
                            func.as_global_value().as_pointer_value().into(),
                            self.u32c(u32::from(fe.at_exit)).into(),
                        ],
                    )?;
                }
                FlowItem::Field(ff) => self.emit_pai_field(d, no, ff)?,
                FlowItem::ChainModule(cm) => {
                    if !matches!(cm.cond, Cond::ChainInput | Cond::ChainRequest) {
                        return err(
                            cm.line,
                            "MODULE in CHAIN needs ON CHAIN-INPUT or ON CHAIN-REQUEST",
                        );
                    }
                    let func = self.optional_module_ptr(Some(&cm.module), cm.line)?;
                    self.call_void(
                        self.rt.screen_pai_chain,
                        &[
                            no.into(),
                            self.u32c(cm.chain as u32).into(),
                            func.into(),
                            self.u32c(cm.cond.abi_code()).into(),
                        ],
                    )?;
                }
            }
        }
        self.call_void(self.rt.screen_end, &[no.into()])
    }

    /// Funktionszeiger eines PAI-Moduls, oder ein Null-Zeiger, wenn kein Modul angegeben ist (Spec 12a).
    fn optional_module_ptr(
        &self,
        module: Option<&str>,
        line: usize,
    ) -> CResult<PointerValue<'ctx>> {
        match module {
            Some(m) => {
                let func = self.module_fn(Phase::Pai, m, line)?;
                Ok(func.as_global_value().as_pointer_value())
            }
            None => Ok(self.ptrt.const_null()),
        }
    }

    /// `FIELD f [MODULE m [ON ...]]` in der PAI-Folge: `abap_rt_screen_pai_field`. Der Name ist die Schreibweise
    /// des `FIELD`-Elements, damit Laufzeit und Element übereinstimmen. Die Kettennummer ist 0 ohne Kette.
    fn emit_pai_field(&mut self, d: &Dynpro, no: IntValue<'ctx>, ff: &FieldFlow) -> CResult<()> {
        let cond = match ff.cond {
            Cond::Plain | Cond::Input | Cond::Request => ff.cond.abi_code(),
            other => {
                return err(
                    ff.line,
                    format!("{} is only allowed for MODULE in a CHAIN", other.text()),
                )
            }
        };
        let Some(name) = d.elements.iter().find_map(|el| match &el.kind {
            ElementKind::Field { name, .. } if name.to_lowercase() == ff.name.to_lowercase() => {
                Some(name.as_str())
            }
            _ => None,
        }) else {
            return err(
                ff.line,
                format!(
                    "FIELD '{}' in the flow is not a FIELD of DYNPRO {:04}",
                    ff.name, d.number
                ),
            );
        };
        let func = self.optional_module_ptr(ff.module.as_deref(), ff.line)?;
        self.call_void(
            self.rt.screen_pai_field,
            &[
                no.into(),
                self.lit(name).into(),
                self.utf16_len(name).into(),
                func.into(),
                self.u32c(cond).into(),
                self.u32c(ff.chain.unwrap_or(0) as u32).into(),
            ],
        )
    }

    /// GUI-Status mit seinen Funktionstasten. Der Name geht in Großbuchstaben an die Laufzeit,
    /// wie `SET PF-STATUS` (siehe `lower/screen.rs`).
    fn emit_status(&mut self, s: &GuiStatus) -> CResult<()> {
        let name = s.name.to_uppercase();
        self.call_void(
            self.rt.status_begin,
            &[self.lit(&name).into(), self.utf16_len(&name).into()],
        )?;
        for k in &s.keys {
            self.call_void(
                self.rt.status_fkey,
                &[
                    self.u32c(k.key).into(),
                    self.lit(&k.text).into(),
                    self.utf16_len(&k.text).into(),
                    self.lit(&k.code).into(),
                    self.utf16_len(&k.code).into(),
                    self.u32c(k.kind.abi_code()).into(),
                ],
            )?;
        }
        // EXCLUDE nach den Funktionstasten, vor dem Ende des Status (Spec 8.2)
        for e in &s.excludes {
            self.call_void(
                self.rt.status_exclude,
                &[self.lit(&e.code).into(), self.utf16_len(&e.code).into()],
            )?;
        }
        self.call_void(self.rt.status_end, &[])
    }

    /// Übersetzt eine Dynpro-Anweisung in den Laufzeitaufruf (Spec 5).
    pub(super) fn emit_screen_op(&mut self, op: &ScreenOp, line: usize) -> CResult<()> {
        match op {
            ScreenOp::SetCursor(name) => self.call_void(
                self.rt.set_cursor,
                &[self.lit(name).into(), self.utf16_len(name).into()],
            ),
            ScreenOp::GetCursor(target) => match self.dest(target, line)? {
                Dest::Chars(p, n) => {
                    self.call_void(self.rt.get_cursor, &[p.into(), self.len(n).into()])
                }
                _ => err(line, "GET CURSOR FIELD expects a character variable"),
            },
            ScreenOp::SetTitlebar(text) => self.call_void(
                self.rt.set_titlebar,
                &[self.lit(text).into(), self.utf16_len(text).into()],
            ),
            ScreenOp::Call(n) => self.call_void(self.rt.call_screen, &[self.u32c(*n).into()]),
            ScreenOp::Set(n) => self.call_void(self.rt.set_screen, &[self.u32c(*n).into()]),
            ScreenOp::Leave => self.call_void(self.rt.leave_screen, &[]),
            ScreenOp::LeaveTo(n) => {
                self.call_void(self.rt.leave_to_screen, &[self.u32c(*n).into()])
            }
            ScreenOp::LeaveProgram => self.call_void(self.rt.leave_program, &[]),
            ScreenOp::PfStatus(name) => self.call_void(
                self.rt.set_pf_status,
                &[self.lit(name).into(), self.utf16_len(name).into()],
            ),
            ScreenOp::Message { kind, text } => self.call_void(
                self.rt.message,
                &[
                    self.u32c(u32::from(*kind)).into(),
                    self.lit(text).into(),
                    self.utf16_len(text).into(),
                ],
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynpro::parse_dynp;
    use crate::lower::parse_source;
    use inkwell::context::Context;

    const ABAP: &str = "\
DATA zkunnr TYPE c LENGTH 10.
DATA zname1 TYPE c LENGTH 30.
START-OF-SELECTION.
  CALL SCREEN 0100.
MODULE status_0100 OUTPUT.
  SET PF-STATUS 'status_0100'.
ENDMODULE.
MODULE user_command_0100 INPUT.
  LEAVE SCREEN.
ENDMODULE.
";

    const DYNP: &str = "\
DYNPRO 0100
  TITLE 'Kunde'
  TEXT 2 2 'Kundennummer'
  FIELD ZKUNNR 2 20 INPUT REQUIRED
  FIELD ZNAME1 4 20 INPUT
  BUTTON 9 2 'Sichern' CODE SAVE
  FLOW
    PROCESS BEFORE OUTPUT
      MODULE STATUS_0100
    PROCESS AFTER INPUT
      MODULE USER_COMMAND_0100 AT EXIT-COMMAND
  ENDFLOW
ENDDYNPRO
GUISTATUS STATUS_0100
  FKEY F3 'Zurueck' CODE BACK TYPE B
ENDGUISTATUS
";

    /// Übersetzt Programm und Dynpro-Datei zur IR mit angehängter Datei.
    fn screen_program(abap: &str, dynp: &str) -> Ir {
        let mut ir = parse_source(abap).expect("program translates");
        let file = parse_dynp("prog.dynp", dynp).expect("dynpro parses");
        let diags = crate::dynpro::check(Some(&file), &ir, "prog.abap");
        assert!(diags.iter().all(|d| d.warning), "{diags:?}");
        ir.screens = Some(file);
        ir
    }

    /// Position des ersten Aufrufs `name(` im Modultext, bzw. panic, wenn er fehlt.
    fn call_pos(ll: &str, name: &str) -> usize {
        ll.find(&format!("call void @{name}("))
            .unwrap_or_else(|| panic!("no call to {name} in IR"))
    }

    #[test]
    fn prolog_registers_sy_fields_then_dynpro_and_status_in_file_order() {
        let ir = screen_program(ABAP, DYNP);
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        cg.emit_program(&ir).expect("codegen succeeds");
        let ll = cg.module.print_to_string().to_string();

        let order = [
            "abap_rt_screen_sy",
            "abap_rt_screen_begin",
            "abap_rt_screen_text",
            "abap_rt_screen_field",
            "abap_rt_screen_button",
            "abap_rt_screen_module",
            "abap_rt_screen_end",
            "abap_rt_status_begin",
            "abap_rt_status_fkey",
            "abap_rt_status_end",
            "abap_rt_call_screen",
        ];
        let positions: Vec<usize> = order.iter().map(|n| call_pos(&ll, n)).collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "calls out of order: {positions:?}"
        );
        // Felder: zwei FIELD-Aufrufe
        assert_eq!(ll.matches("call void @abap_rt_screen_field(").count(), 2);
    }

    #[test]
    fn module_functions_and_sy_globals_exist_in_ir() {
        let ir = screen_program(ABAP, DYNP);
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        cg.emit_program(&ir).expect("codegen succeeds");
        let ll = cg.module.print_to_string().to_string();
        assert!(ll.contains("define void @abap_module_pbo_status_0100()"));
        assert!(ll.contains("define void @abap_module_pai_user_command_0100()"));
        assert!(ll.contains("@abap_g_sy-ucomm"));
        assert!(ll.contains("@abap_g_sy-dynnr"));
    }

    #[test]
    fn program_without_dynpro_file_has_no_prolog_calls() {
        let ir = parse_source("DATA x TYPE i.\nWRITE x.\n").expect("program translates");
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        cg.emit_program(&ir).expect("codegen succeeds");
        let ll = cg.module.print_to_string().to_string();
        assert!(!ll.contains("call void @abap_rt_screen_sy("));
    }

    #[test]
    fn field_registration_uses_address_of_global_and_abi_type() {
        let ir = screen_program(ABAP, DYNP);
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        cg.emit_program(&ir).expect("codegen succeeds");
        let ll = cg.module.print_to_string().to_string();
        // Feld ZKUNNR: c, Länge 10; die Adresse ist die Global-Variable
        let line = ll
            .lines()
            .find(|l| l.contains("call void @abap_rt_screen_field(") && l.contains("i32 10"))
            .expect("field call with length 10");
        assert!(line.contains("@abap_g_zkunnr"), "{line}");
    }

    /// Programm mit globaler Struktur ZKUNDE (NAME1 c30, NR n4) statt ZNAME1.
    fn abap_with_struct() -> String {
        ABAP.replace(
            "DATA zname1 TYPE c LENGTH 30.",
            "DATA: BEGIN OF zkunde,\n  name1 TYPE c LENGTH 30,\n  nr TYPE n LENGTH 4,\nEND OF zkunde.",
        )
    }

    /// Ll-Text eines Programms mit der Dynpro-Datei, ohne Prüfung auf Warnungen.
    fn ll_of(ir: &Ir) -> String {
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        cg.emit_program(ir).expect("codegen succeeds");
        cg.module.print_to_string().to_string()
    }

    #[test]
    fn struct_component_field_is_bound_by_gep_with_component_length() {
        let dynp = DYNP.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NAME1 4 20 INPUT");
        let ir = screen_program(&abap_with_struct(), &dynp);
        let ll = ll_of(&ir);
        // Komponente NAME1 ist Index 0 der Struktur: GEP auf die Globale ZKUNDE, Länge 30 (c30)
        let line = ll
            .lines()
            .find(|l| l.contains("call void @abap_rt_screen_field(") && l.contains("i32 30"))
            .expect("field call with length 30");
        // Komponente 0 liegt am Anfang der Struktur: die Adresse ist die Basis der Globalen
        assert!(line.contains("ptr @abap_g_zkunde,"), "{line}");
    }

    #[test]
    fn struct_component_second_index_uses_its_own_offset_and_type() {
        let dynp = DYNP.replace("FIELD ZNAME1 4 20 INPUT", "FIELD ZKUNDE-NR 4 20 INPUT");
        let ir = screen_program(&abap_with_struct(), &dynp);
        let ll = ll_of(&ir);
        // NR ist Komponente 1 (n4): ABI-Typ 1 (n), Länge 4, GEP-Index 1
        let line = ll
            .lines()
            .find(|l| l.contains("call void @abap_rt_screen_field(") && l.contains("i32 1,"))
            .expect("numc field call");
        assert!(line.contains("i32 4, i32 0,"), "{line}");
        assert!(
            line.contains("i32 0, i32 1)"),
            "GEP index 1 expected: {line}"
        );
    }

    #[test]
    fn password_attribute_sets_bit_16_in_field_call() {
        let dynp = DYNP.replace(
            "FIELD ZNAME1 4 20 INPUT",
            "FIELD ZNAME1 4 20 INPUT PASSWORD",
        );
        let ll = ll_of(&screen_program(ABAP, &dynp));
        // INPUT (1) und PASSWORD (65536)
        assert!(ll.contains("i32 65537"), "{ll}");
    }

    #[test]
    fn exclude_is_registered_after_function_keys_and_before_status_end() {
        let dynp = DYNP.replace("ENDGUISTATUS", "  EXCLUDE PRINT\nENDGUISTATUS");
        let ll = ll_of(&screen_program(ABAP, &dynp));
        let exclude = ll
            .find("call void @abap_rt_status_exclude(")
            .expect("status_exclude call");
        let fkey = ll
            .find("call void @abap_rt_status_fkey(")
            .expect("fkey call");
        let end = ll
            .find("call void @abap_rt_status_end(")
            .expect("status_end call");
        assert!(fkey < exclude && exclude < end);
    }

    #[test]
    fn function_key_24_is_sent_with_key_number_24() {
        let dynp = DYNP.replace(
            "FKEY F3 'Zurueck' CODE BACK TYPE B",
            "FKEY F24 'Ende' CODE END",
        );
        let ll = ll_of(&screen_program(ABAP, &dynp));
        assert!(
            ll.contains("call void @abap_rt_status_fkey(i32 24,"),
            "{ll}"
        );
    }

    #[test]
    fn set_cursor_passes_upper_case_name_and_length() {
        let abap = ABAP.replace(
            "  LEAVE SCREEN.",
            "  SET CURSOR FIELD zkunnr.\n  LEAVE SCREEN.",
        );
        let ll = ll_of(&screen_program(&abap, DYNP));
        // "ZKUNNR" hat 6 Zeichen
        assert!(
            ll.lines()
                .any(|l| l.contains("call void @abap_rt_set_cursor(") && l.contains("i64 6")),
            "{ll}"
        );
    }

    #[test]
    fn get_cursor_passes_address_and_length_of_character_variable() {
        let abap = ABAP.replace(
            "  LEAVE SCREEN.",
            "  GET CURSOR FIELD zkunnr.\n  LEAVE SCREEN.",
        );
        let ll = ll_of(&screen_program(&abap, DYNP));
        // zkunnr ist c10: Länge 10 in UTF-16-Einheiten, Adresse der Globalen
        let line = ll
            .lines()
            .find(|l| l.contains("call void @abap_rt_get_cursor("))
            .expect("get_cursor call");
        assert!(
            line.contains("@abap_g_zkunnr") && line.contains("i64 10"),
            "{line}"
        );
    }

    #[test]
    fn get_cursor_into_numeric_variable_is_rejected_by_codegen() {
        let abap = ABAP
            .replace("DATA zkunnr TYPE c LENGTH 10.", "DATA zkunnr TYPE i.")
            .replace(
                "  LEAVE SCREEN.",
                "  GET CURSOR FIELD zkunnr.\n  LEAVE SCREEN.",
            );
        let ir = parse_source(&abap).expect("program translates");
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        let e = cg.emit_program(&ir).unwrap_err();
        assert!(format!("{e:?}").contains("GET CURSOR FIELD expects a character variable"));
    }

    #[test]
    fn set_titlebar_passes_text_and_length() {
        let abap = ABAP.replace(
            "  LEAVE SCREEN.",
            "  SET TITLEBAR 'Kunde'.\n  LEAVE SCREEN.",
        );
        let ll = ll_of(&screen_program(&abap, DYNP));
        assert!(
            ll.lines()
                .any(|l| l.contains("call void @abap_rt_set_titlebar(") && l.contains("i64 5")),
            "{ll}"
        );
    }

    /// Programm mit zusätzlichem INPUT-Modul CHECK_KUNNR für die PAI-Folge.
    fn abap_with_check() -> String {
        format!("{ABAP}MODULE check_kunnr INPUT.\nENDMODULE.\n")
    }

    /// `DYNP` mit anderer PAI-Folge: die Zeilen unter `PROCESS AFTER INPUT` werden ersetzt.
    fn dynp_with_pai(pai: &str) -> String {
        DYNP.replace(
            "    PROCESS AFTER INPUT\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n",
            &format!("    PROCESS AFTER INPUT\n{pai}"),
        )
    }

    /// Namen der `abap_rt_screen_*`-Aufrufe im IR-Text, in der Reihenfolge des Textes.
    fn screen_call_names(ll: &str) -> Vec<String> {
        ll.lines()
            .filter_map(|l| {
                let start = l.find("call void @abap_rt_screen_")? + "call void @".len();
                let end = start + l[start..].find('(')?;
                Some(l[start..end].to_string())
            })
            .collect()
    }

    /// Die Zeile des Aufrufs `name(`, die auf `tail` endet.
    fn call_line<'a>(ll: &'a str, name: &str, tail: &str) -> &'a str {
        ll.lines()
            .find(|l| l.contains(&format!("call void @{name}(")) && l.ends_with(tail))
            .unwrap_or_else(|| panic!("no call to {name} ending with '{tail}' in IR:\n{ll}"))
    }

    #[test]
    fn pai_entries_are_registered_in_flow_order_after_pbo_module() {
        let dynp = dynp_with_pai(
            "      FIELD ZKUNNR MODULE CHECK_KUNNR ON INPUT\n      CHAIN\n        FIELD ZNAME1\n        MODULE CHECK_KUNNR ON CHAIN-REQUEST\n      ENDCHAIN\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n",
        );
        let ll = ll_of(&screen_program(&abap_with_check(), &dynp));
        let flow: Vec<String> = screen_call_names(&ll)
            .into_iter()
            .filter(|n| {
                matches!(
                    n.as_str(),
                    "abap_rt_screen_module"
                        | "abap_rt_screen_pai_field"
                        | "abap_rt_screen_pai_chain"
                        | "abap_rt_screen_end"
                )
            })
            .collect();
        assert_eq!(
            flow,
            vec![
                "abap_rt_screen_module",
                "abap_rt_screen_pai_field",
                "abap_rt_screen_pai_field",
                "abap_rt_screen_pai_chain",
                "abap_rt_screen_module",
                "abap_rt_screen_end",
            ]
        );
    }

    #[test]
    fn pai_field_with_input_condition_passes_module_pointer_and_chain_zero() {
        let dynp = dynp_with_pai("      FIELD ZKUNNR MODULE CHECK_KUNNR ON INPUT\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n");
        let ll = ll_of(&screen_program(&abap_with_check(), &dynp));
        // no = 100, func = Modulfunktion, cond = 1 (ON INPUT), chain = 0 (ohne Kette)
        let line = call_line(
            &ll,
            "abap_rt_screen_pai_field",
            "ptr @abap_module_pai_check_kunnr, i32 1, i32 0)",
        );
        assert!(
            line.contains("call void @abap_rt_screen_pai_field(i32 100, ptr @"),
            "{line}"
        );
    }

    #[test]
    fn pai_field_without_module_passes_null_pointer() {
        let dynp =
            dynp_with_pai("      FIELD ZNAME1\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n");
        let ll = ll_of(&screen_program(&abap_with_check(), &dynp));
        call_line(&ll, "abap_rt_screen_pai_field", "ptr null, i32 0, i32 0)");
    }

    #[test]
    fn pai_field_on_request_has_condition_code_2() {
        let dynp = dynp_with_pai("      FIELD ZNAME1 MODULE CHECK_KUNNR ON REQUEST\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n");
        let ll = ll_of(&screen_program(&abap_with_check(), &dynp));
        call_line(
            &ll,
            "abap_rt_screen_pai_field",
            "ptr @abap_module_pai_check_kunnr, i32 2, i32 0)",
        );
    }

    #[test]
    fn chain_field_carries_chain_number_and_chain_module_its_condition() {
        let dynp = dynp_with_pai(
            "      CHAIN\n        FIELD ZKUNNR\n      ENDCHAIN\n      CHAIN\n        FIELD ZNAME1\n        MODULE CHECK_KUNNR ON CHAIN-REQUEST\n      ENDCHAIN\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n",
        );
        let ll = ll_of(&screen_program(&abap_with_check(), &dynp));
        // Felder ohne Modul: null, Kette 1 bzw. 2
        call_line(&ll, "abap_rt_screen_pai_field", "ptr null, i32 0, i32 1)");
        call_line(&ll, "abap_rt_screen_pai_field", "ptr null, i32 0, i32 2)");
        // Kettenmodul: no 100, Kette 2, Modul, cond 4 (ON CHAIN-REQUEST)
        call_line(
            &ll,
            "abap_rt_screen_pai_chain",
            "(i32 100, i32 2, ptr @abap_module_pai_check_kunnr, i32 4)",
        );
    }

    #[test]
    fn field_in_flow_that_is_not_an_element_is_a_codegen_error() {
        let dynp =
            dynp_with_pai("      FIELD ZFEHLT\n      MODULE USER_COMMAND_0100 AT EXIT-COMMAND\n");
        let mut ir = parse_source(&abap_with_check()).expect("program translates");
        ir.screens = Some(parse_dynp("prog.dynp", &dynp).expect("dynpro parses"));
        let ctx = Context::create();
        let mut cg = Codegen::new(&ctx);
        let e = cg.emit_program(&ir).unwrap_err();
        assert!(format!("{e:?}").contains("'ZFEHLT' in the flow is not a FIELD of DYNPRO 0100"));
    }
}
