//! C-ABI der Dynpro-Laufzeit (Abschnitt 8.2). Die Prolog-Funktionen bauen Dynpros, GUI-Status und
//! Modul-Bindungen auf. Die übrigen Funktionen kommen aus ABAP-Anweisungen und aus Modulen.
//!
//! Fehler sind Laufzeitfehler (`fatal`), kein `panic!`. Jede Funktion hält den Zustand nur so lange gesperrt,
//! wie sie ihn braucht; `or_fatal` wird immer ohne Sperre aufgerufen.

use std::io::{self, Write};
use std::sync::Arc;

use super::model::{
    AbapType, Button, Dynpro, FKey, FKind, Field, Frame, GuiStatus, Message, Module, Text, VarPtr,
};
use super::value::display_width;
use super::{fatal, or_fatal, session, state, term, Nav, SyVars};

/// UTF-16-Zeichen aus Zeiger und Länge. Ungültige Einzelzeichen werden ersetzt.
///
/// # Safety
/// `ptr` zeigt auf `len` UTF-16-Einheiten, oder `len` ist 0.
unsafe fn wide_text(ptr: *const u16, len: u64) -> String {
    String::from_utf16_lossy(crate::units(ptr, len))
}

// ---- Prolog: Dynpro-Aufbau

/// Registriert `sy-ucomm` und `sy-dynnr` (Prolog).
///
/// # Safety
/// `ucomm` zeigt auf `ucomm_len` Zeichen, `dynnr` auf `dynnr_len` Zeichen des Programms. Die Speicher bleiben
/// bis zum Programmende gültig.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_screen_sy(
    ucomm: *mut u16,
    ucomm_len: u64,
    dynnr: *mut u16,
    dynnr_len: u64,
) {
    if ucomm.is_null() || dynnr.is_null() {
        fatal("SY-UCOMM or SY-DYNNR has no storage");
    }
    state().sy = Some(SyVars {
        ucomm: VarPtr(ucomm as *mut u8),
        ucomm_len: ucomm_len as usize,
        dynnr: VarPtr(dynnr as *mut u8),
        dynnr_len: dynnr_len as usize,
    });
}

/// Beginnt ein Dynpro. `kind`: 0 NORMAL, 1 MODAL.
///
/// # Safety
/// `title` zeigt auf `title_len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_screen_begin(
    no: u32,
    kind: u32,
    rows: u32,
    cols: u32,
    title: *const u16,
    title_len: u64,
) {
    let title = wide_text(title, title_len);
    or_fatal(begin_dynpro(no, kind, rows, cols, title));
}

fn begin_dynpro(no: u32, kind: u32, rows: u32, cols: u32, title: String) -> Result<(), String> {
    let mut st = state();
    if st.building_dynpro.is_some() {
        return Err(format!("DYNPRO {no:04} starts inside another DYNPRO"));
    }
    if st.dynpros.iter().any(|d| d.no == no) {
        return Err(format!("Dynpro {no:04} is defined twice"));
    }
    let modal = match kind {
        0 => false,
        1 => true,
        _ => return Err(format!("Dynpro {no:04}: unknown kind {kind}")),
    };
    if !(1..=20).contains(&rows) || !(1..=80).contains(&cols) {
        return Err(format!(
            "Dynpro {no:04}: size {rows}x{cols} is outside 20x80"
        ));
    }
    st.building_dynpro = Some(Dynpro::new(no, modal, rows, cols, title));
    Ok(())
}

/// Führt `f` mit dem Dynpro im Aufbau aus.
fn with_building(f: impl FnOnce(&mut Dynpro) -> Result<(), String>) -> Result<(), String> {
    let mut st = state();
    match st.building_dynpro.as_mut() {
        Some(dynpro) => f(dynpro),
        None => Err("screen element outside of a DYNPRO definition".to_string()),
    }
}

