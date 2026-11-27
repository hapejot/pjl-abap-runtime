//! Schleife je Dynpro (Abschnitt 6.1): PBO, Anzeige, Eingabe, Übertragung, PAI. Aktionen aus Modulen
//! (`SET SCREEN`, `LEAVE`, `MESSAGE`, `SET PF-STATUS`) werden gespeichert und nach dem Modul ausgewertet.
//!
//! Der Zustand liegt im globalen `State` (Mutex). Vor jedem Aufruf eines Moduls wird der Sperrzustand
//! freigegeben, weil Module wieder Laufzeitfunktionen aufrufen (verschachtelter `CALL SCREEN`).

use std::sync::Arc;

use super::grid::{cursor_cell, render};
use super::input::{self, Editor, Fault, Outcome, Pos, Transfer};
use super::model::{Dynpro, FKind, GuiStatus, Message, Module};
use super::terminal::Event;
use super::value;
use super::{open_terminal, state, Nav};

/// Wie die Schleife eines Dynpros endet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    Leave,
    LeaveTo(u32),
}

/// Ergebnis einer Ablaufphase: Navigationsaktion und gesetzter GUI-Status.
struct Phase {
    nav: Option<Nav>,
    status: Option<Arc<GuiStatus>>,
}

/// Zustand einer laufenden Schleife (ein Aufruf von `CALL SCREEN`).
struct Run {
    dynpro: Arc<Dynpro>,
    status: Option<Arc<GuiStatus>>,
    editor: Editor,
    /// Ist vor der nächsten Anzeige ein PBO-Lauf nötig?
    need_pbo: bool,
}

/// `CALL SCREEN n`: Führt das Dynpro aus, bis es verlassen wird.
pub fn call_screen(no: u32) -> Result<(), String> {
    let dynpro = find_dynpro(no)?;
    let (outermost, saved_nav, saved_status) = {
        let mut guard = state();
        let st = &mut *guard;
        if st.sy.is_none() {
            return Err("SY-UCOMM and SY-DYNNR are not registered".to_string());
        }
        if st.terminal.is_none() {
            st.terminal = Some(open_terminal()?);
        }
        let outermost = st.stack.is_empty();
        if outermost {
            if let Some(terminal) = st.terminal.as_mut() {
                terminal.begin()?;
            }
        }
        st.stack.push(no);
        (outermost, st.pending_nav.take(), st.pending_status.take())
    };

    let mut run = Run {
        dynpro,
        status: None,
        editor: Editor::new(),
        need_pbo: true,
    };
    let result = run_loop(&mut run);

    {
        let mut guard = state();
        let st = &mut *guard;
        st.stack.pop();
        // Aktionen des aufrufenden Dynpros bleiben erhalten; die Schleife hat ihre eigenen verbraucht.
        st.pending_nav = saved_nav;
        st.pending_status = saved_status;
        if outermost {
            if let Some(terminal) = st.terminal.as_mut() {
                terminal.end();
            }
        }
    }

    match result? {
        Exit::Leave => Ok(()),
        Exit::LeaveTo(no) => {
            // LEAVE TO SCREEN n: Das aufrufende Dynpro zeigt n als nächstes (Annahme, siehe Bericht).
            let mut guard = state();
            if guard.stack.is_empty() {
                return Err(format!(
                    "LEAVE TO SCREEN {no:04} without a calling screen is not supported"
                ));
            }
            guard.pending_nav = Some(Nav::SetScreen(no));
            Ok(())
        }
    }
}

fn find_dynpro(no: u32) -> Result<Arc<Dynpro>, String> {
    state()
        .dynpros
        .iter()
        .find(|d| d.no == no)
        .cloned()
        .ok_or_else(|| format!("Dynpro {no:04} is not defined"))
}

