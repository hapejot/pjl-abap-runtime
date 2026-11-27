//! Schleife je Dynpro (Abschnitt 6.1): PBO, Anzeige, Eingabe, Übertragung, PAI. Aktionen aus Modulen
//! (`SET SCREEN`, `LEAVE`, `MESSAGE`, `SET PF-STATUS`) werden gespeichert und nach dem Modul ausgewertet.
//!
//! Der Zustand liegt im globalen `State` (Mutex). Vor jedem Aufruf eines Moduls wird der Sperrzustand
//! freigegeben, weil Module wieder Laufzeitfunktionen aufrufen (verschachtelter `CALL SCREEN`).

use std::sync::Arc;

use super::grid::{cursor_cell, render_titled};
use super::input::{self, Editor, Fault, Outcome, Pos};
use super::model::{Cond, Dynpro, FKind, GuiStatus, Message, Module, PaiEntry};
use super::terminal::Event;
use super::value::{self, Value};
use super::{open_terminal, state, Nav, State};

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
    /// Wiederaufnahme nach E oder W aus einem Feld- oder Kettenmodul: Index des Eintrags in der PAI-Liste, ab dem
    /// die nächste Übertragung weiterläuft (Abschnitt 12a). Die Eingabebeschränkung steht im `editor`.
    resume: Option<usize>,
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
        resume: None,
    };
    let result = run_loop(&mut run);

    {
        let mut guard = state();
        let st = &mut *guard;
        st.stack.pop();
        // Verlassen des Dynpros: Titel und Cursorwunsch dieses Dynpros gelten nicht mehr.
        forget_screen_actions(st, run.dynpro.no);
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
            // Eingabemarkierungen werden vor den PBO-Modulen zurückgesetzt (Abschnitt 12a).
            run.editor.clear_marks(&run.dynpro);
            let phase = run_phase(&run.dynpro.pbo);
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

        apply_pending_cursor(run)?;
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
///
/// Ablauf bei `AT EXIT-COMMAND` (Spec 12.8, Vorgabe des Auftraggebers): Bei einem Funktionscode vom Typ E läuft
/// das Exit-Modul zuerst. Vorher werden keine Felder übertragen und keine Prüfungen ausgeführt. Verlässt das Modul
/// das Dynpro nicht, folgen Übertragung, Prüfungen und die übrigen PAI-Module. Das Exit-Modul läuft nicht noch einmal.
fn transfer(run: &mut Run, fcode: &str, kind: FKind) -> Result<Option<Exit>, String> {
    // Eine Meldung bleibt bis zur nächsten Übertragung stehen (Abschnitt 6.5).
    {
        let mut st = state();
        st.message = None;
        // `GET CURSOR` im PAI liest den Cursor nach der Taste.
        st.cursor_at = Some((run.dynpro.no, run.editor.cursor(&run.dynpro)));
    }
    set_dynnr(run.dynpro.no)?;
    set_ucomm(fcode)?;

    let exit_module = if kind == FKind::Exit {
        run.dynpro.pai.iter().find_map(|entry| match entry {
            PaiEntry::Module(module) if module.at_exit => Some(*module),
            _ => None,
        })
    } else {
        None
    };
    if let Some(module) = exit_module {
        let phase = run_exit_module(module);
        if let Some(status) = phase.status {
            run.status = Some(status);
        }
        if let Some(nav) = phase.nav {
            return apply_nav(run, nav);
        }
    }

    // Lenient nur, wenn kein Exit-Modul das Dynpro weiterlaufen ließ (siehe 6.4 für B, C, E).
    let lenient = kind.is_exit() && exit_module.is_none();
    let transfers = if lenient {
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

    // Die Prüfung ist bestanden. Eine Wiederaufnahme läuft jetzt ab ihrem Eintrag, danach endet die Beschränkung.
    let start = run.resume.take().unwrap_or(0);
    run.editor.lift_restriction();
    // Die geprüften Werte, nach Feldindex. Geschrieben wird erst an den Einträgen der PAI-Liste (Abschnitt 12a).
    let mut values: Vec<Option<Value>> = (0..run.dynpro.fields.len()).map(|_| None).collect();
    for transfer in transfers {
        values[transfer.index] = Some(transfer.value);
    }

    let end = run_pai(&run.dynpro, &run.editor, &mut values, start)?;
    // Nach dem PAI: PBO des gleichen Dynpros (Abschnitt 6.1), außer die Aktion verlangt etwas anderes.
    run.need_pbo = true;
    match end {
        PaiEnd::Done(phase) => {
            if let Some(status) = phase.status {
                run.status = Some(status);
            }
            match phase.nav {
                Some(nav) => apply_nav(run, nav),
                None => Ok(None),
            }
        }
        PaiEnd::Message {
            entry,
            enabled,
            status,
        } => {
            // Das Dynpro wird neu angezeigt (6.5). Die Meldung bleibt, die Eingabe ist auf `enabled` beschränkt.
            if let Some(status) = status {
                run.status = Some(status);
            }
            run.resume = Some(entry);
            run.editor.restrict(&run.dynpro, &enabled);
            Ok(None)
        }
    }
}

/// Ergebnis der PAI-Liste (Abschnitt 12a).
enum PaiEnd {
    /// Die Liste ist durchlaufen, oder ein Modul hat `LEAVE` oder `SET SCREEN` gesetzt.
    Done(Phase),
    /// E oder W aus einem Feld- oder Kettenmodul beim Eintrag `entry`. `enabled`: die Felder, die danach
    /// eingabebereit sind (das Feld des Moduls bzw. alle Felder der Kette).
    Message {
        entry: usize,
        enabled: Vec<usize>,
        status: Option<Arc<GuiStatus>>,
    },
}

/// Läuft die PAI-Liste ab Eintrag `start` (Abschnitt 12a). Ohne Wiederaufnahme ist `start` 0.
///
/// Felder ohne `FIELD`-Eintrag werden zu Beginn übertragen. Felder eines `FIELD`-Eintrags werden bei diesem Eintrag
/// übertragen, Felder einer Kette beim ersten Eintrag der Kette. Vorher sieht das Programm den alten Wert.
/// `AT EXIT-COMMAND`-Module laufen hier nicht (Schritt 1, siehe `run_exit_module`).
fn run_pai(
    dynpro: &Dynpro,
    editor: &Editor,
    values: &mut [Option<Value>],
    start: usize,
) -> Result<PaiEnd, String> {
    state().pending_nav = None;
    let mut listed = vec![false; dynpro.fields.len()];
    for entry in &dynpro.pai {
        if let PaiEntry::Field { name, .. } = entry {
            listed[field_index_of(dynpro, name)?] = true;
        }
    }
    for index in (0..dynpro.fields.len()).filter(|&i| !listed[i]) {
        write_pending(dynpro, values, index)?;
    }

    for (pos, entry) in dynpro.pai.iter().enumerate().skip(start) {
        match entry {
            PaiEntry::Module(module) => {
                if module.at_exit {
                    continue;
                }
                (module.func)();
                if is_leave(state().pending_nav) {
                    return Ok(PaiEnd::Done(take_actions()));
                }
            }
            PaiEntry::Field {
                name,
                func,
                cond,
                chain,
            } => {
                let index = field_index_of(dynpro, name)?;
                if *chain != 0 {
                    for member in chain_fields(dynpro, *chain)? {
                        write_pending(dynpro, values, member)?;
                    }
                }
                write_pending(dynpro, values, index)?;
                let Some(func) = func else {
                    continue;
                };
                if !fires_field(dynpro, editor, index, *cond)? {
                    continue;
                }
                if let Some(end) = call_guarded(*func, pos, vec![index]) {
                    return Ok(end);
                }
            }
            PaiEntry::Chain { chain, func, cond } => {
                let fields = chain_fields(dynpro, *chain)?;
                for &member in &fields {
                    write_pending(dynpro, values, member)?;
                }
                if !fires_chain(dynpro, editor, &fields, *cond)? {
                    continue;
                }
                if let Some(end) = call_guarded(*func, pos, fields) {
                    return Ok(end);
                }
            }
        }
    }
    Ok(PaiEnd::Done(take_actions()))
}

/// Ruft ein Feld- oder Kettenmodul auf und wertet seine Aktionen aus. `Some` beendet die PAI-Liste.
///
/// `LEAVE` oder `LEAVE TO SCREEN` beendet das PAI mit dieser Aktion. E oder W aus dem Modul zeigt das Dynpro neu an
/// (`PaiEnd::Message`). Die Meldung des Moduls ist die letzte; ohne neue Meldung bleibt die vorherige erhalten.
fn call_guarded(func: extern "C" fn(), pos: usize, enabled: Vec<usize>) -> Option<PaiEnd> {
    let saved = state().message.take();
    func();
    let (nav, interrupt) = {
        let mut st = state();
        let raised = st.message.take();
        let interrupt = matches!(
            raised,
            Some(Message {
                kind: 'E' | 'W',
                ..
            })
        );
        st.message = raised.or(saved);
        (st.pending_nav, interrupt)
    };
    if is_leave(nav) {
        return Some(PaiEnd::Done(take_actions()));
    }
    if !interrupt {
        return None;
    }
    // Eine Navigation dieses Moduls wird verworfen: das Dynpro wird neu angezeigt (Annahme, siehe Bericht).
    let status = {
        let mut st = state();
        st.pending_nav = None;
        st.pending_status.take()
    };
    Some(PaiEnd::Message {
        entry: pos,
        enabled,
        status,
    })
}

fn is_leave(nav: Option<Nav>) -> bool {
    matches!(nav, Some(Nav::Leave | Nav::LeaveTo(_)))
}

/// Ausprägung der Bedingung eines `FIELD`-Eintrags (Abschnitt 12a).
fn fires_field(dynpro: &Dynpro, editor: &Editor, index: usize, cond: Cond) -> Result<bool, String> {
    Ok(match cond {
        Cond::Always => true,
        Cond::OnInput => !is_initial_in_memory(dynpro, index)?,
        Cond::OnRequest => editor.marked(index),
        // Kettenbedingungen gehören zu Kettenmodulen; ein FIELD-Eintrag kann sie nicht haben.
        Cond::ChainInput | Cond::ChainRequest => false,
    })
}

/// Ausprägung der Bedingung eines Kettenmoduls: mindestens ein Kettenfeld (Abschnitt 12a).
fn fires_chain(
    dynpro: &Dynpro,
    editor: &Editor,
    fields: &[usize],
    cond: Cond,
) -> Result<bool, String> {
    match cond {
        Cond::ChainInput => {
            for &index in fields {
                if !is_initial_in_memory(dynpro, index)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Cond::ChainRequest => Ok(fields.iter().any(|&index| editor.marked(index))),
        Cond::Always | Cond::OnInput | Cond::OnRequest => Ok(false),
    }
}

/// Ist der Wert der ABAP-Variablen des Feldes initial? Gelesen wird der aktuelle Speicher, also nach der Übertragung.
fn is_initial_in_memory(dynpro: &Dynpro, index: usize) -> Result<bool, String> {
    let field = &dynpro.fields[index];
    // SAFETY: `field.data` zeigt auf `mem_len` Bytes der Programmvariablen (Vertrag des Prologs).
    let value = unsafe { value::read(field) }.map_err(|e| format!("field {}: {e}", field.name))?;
    Ok(value::is_initial(field, &value))
}

fn field_index_of(dynpro: &Dynpro, name: &str) -> Result<usize, String> {
    dynpro
        .field_index(name)
        .ok_or_else(|| format!("field {name} is not defined in dynpro {}", dynpro.label()))
}

/// Indizes der Felder der Kette `chain`, in der Reihenfolge der PAI-Liste.
fn chain_fields(dynpro: &Dynpro, chain: u32) -> Result<Vec<usize>, String> {
    let mut out = Vec::new();
    for entry in &dynpro.pai {
        if let PaiEntry::Field { name, chain: c, .. } = entry {
            if *c == chain {
                out.push(field_index_of(dynpro, name)?);
            }
        }
    }
    Ok(out)
}

/// Schreibt den geprüften Wert des Feldes in die ABAP-Variable, falls noch nicht geschehen.
fn write_pending(
    dynpro: &Dynpro,
    values: &mut [Option<Value>],
    index: usize,
) -> Result<(), String> {
    let Some(value) = values[index].take() else {
        return Ok(());
    };
    let field = &dynpro.fields[index];
    // SAFETY: `field.data` zeigt auf `mem_len` Bytes der Programmvariablen (Vertrag des Prologs).
    unsafe { value::write(field, &value) }.map_err(|e| format!("field {}: {e}", field.name))
}

/// Wendet eine Navigation an. `Some`, wenn die Schleife des Dynpros endet.
fn apply_nav(run: &mut Run, nav: Nav) -> Result<Option<Exit>, String> {
    match nav {
        Nav::Leave => Ok(Some(Exit::Leave)),
        Nav::LeaveTo(no) => Ok(Some(Exit::LeaveTo(no))),
        Nav::SetScreen(no) => {
            switch_to(run, no)?;
            Ok(None)
        }
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
    let next = find_dynpro(no)?;
    {
        let mut st = state();
        forget_screen_actions(&mut st, run.dynpro.no);
    }
    run.dynpro = next;
    run.editor = Editor::new();
    run.need_pbo = true;
    run.resume = None;
    Ok(())
}

/// Verwirft `SET TITLEBAR` und `SET CURSOR` des Dynpros `no`, wenn es verlassen wird.
fn forget_screen_actions(st: &mut State, no: u32) {
    if st.titlebar.as_ref().is_some_and(|(n, _)| *n == no) {
        st.titlebar = None;
    }
    if st.pending_cursor.as_ref().is_some_and(|(n, _)| *n == no) {
        st.pending_cursor = None;
    }
}

/// Wendet `SET CURSOR` auf das angezeigte Dynpro an. Der Wunsch gilt für die nächste Anzeige dieses Dynpros.
fn apply_pending_cursor(run: &mut Run) -> Result<(), String> {
    let name = {
        let mut st = state();
        match st.pending_cursor.take() {
            Some((no, name)) if no == run.dynpro.no => Some(name),
            other => {
                st.pending_cursor = other;
                None
            }
        }
    };
    match name {
        Some(name) => run.editor.goto(&run.dynpro, &name),
        None => Ok(()),
    }
}

/// Führt die PBO-Module eines Dynpros aus. Nach jedem Modul wird die Aktion ausgewertet: `LEAVE` beendet die Phase.
/// Mehrere `SET SCREEN` im selben Vorgang: die letzte Aktion gilt (Annahme, siehe Bericht).
fn run_phase(modules: &[Module]) -> Phase {
    state().pending_nav = None;
    for module in modules.iter().filter(|m| !m.at_exit) {
        (module.func)();
        if is_leave(state().pending_nav) {
            break;
        }
    }
    take_actions()
}

/// Führt das `AT EXIT-COMMAND`-Modul allein aus (Spec 12.8).
fn run_exit_module(module: Module) -> Phase {
    state().pending_nav = None;
    (module.func)();
    take_actions()
}

fn take_actions() -> Phase {
    let mut guard = state();
    let st = &mut *guard;
    Phase {
        nav: st.pending_nav.take(),
        status: st.pending_status.take(),
    }
}

fn draw(run: &Run) -> Result<(), String> {
    let mut guard = state();
    let st = &mut *guard;
    // `SET TITLEBAR` ersetzt den Titel des Dynpros bis zum Verlassen (siehe `forget_screen_actions`).
    let title = match &st.titlebar {
        Some((no, text)) if *no == run.dynpro.no => text.as_str(),
        _ => run.dynpro.title.as_str(),
    };
    let grid = render_titled(
        &run.dynpro,
        title,
        run.editor.bufs(),
        run.editor.restriction(),
        run.status.as_deref(),
        st.message.as_ref(),
    );
    let pos = run.editor.cursor(&run.dynpro);
    st.cursor_at = Some((run.dynpro.no, pos));
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
        abap_rt_call_screen, abap_rt_get_cursor, abap_rt_leave_screen, abap_rt_leave_to_screen,
        abap_rt_message, abap_rt_set_cursor, abap_rt_set_pf_status, abap_rt_set_screen,
        abap_rt_set_titlebar,
    };
    use crate::screen::headless::Headless;
    use crate::screen::model::{
        AbapType, Cond, FKey, FKind, Field, GuiStatus, Message, Module, PaiEntry, VarPtr, INPUT,
        OUTPUT, PASSWORD, REQUIRED,
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
            FIRED.store(0, Ordering::SeqCst);
            PRE_CALLS.store(0, Ordering::SeqCst);
            POST_CALLS.store(0, Ordering::SeqCst);
            RECORDED.lock().unwrap().clear();
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
        dynpro_entries(
            no,
            fields,
            pbo,
            pai.into_iter().map(PaiEntry::Module).collect(),
        )
    }

    /// Dynpro mit einer geordneten PAI-Liste aus Modulen, Feld- und Kettenmodulen.
    fn dynpro_entries(no: u32, fields: Vec<Field>, pbo: Vec<Module>, pai: Vec<PaiEntry>) -> Dynpro {
        let mut d = Dynpro::new(no, false, 20, 80, "Test".into());
        d.fields = fields;
        d.pbo = pbo;
        d.pai = pai;
        d
    }

    /// Feldeintrag der PAI-Liste ohne Modul.
    fn field_entry(name: &str) -> PaiEntry {
        PaiEntry::Field {
            name: name.into(),
            func: None,
            cond: Cond::Always,
            chain: 0,
        }
    }

    fn module(func: extern "C" fn(), at_exit: bool) -> Module {
        Module { func, at_exit }
    }

    fn install_status() {
        state().statuses.push(Arc::new(GuiStatus {
            name: "STATUS".into(),
            fkeys: vec![
                FKey {
                    key: 3,
                    text: "Back".into(),
                    fcode: "BACK".into(),
                    kind: FKind::Back,
                },
                FKey {
                    key: 12,
                    text: "Exit".into(),
                    fcode: "EXIT".into(),
                    kind: FKind::Exit,
                },
            ],
            excluded: vec![],
        }));
    }

    /// PBO-Modul: setzt den Cursor auf das Feld `ZB` (SET CURSOR).
    extern "C" fn pbo_cursor_zb() {
        PBO_CALLS.fetch_add(1, Ordering::SeqCst);
        let name = utf16("zb");
        unsafe { abap_rt_set_cursor(name.as_ptr(), name.len() as u64) };
    }

    /// PBO-Modul: setzt die Titelzeile (SET TITLEBAR).
    extern "C" fn pbo_titlebar() {
        PBO_CALLS.fetch_add(1, Ordering::SeqCst);
        let text = utf16("Neu");
        unsafe { abap_rt_set_titlebar(text.as_ptr(), text.len() as u64) };
    }

    /// Feldname unter dem Cursor, im PAI mit GET CURSOR gelesen.
    static CURSOR_SEEN: Mutex<String> = Mutex::new(String::new());

    /// PAI-Modul: liest mit GET CURSOR den Feldnamen unter dem Cursor und verlässt das Dynpro.
    extern "C" fn pai_read_cursor() {
        let mut buf = vec![0x20u16; 10];
        unsafe { abap_rt_get_cursor(buf.as_mut_ptr(), buf.len() as u64) };
        *CURSOR_SEEN.lock().unwrap() = String::from_utf16_lossy(&buf).trim_end().to_string();
        abap_rt_leave_screen();
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

    /// Exit-Modul, das das Dynpro nicht verlässt.
    extern "C" fn pai_exit_stay() {
        EXIT_CALLS.fetch_add(1, Ordering::SeqCst);
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

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
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
        let h = Harness::new("KEY ENTER\nKEY F3\nKEY F12\n");
        install_status();
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_normal, false), module(pai_exit_leave, true)],
        ));

        call(100).unwrap();

        // Enter: normales PAI. F3 (Typ B): normales PAI, kein Exit-Modul. F12 (Typ E): nur das Exit-Modul, das verlässt.
        assert_eq!(NORMAL_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn exit_module_that_stays_is_followed_by_checks_and_normal_pai_once() {
        let h = Harness::new("KEY F12\nKEY F3\n");
        install_status();
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_normal, false), module(pai_exit_stay, true)],
        ));

        // Das Dynpro wird nie verlassen, das Skript endet beim Warten auf Eingabe.
        assert!(call(100).is_err());

        // Exit-Modul einmal (F12), danach normales PAI (F12 und F3): das Exit-Modul läuft nicht noch einmal.
        assert_eq!(EXIT_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(NORMAL_CALLS.load(Ordering::SeqCst), 2);
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

    #[test]
    fn password_field_shows_stars_and_transfers_the_typed_text() {
        let h = Harness::new("FIELD ZPWD\nTYPE 'geheim'\nSNAPSHOT\nKEY ENTER\n");
        let mut mem = chars_mem("", 8);
        let field = char_field("ZPWD", 2, 2, 8, INPUT | PASSWORD, &mut mem);
        h.install(dynpro(
            100,
            vec![field],
            vec![module(pbo_count, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        // Anzeige: sechs Sterne, die Leerstellen am Ende bleiben leer. Der Wert im Programm ist der Klartext.
        assert_eq!(&h.screens()[0][2][1..9], "******  ");
        assert_eq!(&mem[..], &chars_mem("geheim", 8)[..]);
        assert_eq!(PAI_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn excluded_function_key_is_not_shown_and_sends_an_empty_code() {
        let h = Harness::new("SNAPSHOT\nKEY F3\nSNAPSHOT\n");
        state().statuses.push(Arc::new(GuiStatus {
            name: "STATUS".into(),
            fkeys: vec![FKey {
                key: 3,
                text: "Back".into(),
                fcode: "BACK".into(),
                kind: FKind::Back,
            }],
            excluded: vec!["BACK".into()],
        }));
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_normal, false)],
        ));

        // Das Dynpro wird nicht verlassen: das Skript endet beim Warten auf Eingabe.
        assert!(call(100).is_err());

        assert_eq!(NORMAL_CALLS.load(Ordering::SeqCst), 1);
        let screens = h.screens();
        assert!(!screens[0][21].contains("Back"));
        assert!(!screens[1][21].contains("Back"));
        // Der Funktionscode ist leer: sy-ucomm enthält nur Leerzeichen.
        assert!(h._ucomm.iter().all(|&u| u == 0x20));
    }

    #[test]
    fn shift_function_key_sends_its_code_and_is_shown_in_the_bar() {
        let h = Harness::new("SNAPSHOT\nKEY SHIFT-F1\n");
        state().statuses.push(Arc::new(GuiStatus {
            name: "STATUS".into(),
            fkeys: vec![FKey {
                key: 13,
                text: "Next".into(),
                fcode: "NEXT".into(),
                kind: FKind::None,
            }],
            excluded: vec![],
        }));
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_status, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        assert!(h.screens()[0][21].starts_with("F13 Next"));
        let code: String = String::from_utf16_lossy(&h._ucomm).trim_end().to_string();
        assert_eq!(code, "NEXT");
    }

    #[test]
    fn set_cursor_in_pbo_puts_the_cursor_on_the_named_field() {
        let h = Harness::new("TYPE 'x'\nKEY ENTER\n");
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let field_a = char_field("ZA", 2, 2, 4, INPUT, &mut mem_a);
        let field_b = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro(
            100,
            vec![field_a, field_b],
            vec![module(pbo_cursor_zb, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        // Der Cursor steht auf ZB (nicht auf dem ersten Eingabefeld ZA), also landet das Zeichen dort.
        assert_eq!(&mem_b[..], &chars_mem("x", 4)[..]);
        assert_eq!(&mem_a[..], &chars_mem("", 4)[..]);
    }

    #[test]
    fn get_cursor_in_pai_names_the_field_under_the_cursor() {
        let h = Harness::new("FIELD ZB\nKEY ENTER\n");
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let field_a = char_field("ZA", 2, 2, 4, INPUT, &mut mem_a);
        let field_b = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro(
            100,
            vec![field_a, field_b],
            vec![module(pbo_count, false)],
            vec![module(pai_read_cursor, false)],
        ));

        call(100).unwrap();

        assert_eq!(*CURSOR_SEEN.lock().unwrap(), "ZB");
    }

    #[test]
    fn titlebar_replaces_the_title_until_the_dynpro_is_left() {
        let h = Harness::new("SNAPSHOT\nKEY ENTER\n");
        h.install(dynpro(
            100,
            vec![],
            vec![module(pbo_titlebar, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        assert!(h.screens()[0][0].starts_with("Neu"));
        assert!(!h.screens()[0][0].contains("Test"));
        // Nach dem Verlassen gilt wieder der Titel aus dem Prolog (hier nicht mehr angezeigt, aber gelöscht).
        assert_eq!(state().titlebar, None);
    }

    #[test]
    fn structure_components_are_fields_at_their_own_addresses() {
        let h = Harness::new("FIELD ZC\nTYPE 'X'\nFIELD ZD\nTYPE '2025'\nKEY ENTER\n");
        // Struktur ZREC ab Offset 4 im Speicher: CHAR 4 bei 0 (8 Bytes), INT bei 8 (4 Bytes), DATS bei 12 (16 Bytes).
        // Die Bytes davor und danach sind Randmarken und dürfen sich nicht ändern.
        let base = 4usize;
        let mut rec = vec![0xEEu8; base + 28 + 4];
        rec[base..base + 8].copy_from_slice(&chars_mem("AB", 4));
        rec[base + 8..base + 12].copy_from_slice(&42i32.to_ne_bytes());
        rec[base + 12..base + 28].copy_from_slice(&chars_mem("20240101", 8));
        let p = rec.as_mut_ptr();
        // SAFETY: alle Adressen liegen innerhalb von `rec`, das während des Tests lebt.
        let (c, i, d) = unsafe { (p.add(base), p.add(base + 8), p.add(base + 12)) };
        let fields = vec![
            Field {
                name: "ZC".into(),
                row: 2,
                col: 2,
                ty: AbapType::Char,
                length: 4,
                decimals: 0,
                attrs: INPUT,
                data: VarPtr(c),
                width: 4,
            },
            Field {
                name: "ZI".into(),
                row: 3,
                col: 2,
                ty: AbapType::Int,
                length: 4,
                decimals: 0,
                attrs: INPUT,
                data: VarPtr(i),
                width: display_width(AbapType::Int, 4, 0),
            },
            Field {
                name: "ZD".into(),
                row: 4,
                col: 2,
                ty: AbapType::Date,
                length: 8,
                decimals: 0,
                attrs: INPUT,
                data: VarPtr(d),
                width: 8,
            },
        ];
        h.install(dynpro(
            100,
            fields,
            vec![module(pbo_count, false)],
            vec![module(pai_leave, false)],
        ));

        call(100).unwrap();

        assert_eq!(&rec[base..base + 8], &chars_mem("XB", 4)[..]);
        assert_eq!(
            i32::from_ne_bytes(rec[base + 8..base + 12].try_into().unwrap()),
            42
        );
        assert_eq!(&rec[base + 12..base + 28], &chars_mem("20250101", 8)[..]);
        assert!(rec[..base].iter().all(|&b| b == 0xEE));
        assert!(rec[base + 28..].iter().all(|&b| b == 0xEE));
    }

    // ---- Abschnitt 12a: FIELD, CHAIN, ON INPUT, ON REQUEST

    /// Auslösungen der Feld- und Kettenmodule.
    static FIRED: AtomicUsize = AtomicUsize::new(0);
    /// Läufe der PAI-Module ohne Feldbezug vor und nach dem Feldeintrag (Wiederaufnahme-Test).
    static PRE_CALLS: AtomicUsize = AtomicUsize::new(0);
    static POST_CALLS: AtomicUsize = AtomicUsize::new(0);
    /// Adresse des Feldes ZA für die Module, die seinen Wert lesen.
    static ZA_ADDR: AtomicUsize = AtomicUsize::new(0);
    /// Vom Modul gelesene Werte von ZA, in Aufrufreihenfolge.
    static RECORDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// Liest den Wert von ZA (vier Zeichen) aus dem Programmspeicher, ohne Leerzeichen am Ende.
    fn read_za() -> String {
        let addr = ZA_ADDR.load(Ordering::SeqCst) as *const u16;
        // SAFETY: `ZA_ADDR` zeigt auf den Speicher von ZA mit vier Zeichen, der während des Tests lebt.
        let units = unsafe { std::slice::from_raw_parts(addr, 4) };
        String::from_utf16_lossy(units).trim_end().to_string()
    }

    /// Liest SY-UCOMM aus dem Zustand, ohne Leerzeichen am Ende.
    fn ucomm_now() -> String {
        let sy = state().sy.expect("sy registered");
        // SAFETY: `sy.ucomm` zeigt auf `sy.ucomm_len` Zeichen der Testvariablen (Harness).
        let units = unsafe { std::slice::from_raw_parts(sy.ucomm.0 as *const u16, sy.ucomm_len) };
        String::from_utf16_lossy(units).trim_end().to_string()
    }

    /// Feld- oder Kettenmodul, das bei jeder Auslösung zählt.
    extern "C" fn fire() {
        FIRED.fetch_add(1, Ordering::SeqCst);
    }

    /// PAI-Modul ohne Feldbezug am Anfang der Liste.
    extern "C" fn pre_count() {
        PRE_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    /// PAI-Modul ohne Feldbezug am Ende der Liste.
    extern "C" fn post_count() {
        POST_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    /// Liest den Wert von ZA beim Aufruf.
    extern "C" fn record_za() {
        RECORDED.lock().unwrap().push(read_za());
    }

    /// PAI-Modul: verlässt das Dynpro nur bei F3 (BACK). Bei Enter läuft es weiter.
    extern "C" fn pai_leave_on_back() {
        if ucomm_now() == "BACK" {
            abap_rt_leave_screen();
        }
    }

    /// Feldmodul für ZA: liest den Wert, meldet E oder W, solange ZA den Wert `x` hat.
    fn field_msg_on_x(kind: char) {
        let value = read_za();
        let triggers = value == "x";
        RECORDED.lock().unwrap().push(value);
        if triggers {
            let text = utf16("Wrong value");
            unsafe { abap_rt_message(kind as u32, text.as_ptr(), text.len() as u64) };
        }
    }

    extern "C" fn field_e_on_x() {
        field_msg_on_x('E');
    }

    extern "C" fn field_w_on_x() {
        field_msg_on_x('W');
    }

    /// Kettenmodul: meldet E, solange ZA den Wert `x` hat.
    extern "C" fn chain_e_on_x() {
        FIRED.fetch_add(1, Ordering::SeqCst);
        if read_za() == "x" {
            let text = utf16("Wrong value");
            unsafe { abap_rt_message('E' as u32, text.as_ptr(), text.len() as u64) };
        }
    }

    /// Modul ohne Feldbezug: setzt die Warnung `Careful`.
    extern "C" fn warn_without_field() {
        let text = utf16("Careful");
        unsafe { abap_rt_message('W' as u32, text.as_ptr(), text.len() as u64) };
    }

    /// Feld ZA (Zeile 2, Spalte 2, vier Zeichen), Speicher `mem`, als Eingabefeld.
    fn za(mem: &mut [u8]) -> Field {
        char_field("ZA", 2, 2, 4, INPUT, mem)
    }

    #[test]
    fn on_input_does_not_fire_for_the_initial_value() {
        let h = Harness::new("KEY ENTER\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("", 4);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: Some(fire),
                    cond: Cond::OnInput,
                    chain: 0,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Blank ist initial: das Modul läuft bei Enter und bei F3 nicht.
        assert_eq!(FIRED.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn on_input_fires_for_a_non_initial_value_and_checks_the_value_not_the_change() {
        let h = Harness::new("FIELD ZA\nTYPE 'x'\nKEY ENTER\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("", 4);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: Some(fire),
                    cond: Cond::OnInput,
                    chain: 0,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Enter und F3 übertragen beide 'x'. Der Wert bleibt, also läuft das Modul zweimal.
        assert_eq!(FIRED.load(Ordering::SeqCst), 2);
        assert_eq!(&mem[..], &chars_mem("x", 4)[..]);
        drop(h);
    }

    #[test]
    fn on_request_needs_input_and_fires_again_for_the_same_value() {
        let h = Harness::new(
            "KEY ENTER\nFIELD ZA\nTYPE 'x'\nKEY ENTER\nFIELD ZA\nTYPE 'x'\nKEY ENTER\nKEY F3\n",
        );
        install_status();
        let mut mem = chars_mem("", 4);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: Some(fire),
                    cond: Cond::OnRequest,
                    chain: 0,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Enter ohne Eingabe: nein. Erste Eingabe 'x': ja. Erneut 'x' (gleicher Wert): ja. F3 nach dem PBO: nein.
        assert_eq!(FIRED.load(Ordering::SeqCst), 2);
        drop(h);
    }

    #[test]
    fn field_is_transferred_at_its_entry_so_earlier_modules_see_the_old_value() {
        let h = Harness::new("FIELD ZA\nTYPE 'NEW'\nKEY ENTER\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("OLD", 4);
        ZA_ADDR.store(mem.as_mut_ptr() as usize, Ordering::SeqCst);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Module(module(record_za, false)),
                field_entry("ZA"),
                PaiEntry::Module(module(record_za, false)),
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        let recorded = RECORDED.lock().unwrap().clone();
        // Vor dem FIELD-Eintrag der alte Wert, danach der neue.
        assert_eq!(recorded[0], "OLD");
        assert_eq!(recorded[1], "NEW");
        drop(h);
    }

    #[test]
    fn chain_input_fires_when_a_chain_field_is_not_initial() {
        let script = "FIELD ZB\nTYPE 'x'\nKEY F3\n";
        let h = Harness::new(script);
        install_status();
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let zb = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem_a), zb],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Field {
                    name: "ZB".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Chain {
                    chain: 1,
                    func: fire,
                    cond: Cond::ChainInput,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // ZB ist 'x': die Kette läuft einmal beim F3-Lauf.
        assert_eq!(FIRED.load(Ordering::SeqCst), 1);
        drop(h);
    }

    #[test]
    fn chain_input_does_not_fire_when_all_chain_fields_are_initial() {
        let h = Harness::new("KEY F3\n");
        install_status();
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let zb = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem_a), zb],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Field {
                    name: "ZB".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Chain {
                    chain: 1,
                    func: fire,
                    cond: Cond::ChainInput,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        assert_eq!(FIRED.load(Ordering::SeqCst), 0);
        drop(h);
    }

    #[test]
    fn chain_request_fires_when_a_chain_field_was_edited() {
        let h = Harness::new("KEY ENTER\nFIELD ZA\nTYPE 'x'\nKEY F3\n");
        install_status();
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let zb = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem_a), zb],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Field {
                    name: "ZB".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Chain {
                    chain: 1,
                    func: fire,
                    cond: Cond::ChainRequest,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Enter ohne Eingabe: nein. Nach der Eingabe in ZA und F3: ja, einmal.
        assert_eq!(FIRED.load(Ordering::SeqCst), 1);
        drop(h);
    }

    /// Gemeinsamer Ablauf für E und W im Feldmodul ZA (Abschnitt 12a): Beschränkung, Wiederaufnahme ab dem
    /// Eintrag, vorherige Einträge laufen nicht erneut.
    fn check_field_message_resumes(func: extern "C" fn()) {
        // Eingabe 'x' in ZA löst die Meldung aus. ZB ist danach gesperrt. Dann 'y' in ZA, Enter, F3.
        let h = Harness::new(
            "FIELD ZA\nTYPE 'x'\nKEY ENTER\nSNAPSHOT\nFIELD ZB\nTYPE 'z'\nFIELD ZA\nTYPE 'y'\nKEY ENTER\nKEY F3\n",
        );
        install_status();
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        ZA_ADDR.store(mem_a.as_mut_ptr() as usize, Ordering::SeqCst);
        let zb = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem_a), zb],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Module(module(pre_count, false)),
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: Some(func),
                    cond: Cond::Always,
                    chain: 0,
                },
                field_entry("ZB"),
                PaiEntry::Module(module(post_count, false)),
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Nach der Meldung im Feldmodul steht die Meldung in Zeile 24, ZB ist nur anzeigbar.
        assert_eq!(h.screens()[0][23].trim_end(), "Wrong value");
        // Feldmodul: beim ersten Lauf 'x' (E/W), bei der Wiederaufnahme 'y', beim F3-Lauf 'y'.
        assert_eq!(*RECORDED.lock().unwrap(), vec!["x", "y", "y"]);
        // Die Einträge vor ZA laufen nur im ersten und im F3-Lauf, nicht bei der Wiederaufnahme.
        assert_eq!(PRE_CALLS.load(Ordering::SeqCst), 2);
        assert_eq!(POST_CALLS.load(Ordering::SeqCst), 2);
        // Die Eingabe in ZB war gesperrt, der Wert bleibt leer.
        assert_eq!(&mem_b[..], &chars_mem("", 4)[..]);
        assert_eq!(&mem_a[..], &chars_mem("y", 4)[..]);
        assert!(state().stack.is_empty());
        drop(h);
    }

    #[test]
    fn message_e_in_field_module_restricts_input_and_resumes_at_that_entry() {
        check_field_message_resumes(field_e_on_x);
    }

    #[test]
    fn message_w_in_field_module_behaves_like_e() {
        check_field_message_resumes(field_w_on_x);
    }

    #[test]
    fn message_w_in_module_without_field_stays_a_plain_message() {
        let h = Harness::new("KEY ENTER\nSNAPSHOT\nFIELD ZA\nTYPE 'x'\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("", 4);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Module(module(warn_without_field, false)),
                field_entry("ZA"),
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Die Warnung bleibt Meldung: Ablauf und Eingabe gehen weiter, ZA bleibt eingabebereit.
        assert_eq!(h.screens()[0][23].trim_end(), "Careful");
        assert_eq!(&mem[..], &chars_mem("x", 4)[..]);
        assert!(state().stack.is_empty());
        drop(h);
    }

    #[test]
    fn message_e_in_chain_module_restricts_input_to_the_chain_fields() {
        let h = Harness::new(
            "FIELD ZA\nTYPE 'x'\nKEY ENTER\nFIELD ZC\nTYPE 'c'\nFIELD ZB\nTYPE 'b'\nFIELD ZA\nTYPE 'y'\nKEY ENTER\nKEY F3\n",
        );
        install_status();
        let mut mem_a = chars_mem("", 4);
        let mut mem_b = chars_mem("", 4);
        let mut mem_c = chars_mem("", 4);
        ZA_ADDR.store(mem_a.as_mut_ptr() as usize, Ordering::SeqCst);
        let zb = char_field("ZB", 4, 2, 4, INPUT, &mut mem_b);
        let zc = char_field("ZC", 6, 2, 4, INPUT, &mut mem_c);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem_a), zb, zc],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "ZA".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Field {
                    name: "ZB".into(),
                    func: None,
                    cond: Cond::Always,
                    chain: 1,
                },
                PaiEntry::Chain {
                    chain: 1,
                    func: chain_e_on_x,
                    cond: Cond::ChainInput,
                },
                field_entry("ZC"),
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));

        call(100).unwrap();

        // Kettenmodul: 'x' löst E aus, es läuft bei der Wiederaufnahme wieder (3 Auslösungen insgesamt).
        assert_eq!(FIRED.load(Ordering::SeqCst), 3);
        // ZC gehört nicht zur Kette: gesperrt, bleibt leer. ZA und ZB sind eingabebereit.
        assert_eq!(&mem_c[..], &chars_mem("", 4)[..]);
        assert_eq!(&mem_b[..], &chars_mem("b", 4)[..]);
        assert_eq!(&mem_a[..], &chars_mem("y", 4)[..]);
        assert!(state().stack.is_empty());
        drop(h);
    }

    #[test]
    fn field_entry_names_match_the_field_case_insensitively() {
        let h = Harness::new("FIELD za\nTYPE 'q'\nKEY F3\n");
        install_status();
        let mut mem = chars_mem("", 4);
        h.install(dynpro_entries(
            100,
            vec![za(&mut mem)],
            vec![module(pbo_status, false)],
            vec![
                PaiEntry::Field {
                    name: "za".into(),
                    func: Some(fire),
                    cond: Cond::OnInput,
                    chain: 0,
                },
                PaiEntry::Module(module(pai_leave_on_back, false)),
            ],
        ));
        call(100).unwrap();
        // Der Eintrag `za` findet das Feld ZA: Übertragung und ON INPUT wirken.
        assert_eq!(&mem[..], &chars_mem("q", 4)[..]);
        assert_eq!(FIRED.load(Ordering::SeqCst), 1);
        drop(h);
    }
}
