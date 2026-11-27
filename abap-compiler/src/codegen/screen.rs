//! Dynpro-Aufrufe (Spec 5, 8.2): Prolog aus der `.dynp` und die Übersetzung der Screen-Anweisungen des Programms.
//! Alle Zeichenketten gehen als UTF-16 mit Länge in Zeichen (`lit` und `len`), Zahlen als `u32` (i32 in LLVM).

use super::*;
use crate::dynpro::{color_bits, Dynpro, DynproFile, DynproItem, ElementKind, GuiStatus, Phase};
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
                    let key = name.to_lowercase();
                    let Some(ty) = ir.field_types.get(&key).copied() else {
                        return err(el.line, format!("Field '{name}' is not declared"));
                    };
                    let (Some(code), Some(length)) = (ty.abi_code(), ty.abi_length()) else {
                        return err(
                            el.line,
                            format!("Type of dynpro field '{name}' is not supported"),
                        );
                    };
                    let data = self.global_ptr(&key, el.line)?;
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
        for fe in &d.flow {
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
        self.call_void(self.rt.screen_end, &[no.into()])
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
        self.call_void(self.rt.status_end, &[])
    }

    /// Übersetzt eine Dynpro-Anweisung in den Laufzeitaufruf (Spec 5).
    pub(super) fn emit_screen_op(&mut self, op: &ScreenOp) -> CResult<()> {
        match op {
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
}