fn run_loop(run: &mut Run) -> Result<Exit, String> {
    loop {
        if run.need_pbo {
            set_dynnr(run.dynpro.no)?;
            let phase = run_phase(&run.dynpro.pbo, false)?;
            if let Some(status) = phase.status {
                run.status = Some(status);
            }
            match phase.nav {
                Some(Nav::Leave) => return Ok(Exit::Leave),
                Some(Nav::LeaveTo(no)) => return Ok(Exit::LeaveTo(no)),
                Some(Nav::SetScreen(no)) => {
                    switch_to(run, no)?;
                    continue;
                }
                None => {}
            }
            run.editor.reload(&run.dynpro).map_err(String::from)?;
            run.need_pbo = false;
        }

        draw(run)?;
        match next_event()? {
            Event::Eof => {
                return Err(format!(
                    "screen input script ended while dynpro {} waits for input",
                    run.dynpro.label()
                ))
            }
            Event::Field(name) => run.editor.goto(&run.dynpro, &name)?,
            Event::Key(key) => match run.editor.handle(&run.dynpro, run.status.as_deref(), key) {
                Outcome::Nothing => {}
                Outcome::Interrupt => return Err("interrupted by user".to_string()),
                Outcome::Transfer { fcode, kind } => {
                    if let Some(exit) = transfer(run, &fcode, kind)? {
                        return Ok(exit);
                    }
                }
            },
        }
    }
}

/// Übertragung zum PAI (Abschnitt 6.4). Liefert `Some`, wenn die Schleife endet.
fn transfer(run: &mut Run, fcode: &str, kind: FKind) -> Result<Option<Exit>, String> {
    // Eine Meldung bleibt bis zur nächsten Übertragung stehen (Abschnitt 6.5).
    state().message = None;
    let exit_kind = kind.is_exit();
    let values = if exit_kind {
        run.editor.check_lenient(&run.dynpro)
    } else {
        match run.editor.check(&run.dynpro) {
            Ok(values) => values,
            Err(fault) => {
                report_fault(run, fault);
                return Ok(None);
            }
        }
    };
    write_values(&run.dynpro, &values)?;
    set_dynnr(run.dynpro.no)?;
    set_ucomm(fcode)?;

    let phase = run_phase(&run.dynpro.pai, exit_kind)?;
    if let Some(status) = phase.status {
        run.status = Some(status);
    }
    // Nach dem PAI: PBO des gleichen Dynpros (Abschnitt 6.1), außer die Aktion verlangt etwas anderes.
    run.need_pbo = true;
    match phase.nav {
        Some(Nav::Leave) => Ok(Some(Exit::Leave)),
        Some(Nav::LeaveTo(no)) => Ok(Some(Exit::LeaveTo(no))),
        Some(Nav::SetScreen(no)) => {
            switch_to(run, no)?;
            Ok(None)
        }
        None => Ok(None),
    }
}

/// Fehler bei der Prüfung: Meldung E, Cursor auf das Feld, kein PAI (Abschnitt 6.4).
fn report_fault(run: &mut Run, fault: Fault) {
    let field = &run.dynpro.fields[fault.index];
    state().message = Some(Message {
        kind: 'E',
        text: input::fault_text(field, fault.error),
    });
    run.editor.focus(Pos {
        row: field.row,
        col: field.col,
    });
}

fn switch_to(run: &mut Run, no: u32) -> Result<(), String> {
    run.dynpro = find_dynpro(no)?;
    run.editor = Editor::new();
    run.need_pbo = true;
    Ok(())
}

/// Führt die Module einer Phase aus. Nach jedem Modul wird die Aktion ausgewertet: `LEAVE` beendet die Phase.
/// Mehrere `SET SCREEN` im selben Vorgang: die letzte Aktion gilt (Annahme, siehe Bericht).
/// Module mit `AT EXIT-COMMAND` laufen nur bei Funktionscodes vom Typ B, C oder E.
fn run_phase(modules: &[Module], exit_kind: bool) -> Result<Phase, String> {
    state().pending_nav = None;
    for module in modules {
        if module.at_exit && !exit_kind {
            continue;
        }
        (module.func)();
        let nav = state().pending_nav;
        if matches!(nav, Some(Nav::Leave | Nav::LeaveTo(_))) {
            break;
        }
    }
    let mut guard = state();
    let st = &mut *guard;
    Ok(Phase {
        nav: st.pending_nav.take(),
        status: st.pending_status.take(),
    })
}