/// Text auf dem Dynpro. `attrs`: Attributbits (Abschnitt 8.2).
///
/// # Safety
/// `txt` zeigt auf `len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_screen_text(
    row: u32,
    col: u32,
    txt: *const u16,
    len: u64,
    attrs: u32,
) {
    let text = wide_text(txt, len);
    or_fatal(with_building(|d| {
        if !d.contains_rect(row, col, text.chars().count()) {
            return Err(format!("text '{text}' is outside dynpro {}", d.label()));
        }
        d.texts.push(Text {
            row,
            col,
            text,
            attrs,
        });
        Ok(())
    }));
}

/// Eingabe- oder Anzeigefeld, gebunden an die Adresse `data` der ABAP-Variablen.
/// `abap_type`: 0 c, 1 n, 2 d, 3 t, 4 i, 5 p. `length`: Zeichen bei c, n, d, t; Bytes bei p. Bei i ist `length`
/// ohne Bedeutung (4 Bytes), bei d 8, bei t 6. `decimals` gilt nur für p.
///
/// # Safety
/// `name` zeigt auf `name_len` UTF-16-Einheiten. `data` zeigt auf den Speicher der Variablen (siehe `value::read`).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn abap_rt_screen_field(
    row: u32,
    col: u32,
    name: *const u16,
    name_len: u64,
    data: *mut u8,
    abap_type: u32,
    length: u32,
    decimals: u32,
    attrs: u32,
) {
    let name = wide_text(name, name_len);
    or_fatal(add_field(
        row,
        col,
        name,
        data,
        abap_type,
        length as usize,
        decimals as usize,
        attrs,
    ));
}

#[allow(clippy::too_many_arguments)]
fn add_field(
    row: u32,
    col: u32,
    name: String,
    data: *mut u8,
    abap_type: u32,
    length: usize,
    decimals: usize,
    attrs: u32,
) -> Result<(), String> {
    let ty = AbapType::from_code(abap_type)
        .ok_or_else(|| format!("field {name}: unknown type code {abap_type}"))?;
    if data.is_null() {
        return Err(format!("field {name} has no variable"));
    }
    let (length, decimals) =
        normalize_length(ty, length, decimals).map_err(|e| format!("field {name}: {e}"))?;
    let width = display_width(ty, length, decimals);
    with_building(|d| {
        if !d.contains_rect(row, col, width) {
            return Err(format!("field {name} is outside dynpro {}", d.label()));
        }
        if d.field_index(&name).is_some() {
            return Err(format!(
                "field {name} is defined twice in dynpro {}",
                d.label()
            ));
        }
        d.fields.push(Field {
            name,
            row,
            col,
            ty,
            length,
            decimals,
            attrs,
            data: VarPtr(data),
            width,
        });
        Ok(())
    })
}

/// Länge und Nachkommastellen je Typ prüfen. Ungültige Angaben sind ein Fehler des Compilers.
fn normalize_length(
    ty: AbapType,
    length: usize,
    decimals: usize,
) -> Result<(usize, usize), String> {
    match ty {
        AbapType::Char | AbapType::Num => {
            if length == 0 {
                return Err("length must be at least 1".to_string());
            }
            Ok((length, 0))
        }
        AbapType::Date => Ok((8, 0)),
        AbapType::Time => Ok((6, 0)),
        AbapType::Int => Ok((4, 0)),
        AbapType::Packed => {
            if length == 0 {
                return Err("packed length must be at least 1 byte".to_string());
            }
            if decimals > 2 * length - 1 {
                return Err(format!(
                    "{decimals} decimals do not fit into {length} packed bytes"
                ));
            }
            Ok((length, decimals))
        }
    }
}

/// Drucktaste mit Funktionscode. Die Attribute gelten in Phase 1 nicht für Tasten.
///
/// # Safety
/// `txt` zeigt auf `len` UTF-16-Einheiten, `code` auf `code_len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_screen_button(
    row: u32,
    col: u32,
    txt: *const u16,
    len: u64,
    code: *const u16,
    code_len: u64,
    _attrs: u32,
) {
    let text = wide_text(txt, len);
    let fcode = wide_text(code, code_len);
    or_fatal(with_building(|d| {
        let button = Button {
            row,
            col,
            text,
            fcode,
        };
        if !d.contains_rect(row, col, button.label().chars().count()) {
            return Err(format!(
                "button {} is outside dynpro {}",
                button.text,
                d.label()
            ));
        }
        d.buttons.push(button);
        Ok(())
    }));
}

