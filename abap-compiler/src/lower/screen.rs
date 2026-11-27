//! Dynpro-Anweisungen im ABAP-Programm (Spec 5): `CALL SCREEN`, `SET SCREEN`, `LEAVE SCREEN`,
//! `LEAVE TO SCREEN`, `LEAVE PROGRAM`, `SET PF-STATUS`, `MESSAGE 'text' TYPE t`, sowie die Typen der
//! Variablen für die Prüfung der `.dynp`-Felder (Spec 4.7). Die Zuordnung zu den Laufzeitaufrufen steht im Codegen.
//!
//! Nicht unterstützt (Phase 2): `CALL SCREEN ... STARTING AT` mit Variablen als Koordinaten, `SET PF-STATUS ... OF
//! PROGRAM / EXCLUDING`, `SET SCREEN 0`, `MESSAGE` mit Nachrichtenklasse oder Ersatzwerten.

use super::*;
use crate::dynpro::{FieldType, UCOMM_LENGTH};

impl Lowerer {
    /// Übersetzt eine Dynpro-Anweisung. `None`, wenn die Anweisung keine ist; dann greift der übliche Pfad.
    pub(super) fn lower_screen_stmt(
        &mut self,
        kw: &str,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Option<Result<(), CompileError>> {
        let is = |i: usize, w: &str| toks.get(i).is_some_and(|t| t.is_word(w));
        let result = match kw {
            "CALL" if is(1, "SCREEN") => self.call_screen(toks, line, out),
            "SET" if is(1, "PF-STATUS") => self.set_pf_status(toks, line, out),
            "SET" if is(1, "SCREEN") => self.set_screen(toks, line, out),
            "SET" if is(1, "CURSOR") => self.set_cursor(toks, line, out),
            "GET" if is(1, "CURSOR") => self.get_cursor(toks, line, out),
            "SET" if is(1, "TITLEBAR") => self.set_titlebar(toks, line, out),
            "LEAVE" if toks.len() == 2 && is(1, "SCREEN") => {
                out.push(Stmt::Screen {
                    op: ScreenOp::Leave,
                    line,
                });
                Ok(())
            }
            "LEAVE" if toks.len() == 2 && is(1, "PROGRAM") => {
                out.push(Stmt::Screen {
                    op: ScreenOp::LeaveProgram,
                    line,
                });
                Ok(())
            }
            "LEAVE" if is(1, "TO") && is(2, "SCREEN") => self.leave_to_screen(toks, line, out),
            "MESSAGE" if toks.len() == 4 && toks[1].kind == Kind::Literal && is(2, "TYPE") => {
                self.message(toks, line, out)
            }
            _ => return None,
        };
        Some(result)
    }

    /// `CALL SCREEN n.` und `CALL SCREEN n STARTING AT x y [ENDING AT x2 y2].`
    fn call_screen(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if toks.get(3).is_some_and(|t| t.is_word("STARTING")) {
            return self.call_screen_modal(toks, line, out);
        }
        if toks.len() != 3 {
            return err(
                line,
                "Unsupported form of CALL SCREEN (expected: CALL SCREEN <number>)",
            );
        }
        let n = dynpro_number(&toks[2], line)?;
        if n == 0 {
            return err(line, "Dynpro number must be 1 to 9999, found '0'");
        }
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Dynpro(n),
            what: "CALL SCREEN",
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::Call(n),
            line,
        });
        Ok(())
    }

    /// `CALL SCREEN n STARTING AT x y [ENDING AT x2 y2].` Koordinaten nur als ganze Zahlen (Literale, Spec 12c).
    /// Bereiche: x, y, x2, y2 liegen im Terminal (80 × 24), x2 ≥ x und y2 ≥ y. Dass das Dynpro `KIND MODAL` ist,
    /// prüft `dynpro/check.rs` gegen die `.dynp`-Datei.
    fn call_screen_modal(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let is = |i: usize, w: &str| toks.get(i).is_some_and(|t| t.is_word(w));
        let shape_ok = is(4, "AT")
            && match toks.len() {
                7 => true,
                11 => is(7, "ENDING") && is(8, "AT"),
                _ => false,
            };
        if !shape_ok {
            return err(
                line,
                "Unsupported form of CALL SCREEN ... STARTING AT (expected: CALL SCREEN <number> STARTING AT <x> <y> [ENDING AT <x2> <y2>])",
            );
        }
        let n = dynpro_number(&toks[2], line)?;
        let x = window_value(&toks[5], "STARTING AT column", 80, line)?;
        let y = window_value(&toks[6], "STARTING AT line", 24, line)?;
        let (x2, y2) = if toks.len() == 11 {
            let x2 = window_value(&toks[9], "ENDING AT column", 80, line)?;
            let y2 = window_value(&toks[10], "ENDING AT line", 24, line)?;
            if x2 < x || y2 < y {
                return err(
                    line,
                    format!("ENDING AT {x2} {y2} is before STARTING AT {x} {y}"),
                );
            }
            (x2, y2)
        } else {
            (0, 0)
        };
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Dynpro(n),
            what: MODAL_CALL,
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::CallModal {
                no: n,
                x,
                y,
                x2,
                y2,
            },
            line,
        });
        Ok(())
    }

    /// `SET SCREEN n.` (n = 0 ist Phase 2)
    fn set_screen(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if toks.len() != 3 {
            return err(
                line,
                "Unsupported form of SET SCREEN (expected: SET SCREEN <number>)",
            );
        }
        let n = dynpro_number_or_zero(&toks[2], line)?;
        if n == 0 {
            return err(line, "SET SCREEN 0 is not supported yet");
        }
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Dynpro(n),
            what: "SET SCREEN",
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::Set(n),
            line,
        });
        Ok(())
    }

    /// `LEAVE TO SCREEN n.` Entscheidung (Auftrag): `LEAVE TO SCREEN 0` ist `LEAVE SCREEN`.
    fn leave_to_screen(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if toks.len() != 4 {
            return err(
                line,
                "Unsupported form of LEAVE TO SCREEN (expected: LEAVE TO SCREEN <number>)",
            );
        }
        let n = dynpro_number_or_zero(&toks[3], line)?;
        if n == 0 {
            out.push(Stmt::Screen {
                op: ScreenOp::Leave,
                line,
            });
            return Ok(());
        }
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Dynpro(n),
            what: "LEAVE TO SCREEN",
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::LeaveTo(n),
            line,
        });
        Ok(())
    }

    /// `SET PF-STATUS 'name'.` Der Name wird in Großbuchstaben weitergegeben; `.dynp` und Laufzeit nutzen
    /// denselben Schlüssel (Entscheidung, Groß-/Kleinschreibung wird nicht unterschieden).
    fn set_pf_status(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if toks.len() > 3 {
            return err(
                line,
                "SET PF-STATUS with OF PROGRAM or EXCLUDING is not supported yet",
            );
        }
        let name = match toks.get(2) {
            Some(t) if t.kind == Kind::Literal => literal_content(&t.text),
            _ => None,
        };
        let Some(name) = name else {
            return err(line, "SET PF-STATUS expects a status name in single quotes");
        };
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Status(name.to_lowercase()),
            what: "SET PF-STATUS",
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::PfStatus(name.to_uppercase()),
            line,
        });
        Ok(())
    }

    /// `SET CURSOR FIELD f.` Nur der Feldname (Auftrag Phase 2). Ob das Feld auf dem aktuellen Dynpro liegt,
    /// prüft `check.rs` gegen die `.dynp`-Datei (Feld kommt in einem Dynpro der Datei vor).
    fn set_cursor(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if !toks.get(2).is_some_and(|t| t.is_word("FIELD")) {
            return err(
                line,
                "SET CURSOR with row and column is not supported yet (use SET CURSOR FIELD <name>)",
            );
        }
        if toks.len() != 4 {
            return err(
                line,
                "SET CURSOR FIELD with LINE or OFFSET is not supported yet (use SET CURSOR FIELD <name>)",
            );
        }
        let name = match toks.get(3) {
            Some(t) if t.kind == Kind::Word => word(t),
            _ => return err(line, "SET CURSOR FIELD expects a field name, not a literal"),
        };
        self.screen_refs.push(ScreenRef {
            target: ScreenTarget::Field(name.clone()),
            what: "SET CURSOR",
            line,
        });
        out.push(Stmt::Screen {
            op: ScreenOp::SetCursor(name.to_uppercase()),
            line,
        });
        Ok(())
    }

    /// `GET CURSOR FIELD f.` Ziel `f` ist eine Variable; ob es eine Zeichenvariable ist, prüft der Codegen.
    fn get_cursor(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if !toks.get(2).is_some_and(|t| t.is_word("FIELD")) {
            return err(
                line,
                "GET CURSOR with row, column or VALUE is not supported yet (use GET CURSOR FIELD <variable>)",
            );
        }
        if toks.len() != 4 || toks[3].kind != Kind::Word {
            return err(line, "GET CURSOR FIELD expects one variable as target");
        }
        out.push(Stmt::Screen {
            op: ScreenOp::GetCursor(Target::Var(word(&toks[3]))),
            line,
        });
        Ok(())
    }

    /// `SET TITLEBAR 'text'.` Nur ein Literal ohne `WITH`. Die Form mit Textsymbol-ID (Bezeichner oder
    /// Literal mit `WITH`) ist nicht unterstützt. Ein Literal ohne `WITH` gilt als Titeltext, auch wenn es
    /// wie eine Textsymbol-ID aussieht (Annahme, siehe Bericht).
    fn set_titlebar(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        if toks.len() > 3 {
            return err(
                line,
                "SET TITLEBAR with WITH, OF PROGRAM or further options is not supported yet (text symbol form)",
            );
        }
        match toks.get(2) {
            Some(t) if t.kind == Kind::Literal => {
                let Some(text) = literal_content(&t.text) else {
                    return err(line, "SET TITLEBAR expects a title in single quotes");
                };
                out.push(Stmt::Screen {
                    op: ScreenOp::SetTitlebar(text),
                    line,
                });
                Ok(())
            }
            _ => err(
                line,
                "SET TITLEBAR with a text symbol ID or variable is not supported yet (use a literal in single quotes)",
            ),
        }
    }

    /// `MESSAGE 'text' TYPE 'T'.` Typen S, I, W, E, A, X. Nur Großbuchstaben, Kleinbuchstaben sind ein Fehler.
    fn message(
        &mut self,
        toks: &[Token],
        line: usize,
        out: &mut Vec<Stmt>,
    ) -> Result<(), CompileError> {
        let text = literal_content(&toks[1].text).unwrap_or_default();
        let kind_tok = &toks[3];
        let kind_text = literal_content(&kind_tok.text).unwrap_or_else(|| kind_tok.text.clone());
        let kind = match kind_text.as_str() {
            "S" | "I" | "W" | "E" | "A" | "X" => kind_text.chars().next().unwrap_or(' '),
            _ => {
                return err(
                    line,
                    format!(
                        "Message type '{kind_text}' is not supported (use upper-case S, I, W, E, A or X)"
                    ),
                )
            }
        };
        out.push(Stmt::Screen {
            op: ScreenOp::Message { kind, text },
            line,
        });
        Ok(())
    }
}