fn draw(run: &Run) -> Result<(), String> {
    let mut guard = state();
    let st = &mut *guard;
    let grid = render(
        &run.dynpro,
        run.editor.bufs(),
        run.status.as_deref(),
        st.message.as_ref(),
    );
    let pos = run.editor.cursor(&run.dynpro);
    let cursor = cursor_cell(pos.row, pos.col);
    let terminal = st
        .terminal
        .as_mut()
        .ok_or_else(|| "no terminal open for dynpro".to_string())?;
    terminal.draw(&grid, cursor)
}

fn next_event() -> Result<Event, String> {
    let mut guard = state();
    let terminal = guard
        .terminal
        .as_mut()
        .ok_or_else(|| "no terminal open for dynpro".to_string())?;
    terminal.next_event()
}

/// Schreibt die geprüften Werte in die ABAP-Variablen.
fn write_values(dynpro: &Dynpro, values: &[Transfer]) -> Result<(), String> {
    for transfer in values {
        let field = &dynpro.fields[transfer.index];
        // SAFETY: `field.data` zeigt auf `mem_len` Bytes der Programmvariablen (Vertrag des Prologs).
        unsafe { value::write(field, &transfer.value) }
            .map_err(|e| format!("field {}: {e}", field.name))?;
    }
    Ok(())
}

/// Schreibt `sy-dynnr` (vier Stellen).
fn set_dynnr(no: u32) -> Result<(), String> {
    let sy = state()
        .sy
        .ok_or_else(|| "SY-UCOMM and SY-DYNNR are not registered".to_string())?;
    let units: Vec<u16> = format!("{no:04}").encode_utf16().collect();
    // SAFETY: `sy.dynnr` zeigt auf `sy.dynnr_len` Zeichen der Programmvariablen (Vertrag von `abap_rt_screen_sy`).
    unsafe { value::write_units(sy.dynnr.0, sy.dynnr_len, &units) };
    Ok(())
}