/// Rahmen von (`row1`, `col1`) bis (`row2`, `col2`), beide Ecken eingeschlossen.
///
/// # Safety
/// `title` zeigt auf `title_len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_screen_frame(
    row1: u32,
    col1: u32,
    row2: u32,
    col2: u32,
    title: *const u16,
    title_len: u64,
) {
    let title = wide_text(title, title_len);
    or_fatal(with_building(|d| {
        if row1 > row2
            || col1 > col2
            || !d.contains_rect(row1, col1, 1)
            || !d.contains_rect(row2, col2, 1)
        {
            return Err(format!(
                "frame {row1},{col1} to {row2},{col2} is outside dynpro {}",
                d.label()
            ));
        }
        d.frames.push(Frame {
            row1,
            col1,
            row2,
            col2,
            title,
        });
        Ok(())
    }));
}

/// Bindet ein Modul an das offene Dynpro. `phase`: 0 PBO, 1 PAI. `at_exit`: `AT EXIT-COMMAND`.
#[no_mangle]
pub extern "C" fn abap_rt_screen_module(no: u32, phase: u32, func: extern "C" fn(), at_exit: u32) {
    or_fatal(with_building(|d| {
        if d.no != no {
            return Err(format!(
                "module for dynpro {no:04} inside dynpro {}",
                d.label()
            ));
        }
        let module = Module {
            func,
            at_exit: at_exit != 0,
        };
        match phase {
            0 => d.pbo.push(module),
            1 => d.pai.push(module),
            _ => return Err(format!("unknown module phase {phase}")),
        }
        Ok(())
    }));
}

/// Schließt das Dynpro ab und registriert es.
#[no_mangle]
pub extern "C" fn abap_rt_screen_end(no: u32) {
    or_fatal(end_dynpro(no));
}

fn end_dynpro(no: u32) -> Result<(), String> {
    let mut st = state();
    let Some(dynpro) = st.building_dynpro.take() else {
        return Err(format!("DYNPRO {no:04} ended without a start"));
    };
    if dynpro.no != no {
        return Err(format!("DYNPRO {:04} ended as {no:04}", dynpro.no));
    }
    st.dynpros.push(Arc::new(dynpro));
    Ok(())
}

// ---- Prolog: GUI-Status

/// Beginnt einen GUI-Status.
///
/// # Safety
/// `name` zeigt auf `name_len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_status_begin(name: *const u16, name_len: u64) {
    let name = wide_text(name, name_len).trim_end().to_ascii_uppercase();
    or_fatal(begin_status(name));
}

fn begin_status(name: String) -> Result<(), String> {
    let mut st = state();
    if st.building_status.is_some() {
        return Err(format!("GUISTATUS {name} starts inside another GUISTATUS"));
    }
    if st.statuses.iter().any(|s| s.name == name) {
        return Err(format!("GUI status '{name}' is defined twice"));
    }
    st.building_status = Some(GuiStatus {
        name,
        fkeys: Vec::new(),
    });
    Ok(())
}

/// Funktionstaste. `key` 1 bis 24, `kind` 0 keine, 1 B, 2 C, 3 E.
///
/// # Safety
/// `txt` zeigt auf `len`, `code` auf `code_len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_status_fkey(
    key: u32,
    txt: *const u16,
    len: u64,
    code: *const u16,
    code_len: u64,
    kind: u32,
) {
    let text = wide_text(txt, len);
    let fcode = wide_text(code, code_len);
    or_fatal(add_fkey(key, text, fcode, kind));
}