/// Dynpro-Nummer 1 bis 9999 aus einem Literal-Token (`100` und `0100` sind gleich).
fn dynpro_number(t: &Token, line: usize) -> Result<u32, CompileError> {
    let n = dynpro_number_or_zero(t, line)?;
    if n == 0 {
        return err(line, "Dynpro number must be 1 to 9999, found '0'");
    }
    Ok(n)
}

/// Koordinate eines Modalfensters: ganze Zahl von 1 bis `max`. Eine Variable ist nicht unterstützt (Spec 12c).
fn window_value(t: &Token, what: &str, max: u32, line: usize) -> Result<u32, CompileError> {
    let Ok(value) = t.text.parse::<i64>() else {
        return err(
            line,
            format!(
                "{what} with a variable is not supported yet (use an integer literal from 1 to {max})"
            ),
        );
    };
    if (1..=i64::from(max)).contains(&value) {
        Ok(value as u32)
    } else {
        err(
            line,
            format!("{what} must be 1 to {max}, found '{}'", t.text),
        )
    }
}

/// Wie `dynpro_number`, lässt aber 0 zu (für `LEAVE TO SCREEN 0`).
fn dynpro_number_or_zero(t: &Token, line: usize) -> Result<u32, CompileError> {
    match t.text.parse::<u32>() {
        Ok(n) if t.text.chars().all(|c| c.is_ascii_digit()) && n <= 9999 => Ok(n),
        _ => err(
            line,
            format!("Dynpro number must be 1 to 9999, found '{}'", t.text),
        ),
    }
}