/// Schreibt `sy-ucomm` mit dem Funktionscode.
fn set_ucomm(fcode: &str) -> Result<(), String> {
    let sy = state()
        .sy
        .ok_or_else(|| "SY-UCOMM and SY-DYNNR are not registered".to_string())?;
    let units: Vec<u16> = fcode.encode_utf16().collect();
    // SAFETY: `sy.ucomm` zeigt auf `sy.ucomm_len` Zeichen der Programmvariablen (Vertrag von `abap_rt_screen_sy`).
    unsafe { value::write_units(sy.ucomm.0, sy.ucomm_len, &units) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::abi::{
        abap_rt_call_screen, abap_rt_leave_screen, abap_rt_leave_to_screen, abap_rt_message,
        abap_rt_set_pf_status, abap_rt_set_screen,
    };
    use crate::screen::headless::Headless;
    use crate::screen::model::{
        AbapType, FKey, FKind, Field, Message, Module, VarPtr, INPUT, OUTPUT, REQUIRED,
    };
    use crate::screen::test_support::exclusive;
    use crate::screen::value::display_width;
    use crate::screen::SyVars;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    static PBO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PAI_CALLS: AtomicUsize = AtomicUsize::new(0);
    static NORMAL_CALLS: AtomicUsize = AtomicUsize::new(0);
    static EXIT_CALLS: AtomicUsize = AtomicUsize::new(0);
    static Z_TARGET: AtomicUsize = AtomicUsize::new(0);

    /// Ausgabe des Headless-Terminals, die der Test danach liest.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Testumgebung: exklusiver Zustand, Headless-Terminal mit Skript, sy-Felder.
    struct Harness {
        _guard: MutexGuard<'static, ()>,
        _ucomm: Box<[u16]>,
        _dynnr: Box<[u16]>,
        out: Shared,
    }

    impl Harness {
        fn new(script: &str) -> Harness {
            let guard = exclusive();
            PBO_CALLS.store(0, Ordering::SeqCst);
            PAI_CALLS.store(0, Ordering::SeqCst);
            NORMAL_CALLS.store(0, Ordering::SeqCst);
            EXIT_CALLS.store(0, Ordering::SeqCst);
            let mut ucomm = vec![0x20u16; 70].into_boxed_slice();
            let mut dynnr = vec![0x20u16; 4].into_boxed_slice();
            state().sy = Some(SyVars {
                ucomm: VarPtr(ucomm.as_mut_ptr() as *mut u8),
                ucomm_len: 70,
                dynnr: VarPtr(dynnr.as_mut_ptr() as *mut u8),
                dynnr_len: 4,
            });
            let out = Shared::default();
            state().terminal = Some(Box::new(
                Headless::from_script(script, Box::new(out.clone())).unwrap(),
            ));
            Harness {
                _guard: guard,
                _ucomm: ucomm,
                _dynnr: dynnr,
                out,
            }
        }

        fn install(&self, dynpro: Dynpro) {
            state().dynpros.push(Arc::new(dynpro));
        }

        /// Gitterabzüge: je Abzug die 24 Zeilen.
        fn screens(&self) -> Vec<Vec<String>> {
            let text = String::from_utf8(self.out.0.lock().unwrap().clone()).unwrap();
            text.split("----\n")
                .filter(|block| !block.is_empty())
                .map(|block| block.lines().map(String::from).collect())
                .collect()
        }

        fn message(&self) -> Option<Message> {
            state().message.clone()
        }
    }

    /// Zeichen als UTF-16-Bytes (Speicherform eines `TYPE c`-Feldes).
    fn chars_mem(text: &str, len: usize) -> Box<[u8]> {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.resize(len, 0x20);
        units
            .iter()
            .flat_map(|u| u.to_ne_bytes())
            .collect::<Vec<u8>>()
            .into_boxed_slice()
    }

    fn char_field(name: &str, row: u32, col: u32, len: usize, attrs: u32, mem: &mut [u8]) -> Field {
        Field {
            name: name.into(),
            row,
            col,
            ty: AbapType::Char,
            length: len,
            decimals: 0,
            attrs,
            data: VarPtr(mem.as_mut_ptr()),
            width: display_width(AbapType::Char, len, 0),
        }
    }

    fn dynpro(no: u32, fields: Vec<Field>, pbo: Vec<Module>, pai: Vec<Module>) -> Dynpro {
        let mut d = Dynpro::new(no, false, 20, 80, "Test".into());
        d.fields = fields;
        d.pbo = pbo;
        d.pai = pai;
        d
    }

    fn module(func: extern "C" fn(), at_exit: bool) -> Module {
        Module { func, at_exit }
    }

    fn install_status() {
        state().statuses.push(Arc::new(GuiStatus {
            name: "STATUS".into(),
            fkeys: vec![FKey {
                key: 3,
                text: "Back".into(),
                fcode: "BACK".into(),
                kind: FKind::Back,
            }],
        }));
    }

    /// PBO-Modul: setzt den GUI-Status `STATUS`.
    extern "C" fn pbo_status() {
        PBO_CALLS.fetch_add(1, Ordering::SeqCst);
        let name: Vec<u16> = "STATUS".encode_utf16().collect();
        unsafe { abap_rt_set_pf_status(name.as_ptr(), name.len() as u64) };
    }

    extern "C" fn pbo_count() {
        PBO_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    /// PBO-Modul: schreibt 'Z' an den Anfang des Feldes, dessen Adresse in `Z_TARGET` steht.
    extern "C" fn pbo_write_z() {
        PBO_CALLS.fetch_add(1, Ordering::SeqCst);
        unsafe { *(Z_TARGET.load(Ordering::SeqCst) as *mut u16) = 'Z' as u16 };
    }

    extern "C" fn pai_leave() {
        PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_leave_screen();
    }

    extern "C" fn pai_set_200() {
        PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_set_screen(200);
    }

    extern "C" fn pai_call_200() {
        PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_call_screen(200);
    }

    extern "C" fn pai_leave_to_300() {
        PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_leave_to_screen(300);
    }

    extern "C" fn pai_normal() {
        NORMAL_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C" fn pai_exit_leave() {
        EXIT_CALLS.fetch_add(1, Ordering::SeqCst);
        abap_rt_leave_screen();
    }

    /// PAI-Modul: beim ersten Aufruf eine Meldung E, danach verlassen.
    extern "C" fn pai_message_then_leave() {
        let n = PAI_CALLS.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            let text: Vec<u16> = "Wrong value".encode_utf16().collect();
            unsafe { abap_rt_message('E' as u32, text.as_ptr(), text.len() as u64) };
        } else {
            abap_rt_leave_screen();
        }
    }

    fn call(no: u32) -> Result<(), String> {
        call_screen(no)
    }

    #[test]
    fn enter_transfers_typed_value_and_leave_ends_the_call() {
        let h = Harness::new("FIELD ZKUNNR\nTYPE '4711'\nSNAPSHOT\nKEY ENTER\n");
        let mut mem = chars_mem("", 4);
        let field = char_field("ZKUNNR", 2, 20, 4, INPUT, &mut mem);
        let mut d = dynpro(
            100,
            vec![field],
            vec![module(pbo_count, false)],
            vec![module(pai_leave, false)],
        );
        d.texts.push(text_at("Kundennummer", 2, 2));
        h.install(d);

        call(100).unwrap();

        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(&mem[..], &chars_mem("4711", 4)[..]);
        let screens = h.screens();
        assert_eq!(screens.len(), 1);
        // Dynpro-Position (2, 2) liegt in Gitterzeile 2 ab Spalte 1.
        assert!(screens[0][2].starts_with(" Kundennummer"));
        assert_eq!(&screens[0][2][19..23], "4711");
        assert!(state().stack.is_empty());
    }

    /// Text auf dem Dynpro.
    fn text_at(text: &str, row: u32, col: u32) -> crate::screen::model::Text {
        crate::screen::model::Text {
            row,
            col,
            text: text.into(),
            attrs: 0,
        }
    }

    #[test]
    fn pbo_runs_before_display_and_shows_the_program_value() {
        let h = Harness::new("SNAPSHOT\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("ABCD", 4);
        Z_TARGET.store(mem.as_mut_ptr() as usize, Ordering::SeqCst);
        let field = char_field("ZFELD", 2, 2, 4, OUTPUT, &mut mem);
        h.install(dynpro(
            100,
            vec![field],
            vec![module(pbo_write_z, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        let screens = h.screens();
        assert_eq!(&screens[0][2][1..5], "ZBCD");
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn required_field_blocks_pai_and_reports_error_in_line_24() {
        let h = Harness::new("KEY ENTER\nSNAPSHOT\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("", 4);
        let mut req = char_field("ZKUNNR", 2, 2, 4, INPUT | REQUIRED, &mut mem);
        req.name = "ZKUNNR".into();
        h.install(dynpro(
            100,
            vec![req],
            vec![module(pbo_status, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        // Enter wurde abgewiesen, PAI lief erst bei F3 (Typ B, ohne Prüfung).
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
        let screens = h.screens();
        assert_eq!(screens[0][23].trim_end(), "Field ZKUNNR is required");
    }

    #[test]
    fn invalid_date_is_reported_and_value_is_not_written() {
        let h = Harness::new("FIELD ZDATUM\nTYPE '20231301'\nKEY ENTER\nSNAPSHOT\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("00000000", 8);
        let mut field = char_field("ZDATUM", 2, 2, 8, INPUT, &mut mem);
        field.ty = AbapType::Date;
        field.width = 8;
        h.install(dynpro(
            100,
            vec![field],
            vec![module(pbo_status, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        let screens = h.screens();
        assert_eq!(screens[0][23].trim_end(), "Invalid date in field ZDATUM");
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
        // Bei F3 (Typ B) wird der ungültige Wert nicht übernommen.
        assert_eq!(&mem[..], &chars_mem("00000000", 8)[..]);
    }

    #[test]
    fn set_screen_in_pai_switches_to_the_next_dynpro() {
        let h = Harness::new("KEY ENTER\nSNAPSHOT\nKEY F3\n");
        install_status();
        let mut second = dynpro(
            200,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_leave, false)],
        );
        second.texts.push(text_at("SECOND", 2, 2));
        h.install(second);
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_count, false)],
            vec![module(pai_set_200, false)],
        ));

        call(100).unwrap();

        let screens = h.screens();
        assert!(screens[0][2].starts_with(" SECOND"));
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn nested_call_screen_returns_and_leave_to_screen_switches_the_caller() {
        let h = Harness::new("KEY ENTER\nKEY ENTER\nSNAPSHOT\nKEY F3\n");
        install_status();
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_count, false)],
            vec![module(pai_call_200, false)],
        ));
        h.install(dynpro(
            200,
            vec![],
            vec![module(pbo_count, false)],
            vec![module(pai_leave_to_300, false)],
        ));
        let mut third = dynpro(
            300,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_leave, false)],
        );
        third.texts.push(text_at("THIRD", 2, 2));
        h.install(third);

        call(100).unwrap();

        assert!(state().stack.is_empty());
        let screens = h.screens();
        assert!(screens[0][2].starts_with(" THIRD"));
        // PAI von 100, 200 und 300 je einmal.
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn leave_to_screen_without_caller_is_a_runtime_error() {
        let h = Harness::new("KEY ENTER\n");
        h.install(dynpro(
            100,
            vec![],
            vec![],
            vec![module(pai_leave_to_300, false)],
        ));
        let err = call(100).unwrap_err();
        assert!(
            err.contains("LEAVE TO SCREEN 0300 without a calling screen"),
            "{err}"
        );
    }

    #[test]
    fn at_exit_module_runs_only_for_exit_function_codes() {
        let h = Harness::new("KEY ENTER\nKEY F3\n");
        install_status();
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_normal, false), module(pai_exit_leave, true)],
        ));

        call(100).unwrap();

        // Enter: normales PAI, kein Verlassen. F3 (Typ B): normales PAI und AT EXIT-COMMAND.
        assert_eq!(NORMAL_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn message_e_in_pai_reruns_pbo_and_keeps_the_message() {
        let h = Harness::new("KEY ENTER\nSNAPSHOT\nKEY F3\n");
        install_status();
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_message_then_leave, false)],
        ));

        call(100).unwrap();

        // PBO: beim Start und nach der Meldung E im PAI. Bei F3 wird die Meldung gelöscht.
        assert_eq!(PBO_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(h.screens()[0][23].trim_end(), "Wrong value");
        assert_eq!(h.message(), None);
    }

    #[test]
    fn unknown_dynpro_is_a_runtime_error() {
        let _h = Harness::new("");
        assert_eq!(call(999).unwrap_err(), "Dynpro 0999 is not defined");
    }

    #[test]
    fn script_end_while_waiting_for_input_is_a_runtime_error() {
        let h = Harness::new("SNAPSHOT\n");
        h.install(dynpro(100, vec![], vec![], vec![module(pai_leave, false)]));
        let err = call(100).unwrap_err();
        assert!(
            err.starts_with("screen input script ended while dynpro 0100"),
            "{err}"
        );
        assert_eq!(h.screens().len(), 1);
    }

    #[test]
    fn message_type_e_outside_a_screen_does_not_change_state() {
        let _h = Harness::new("");
        let text: Vec<u16> = "x".encode_utf16().collect();
        unsafe { abap_rt_message('E' as u32, text.as_ptr(), 1) };
        assert_eq!(state().message, None);
    }
}