fn add_fkey(key: u32, text: String, fcode: String, kind: u32) -> Result<(), String> {
    if !(1..=24).contains(&key) {
        return Err(format!("function key F{key} does not exist"));
    }
    let kind = FKind::from_code(kind).ok_or_else(|| format!("unknown FKEY type code {kind}"))?;
    let mut st = state();
    let Some(status) = st.building_status.as_mut() else {
        return Err("FKEY outside of a GUISTATUS".to_string());
    };
    if status.fkeys.iter().any(|k| k.key == key) {
        return Err(format!(
            "function key F{key} is defined twice in {}",
            status.name
        ));
    }
    status.fkeys.push(FKey {
        key,
        text,
        fcode,
        kind,
    });
    Ok(())
}

/// Schließt den GUI-Status ab.
#[no_mangle]
pub extern "C" fn abap_rt_status_end() {
    or_fatal(end_status());
}

fn end_status() -> Result<(), String> {
    let mut st = state();
    let Some(status) = st.building_status.take() else {
        return Err("GUISTATUS ended without a start".to_string());
    };
    st.statuses.push(Arc::new(status));
    Ok(())
}

// ---- Ausführung: Aufrufe aus ABAP-Anweisungen und Modulen

/// `CALL SCREEN n`.
#[no_mangle]
pub extern "C" fn abap_rt_call_screen(no: u32) {
    or_fatal(session::call_screen(no));
}

/// Setzt eine Navigationsaktion im laufenden Dynpro. Außerhalb eines Dynpros ohne Wirkung.
fn set_nav(nav: Nav) {
    let mut st = state();
    if !st.stack.is_empty() {
        st.pending_nav = Some(nav);
    }
}

/// `SET SCREEN n`.
#[no_mangle]
pub extern "C" fn abap_rt_set_screen(no: u32) {
    set_nav(Nav::SetScreen(no));
}

/// `LEAVE SCREEN`.
#[no_mangle]
pub extern "C" fn abap_rt_leave_screen() {
    set_nav(Nav::Leave);
}

/// `LEAVE TO SCREEN n`. `n` = 0 wie `LEAVE SCREEN` (Entscheidung).
#[no_mangle]
pub extern "C" fn abap_rt_leave_to_screen(no: u32) {
    set_nav(if no == 0 {
        Nav::Leave
    } else {
        Nav::LeaveTo(no)
    });
}

/// `LEAVE PROGRAM`: beendet das Programm sofort mit Status 0. Die Liste wird abgeschlossen, das Terminal wiederhergestellt.
#[no_mangle]
pub extern "C" fn abap_rt_leave_program() {
    term::restore();
    crate::abap_rt_finish();
    let _ = io::stdout().flush();
    std::process::exit(0);
}

fn set_status(name: &str) -> Result<(), String> {
    let mut st = state();
    if st.stack.is_empty() {
        // Außerhalb von Dynpros hat `SET PF-STATUS` in Phase 1 keine Wirkung (Annahme, siehe Bericht).
        return Ok(());
    }
    let Some(status) = st.statuses.iter().find(|s| s.name == name).cloned() else {
        return Err(format!("GUI status '{name}' is not defined"));
    };
    st.pending_status = Some(status);
    Ok(())
}

/// `SET PF-STATUS name`.
///
/// # Safety
/// `name` zeigt auf `len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_set_pf_status(name: *const u16, len: u64) {
    let name = wide_text(name, len).trim_end().to_ascii_uppercase();
    or_fatal(set_status(&name));
}