/// Dynpro-Typ einer Strukturkomponente aus `name TYPE typ` bzw. `name(len) TYPE typ` (Spec 4.7).
/// `LIKE` und andere Formen sind `Other`: der Typ ist dem Dynpro dann nicht bekannt.
pub(super) fn component_field_type(f: &Statement) -> FieldType {
    let toks = &f.tokens;
    let Some(p) = toks.iter().position(|t| t.is_word("TYPE")) else {
        return FieldType::Other;
    };
    let end = toks[p + 1..]
        .iter()
        .position(|t| t.is_punct("#"))
        .map_or(toks.len(), |e| p + 1 + e);
    field_type_of(&toks[p + 1..end])
}

/// Dynpro-Typ aus den Tokens nach `TYPE` (Spec 4.7). Alles, was das Dynpro nicht abbildet, ist `Other`.
pub(super) fn field_type_of(toks: &[Token]) -> FieldType {
    let number = |t: &Token| t.text.parse::<usize>().ok();
    match toks {
        [t] if t.is_word("c") => FieldType::Char(1),
        [t, l, n] if t.is_word("c") && l.is_word("LENGTH") => {
            number(n).map_or(FieldType::Other, FieldType::Char)
        }
        [t] if t.is_word("n") => FieldType::Numc(1),
        [t, l, n] if t.is_word("n") && l.is_word("LENGTH") => {
            number(n).map_or(FieldType::Other, FieldType::Numc)
        }
        [t] if t.is_word("d") => FieldType::Date,
        [t] if t.is_word("t") => FieldType::Time,
        [t] if t.is_word("i") => FieldType::Int,
        [t] if t.is_word("string") => FieldType::String,
        [t] if t.is_word("sy-ucomm") => FieldType::Char(UCOMM_LENGTH),
        [t] if t.is_word("sy-dynnr") => FieldType::Char(4),
        [t, l, n, d, m] if t.is_word("p") && l.is_word("LENGTH") && d.is_word("DECIMALS") => {
            match (number(n), number(m)) {
                (Some(bytes), Some(decimals)) if bytes >= 1 => {
                    FieldType::Packed { bytes, decimals }
                }
                _ => FieldType::Other,
            }
        }
        _ => FieldType::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lower::parse_source;

    fn stmts(src: &str) -> crate::lower::Ir {
        parse_source(src).expect("program translates")
    }

    fn screen_ops(ir: &crate::lower::Ir) -> Vec<ScreenOp> {
        ir.stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Screen { op, .. } => Some(op.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn call_screen_lowers_with_number_and_reference() {
        let ir = stmts("CALL SCREEN 0100.\n");
        assert_eq!(screen_ops(&ir), vec![ScreenOp::Call(100)]);
        assert_eq!(
            ir.screen_refs,
            vec![ScreenRef {
                target: ScreenTarget::Dynpro(100),
                what: "CALL SCREEN",
                line: 1
            }]
        );
    }

    #[test]
    fn call_screen_zero_is_rejected() {
        let errs = parse_source("CALL SCREEN 0.\n").unwrap_err();
        assert!(errs[0].message.contains("Dynpro number must be 1 to 9999"));
    }

    #[test]
    fn call_screen_starting_at_lowers_to_modal_call_with_corners() {
        let ir = stmts("CALL SCREEN 0200 STARTING AT 10 5 ENDING AT 60 15.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::CallModal {
                no: 200,
                x: 10,
                y: 5,
                x2: 60,
                y2: 15
            }]
        );
        assert_eq!(
            ir.screen_refs,
            vec![ScreenRef {
                target: ScreenTarget::Dynpro(200),
                what: "CALL SCREEN ... STARTING AT",
                line: 1
            }]
        );
    }

    #[test]
    fn call_screen_starting_at_without_ending_at_passes_zero_corner() {
        let ir = stmts("CALL SCREEN 200 STARTING AT 3 2.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::CallModal {
                no: 200,
                x: 3,
                y: 2,
                x2: 0,
                y2: 0
            }]
        );
    }

    #[test]
    fn call_screen_starting_at_with_variable_position_is_not_supported_yet() {
        let errs = parse_source("CALL SCREEN 200 STARTING AT zx 5.\n").unwrap_err();
        assert!(errs[0].message.contains("not supported yet"));
        let errs =
            parse_source("CALL SCREEN 200 STARTING AT 10 zy ENDING AT 60 15.\n").unwrap_err();
        assert!(errs[0].message.contains("not supported yet"));
    }

    #[test]
    fn call_screen_starting_at_outside_the_terminal_is_rejected() {
        let errs = parse_source("CALL SCREEN 200 STARTING AT 0 5.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("STARTING AT column must be 1 to 80, found '0'"));
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 25.\n").unwrap_err();
        assert!(errs[0].message.contains("STARTING AT line must be 1 to 24"));
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 5 ENDING AT 81 15.\n").unwrap_err();
        assert!(errs[0].message.contains("ENDING AT column must be 1 to 80"));
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 5 ENDING AT 60 25.\n").unwrap_err();
        assert!(errs[0].message.contains("ENDING AT line must be 1 to 24"));
    }

    #[test]
    fn call_screen_ending_at_before_starting_at_is_rejected() {
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 5 ENDING AT 9 15.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("ENDING AT 9 15 is before STARTING AT 10 5"));
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 5 ENDING AT 60 4.\n").unwrap_err();
        assert!(errs[0].message.contains("is before STARTING AT"));
    }

    #[test]
    fn call_screen_starting_at_wrong_shape_is_rejected() {
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10 5 ENDING 60 15.\n").unwrap_err();
        assert!(errs[0].message.contains("Unsupported form of CALL SCREEN"));
        let errs = parse_source("CALL SCREEN 200 STARTING AT 10.\n").unwrap_err();
        assert!(errs[0].message.contains("Unsupported form of CALL SCREEN"));
    }

    #[test]
    fn leave_to_screen_zero_is_leave_screen() {
        let ir = stmts("LEAVE TO SCREEN 0.\n");
        assert_eq!(screen_ops(&ir), vec![ScreenOp::Leave]);
        assert!(ir.screen_refs.is_empty());
    }

    #[test]
    fn leave_to_screen_number_is_recorded() {
        let ir = stmts("LEAVE TO SCREEN 200.\n");
        assert_eq!(screen_ops(&ir), vec![ScreenOp::LeaveTo(200)]);
        assert_eq!(ir.screen_refs[0].what, "LEAVE TO SCREEN");
    }

    #[test]
    fn leave_screen_and_leave_program_lower_to_their_ops() {
        let ir = stmts("LEAVE SCREEN.\nLEAVE PROGRAM.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::Leave, ScreenOp::LeaveProgram]
        );
    }

    #[test]
    fn set_pf_status_uses_uppercase_name_and_lowercase_reference() {
        let ir = stmts("SET PF-STATUS 'status_0100'.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::PfStatus("STATUS_0100".into())]
        );
        assert_eq!(
            ir.screen_refs[0].target,
            ScreenTarget::Status("status_0100".into())
        );
    }

    #[test]
    fn set_pf_status_with_exclusion_is_phase_2() {
        let errs = parse_source("SET PF-STATUS 'X' EXCLUDING fc.\n").unwrap_err();
        assert!(errs[0].message.contains("not supported yet"));
    }

    #[test]
    fn set_screen_lowers_and_rejects_zero() {
        let ir = stmts("SET SCREEN 0200.\n");
        assert_eq!(screen_ops(&ir), vec![ScreenOp::Set(200)]);
        let errs = parse_source("SET SCREEN 0.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("SET SCREEN 0 is not supported yet"));
    }

    #[test]
    fn message_with_literal_text_and_upper_case_type_lowers() {
        let ir = stmts("MESSAGE 'Customer saved' TYPE 'S'.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::Message {
                kind: 'S',
                text: "Customer saved".into()
            }]
        );
    }

    #[test]
    fn message_with_lower_case_type_is_rejected() {
        let errs = parse_source("MESSAGE 'x' TYPE 's'.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("Message type 's' is not supported"));
    }

    #[test]
    fn message_with_unknown_type_is_rejected() {
        let errs = parse_source("MESSAGE 'x' TYPE 'Q'.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("Message type 'Q' is not supported"));
    }

    #[test]
    fn data_declarations_record_dynpro_field_types() {
        let ir = stmts(
            "DATA a TYPE n LENGTH 4.\nDATA b TYPE d.\nDATA c TYPE p LENGTH 3 DECIMALS 2.\nDATA e TYPE string.\nDATA f TYPE sy-dynnr.\n",
        );
        assert_eq!(ir.field_types.get("a"), Some(&FieldType::Numc(4)));
        assert_eq!(ir.field_types.get("b"), Some(&FieldType::Date));
        assert_eq!(
            ir.field_types.get("c"),
            Some(&FieldType::Packed {
                bytes: 3,
                decimals: 2
            })
        );
        assert_eq!(ir.field_types.get("e"), Some(&FieldType::String));
        assert_eq!(ir.field_types.get("f"), Some(&FieldType::Char(4)));
    }

    #[test]
    fn module_blocks_are_collected_with_phase_and_lowercase_name() {
        let ir = stmts(
            "MODULE Status_0100 OUTPUT.\n  SET PF-STATUS 'st'.\nENDMODULE.\nMODULE user_command_0100 INPUT.\n  LEAVE SCREEN.\nENDMODULE.\n",
        );
        assert_eq!(ir.modules.len(), 2);
        assert_eq!(ir.modules[0].name, "status_0100");
        assert_eq!(ir.modules[0].phase, crate::dynpro::Phase::Pbo);
        assert_eq!(ir.modules[1].phase, crate::dynpro::Phase::Pai);
        assert_eq!(ir.modules[1].body.len(), 1);
    }

    #[test]
    fn set_cursor_field_lowers_to_upper_case_name_and_records_reference() {
        let ir = stmts("SET CURSOR FIELD zkunnr.\n");
        assert_eq!(screen_ops(&ir), vec![ScreenOp::SetCursor("ZKUNNR".into())]);
        assert_eq!(
            ir.screen_refs,
            vec![ScreenRef {
                target: ScreenTarget::Field("zkunnr".into()),
                what: "SET CURSOR",
                line: 1
            }]
        );
    }

    #[test]
    fn set_cursor_with_offset_or_line_is_not_supported_yet() {
        let errs = parse_source("SET CURSOR FIELD zkunnr OFFSET 2.\n").unwrap_err();
        assert!(errs[0].message.contains("OFFSET is not supported yet"));
        let errs = parse_source("SET CURSOR FIELD zkunnr LINE 2.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("LINE or OFFSET is not supported yet"));
    }

    #[test]
    fn set_cursor_row_and_column_is_not_supported_yet() {
        let errs = parse_source("SET CURSOR 2 10.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("row and column is not supported yet"));
    }

    #[test]
    fn set_cursor_with_literal_is_rejected() {
        let errs = parse_source("SET CURSOR FIELD 'ZKUNNR'.\n").unwrap_err();
        assert!(errs[0].message.contains("expects a field name"));
    }

    #[test]
    fn get_cursor_field_takes_variable_as_target() {
        // GET ist ein Ereignis: außerhalb eines Ereignisblocks ist `GET` als Anweisungsanfang nicht eindeutig
        let ir = stmts("START-OF-SELECTION.\nGET CURSOR FIELD zfeld.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::GetCursor(Target::Var("zfeld".into()))]
        );
        assert!(ir.screen_refs.is_empty());
    }

    #[test]
    fn get_cursor_with_literal_target_is_rejected() {
        let errs = parse_source("START-OF-SELECTION.\nGET CURSOR FIELD 'ZFELD'.\n").unwrap_err();
        assert!(errs[0].message.contains("expects one variable as target"));
    }

    #[test]
    fn get_cursor_without_field_is_not_supported_yet() {
        let errs = parse_source("START-OF-SELECTION.\nGET CURSOR LINE zline.\n").unwrap_err();
        assert!(errs[0].message.contains("not supported yet"));
    }

    #[test]
    fn set_titlebar_literal_lowers_to_text() {
        let ir = stmts("SET TITLEBAR 'Kunde anlegen'.\n");
        assert_eq!(
            screen_ops(&ir),
            vec![ScreenOp::SetTitlebar("Kunde anlegen".into())]
        );
    }

    #[test]
    fn set_titlebar_text_symbol_form_with_with_is_not_supported_yet() {
        let errs = parse_source("SET TITLEBAR 'T01' WITH zkunnr.\n").unwrap_err();
        assert!(errs[0].message.contains("not supported yet"));
    }

    #[test]
    fn set_titlebar_with_variable_is_not_supported_yet() {
        let errs = parse_source("SET TITLEBAR ztitel.\n").unwrap_err();
        assert!(errs[0]
            .message
            .contains("text symbol ID or variable is not supported yet"));
    }

    #[test]
    fn struct_components_record_dynpro_types_in_declaration_order() {
        let ir = stmts(
            "DATA zkunnr TYPE c LENGTH 10.\nDATA: BEGIN OF zkunde,\n  name1 TYPE c LENGTH 30,\n  nr TYPE n LENGTH 4,\n  dat TYPE d,\n  ref LIKE zkunnr,\nEND OF zkunde.\n",
        );
        let idx = ir
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Data {
                    name,
                    ty: crate::lower::Ty::Struct(i),
                    ..
                } if name == "zkunde" => Some(*i),
                _ => None,
            })
            .expect("global structure ZKUNDE");
        assert_eq!(
            ir.struct_field_types.get(&idx),
            Some(&vec![
                FieldType::Char(30),
                FieldType::Numc(4),
                FieldType::Date,
                FieldType::Other
            ])
        );
    }
}