/// `MESSAGE txt TYPE t`. `kind` ist der ASCII-Code des Typs: S, I, W, E, A oder X.
/// Typ A und X beenden das Programm mit Fehler. Die übrigen Typen erscheinen in Zeile 24 des Dynpros.
///
/// # Safety
/// `txt` zeigt auf `len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_message(kind: u32, txt: *const u16, len: u64) {
    let text = wide_text(txt, len);
    let kind = match u8::try_from(kind).map(char::from) {
        Ok(c @ ('S' | 'I' | 'W' | 'E' | 'A' | 'X')) => c,
        _ => fatal(&format!("unknown message type code {kind}")),
    };
    if matches!(kind, 'A' | 'X') {
        fatal(&format!(
            "program aborted by message of type '{kind}': {text}"
        ));
    }
    let mut st = state();
    if st.stack.is_empty() {
        drop(st);
        eprintln!("Message (type {kind}): {text}");
        return;
    }
    st.message = Some(Message { kind, text });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::headless::Headless;
    use crate::screen::model::{FKind, INPUT};
    use crate::screen::test_support::exclusive;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static PAI_CALLS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn pbo_set_status() {
        let name = utf16("ZSTATUS");
        unsafe { abap_rt_set_pf_status(name.as_ptr(), name.len() as u64) };
    }

    extern "C" fn pai_leave() {
        PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_leave_screen();
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// Der Prolog wird ausschließlich über die C-ABI aufgebaut. Danach läuft das Dynpro über das Skript.
    #[test]
    fn prolog_through_abi_builds_dynpro_status_and_bindings_then_runs() {
        let _guard = exclusive();
        PAI_CALLS.store(0, Ordering::SeqCst);
        let mut ucomm = vec![0x20u16; 70];
        let mut dynnr = vec![0x20u16; 4];
        let mut var = vec![0x41u16, 0x42, 0x20, 0x20];

        unsafe {
            abap_rt_screen_sy(ucomm.as_mut_ptr(), 70, dynnr.as_mut_ptr(), 4);
            let title = utf16("Kunde");
            abap_rt_screen_begin(100, 0, 20, 80, title.as_ptr(), title.len() as u64);
            let text = utf16("Ort");
            abap_rt_screen_text(3, 2, text.as_ptr(), 3, 0);
            let name = utf16("ZORT");
            abap_rt_screen_field(
                3,
                20,
                name.as_ptr(),
                4,
                var.as_mut_ptr() as *mut u8,
                0,
                4,
                0,
                INPUT,
            );
            let label = utf16("Sichern");
            let code = utf16("SAVE");
            abap_rt_screen_button(5, 2, label.as_ptr(), 7, code.as_ptr(), 4, 0);
            let frame_title = utf16("");
            abap_rt_screen_frame(1, 1, 6, 40, frame_title.as_ptr(), 0);
            abap_rt_screen_module(100, 0, pbo_set_status, 0);
            abap_rt_screen_module(100, 1, pai_leave, 0);
            abap_rt_screen_end(100);

            let status = utf16("zstatus");
            abap_rt_status_begin(status.as_ptr(), status.len() as u64);
            let key = utf16("Back");
            let fcode = utf16("BACK");
            abap_rt_status_fkey(3, key.as_ptr(), 4, fcode.as_ptr(), 4, 1);
            abap_rt_status_end();
        }

        {
            let st = state();
            assert_eq!(st.dynpros.len(), 1);
            let d = &st.dynpros[0];
            assert_eq!(d.title, "Kunde");
            assert_eq!((d.rows, d.cols, d.modal), (20, 80, false));
            assert_eq!(d.texts.len(), 1);
            assert_eq!(d.fields.len(), 1);
            assert_eq!(d.fields[0].name, "ZORT");
            assert_eq!(d.fields[0].width, 4);
            assert_eq!(d.buttons[0].fcode, "SAVE");
            assert_eq!(d.frames.len(), 1);
            assert_eq!((d.pbo.len(), d.pai.len()), (1, 1));
            assert_eq!(st.statuses[0].name, "ZSTATUS");
            assert_eq!(st.statuses[0].fkeys[0].kind, FKind::Back);
        }

        state().terminal = Some(Box::new(
            Headless::from_script("KEY F3\n", Box::new(std::io::sink())).unwrap(),
        ));
        session::call_screen(100).unwrap();
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
        assert!(state().stack.is_empty());
    }

    #[test]
    fn leave_outside_a_screen_has_no_effect() {
        let _guard = exclusive();
        abap_rt_leave_screen();
        abap_rt_set_screen(200);
        assert_eq!(state().pending_nav, None);
    }
}
