//! Laufzeitbibliothek für kompilierte ABAP-Programme.
//! Alle Funktionen haben C-ABI und werden vom generierten Code aufgerufen.
//!
//! Zeichen werden intern als UTF-16-Einheiten (2 Byte, wie in Unicode-SAP-Systemen) gehalten.
//! Längen in den Funktionen sind immer Zeichenzahlen, nicht Bytes.
//! Zeichenfelder (`TYPE c`) sind Puffer fester Länge, der Compiler kennt die Länge.
//! Strings (`TYPE string`) sind Handles auf `RtStr`. Sie werden nicht freigegeben (kein GC).

use std::io::{self, Write};
use std::sync::atomic::{AtomicU8, Ordering};

const BLANK: u16 = 0x20;

/// Opaker Handle für `TYPE string`.
pub struct RtStr {
    data: Vec<u16>,
}

/// Listzustand: Bit 0 = es wurde schon ausgegeben, Bit 1 = die aktuelle Zeile hat Inhalt.
static LIST: AtomicU8 = AtomicU8::new(0);
const STARTED: u8 = 1;
const DIRTY: u8 = 2;

fn out(bytes: &[u8]) {
    if !bytes.is_empty() {
        LIST.fetch_or(STARTED | DIRTY, Ordering::Relaxed);
    }
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(bytes);
}

/// Zeichen aus Zeiger und Länge; Nullzeiger oder Länge 0 ergeben eine leere Folge.
unsafe fn units<'a>(ptr: *const u16, len: u64) -> &'a [u16] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len as usize)
    }
}

/// ABAP-Vergleich für Zeichenfelder: kürzere Folge wird mit Leerzeichen aufgefüllt.
fn cmp_padded(a: &[u16], b: &[u16]) -> i32 {
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(BLANK);
        let y = b.get(i).copied().unwrap_or(BLANK);
        if x != y {
            return if x < y { -1 } else { 1 };
        }
    }
    0
}

fn sign(o: std::cmp::Ordering) -> i32 {
    match o {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Kopiert `src` in das Zeichenfeld `dst` (`dlen` Zeichen): abschneiden oder mit Leerzeichen auffüllen.
unsafe fn move_into(dst: *mut u16, dlen: u64, src: &[u16]) {
    if dst.is_null() {
        return;
    }
    let d = std::slice::from_raw_parts_mut(dst, dlen as usize);
    for (i, slot) in d.iter_mut().enumerate() {
        *slot = src.get(i).copied().unwrap_or(BLANK);
    }
}

/// WRITE für ganze Zahlen (ABAP-Darstellung: ohne führende Nullen, negatives Vorzeichen am Ende).
#[no_mangle]
pub extern "C" fn abap_rt_write_int(value: i32) {
    let mut s = value.unsigned_abs().to_string();
    if value < 0 {
        s.push('-');
    }
    out(format!("{:>10}", s).as_bytes());
}

/// WRITE für Zeichenfelder: gibt die vollen `len` Zeichen aus (inkl. Auffüllung), als UTF-8.
///
/// # Safety
/// `ptr` muss auf `len` gültige UTF-16-Einheiten zeigen.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_write_chars(ptr: *const u16, len: u64) {
    out(String::from_utf16_lossy(units(ptr, len)).as_bytes());
}

/// WRITE für Strings.
///
/// # Safety
/// `h` muss ein Handle aus diesem Crate sein oder null.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_write_str(h: *const RtStr) {
    if let Some(s) = h.as_ref() {
        out(String::from_utf16_lossy(&s.data).as_bytes());
    }
}

/// `WRITE /`: beendet die aktuelle Zeile. Vor der ersten Ausgabe gibt es keine leere Zeile.
#[no_mangle]
pub extern "C" fn abap_rt_newline() {
    if LIST.load(Ordering::Relaxed) & STARTED != 0 {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(b"\n");
    }
    LIST.store(STARTED, Ordering::Relaxed);
}

/// `WRITE `: erzeugt einen Field Separator. Vor der ersten Ausgabe gibt es keine leere Zeile.
#[no_mangle]
pub extern "C" fn abap_rt_newfield() {
    if LIST.load(Ordering::Relaxed) & DIRTY != 0 {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(b"\t \t");
    }
    LIST.store(STARTED, Ordering::Relaxed);
}

/// Programmende: schließt eine offene Zeile ab.
#[no_mangle]
pub extern "C" fn abap_rt_finish() {
    if LIST.load(Ordering::Relaxed) & DIRTY != 0 {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(b"\n");
    }
    LIST.store(0, Ordering::Relaxed);
}

/// Zuweisung Zeichen -> Zeichen: abschneiden bzw. mit Leerzeichen auffüllen.
/// Die Quelle wird vorher kopiert, damit sich überlappende Bereiche (Teilfelder) korrekt verhalten.
///
/// # Safety
/// Die Zeiger müssen auf die angegebenen Längen gültig sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_chars_move(dst: *mut u16, dlen: u64, src: *const u16, slen: u64) {
    let copy = units(src, slen).to_vec();
    move_into(dst, dlen, &copy);
}

/// Vergleich zweier Zeichenfelder mit Leerzeichen-Auffüllung. Ergebnis: -1, 0 oder 1.
///
/// # Safety
/// Die Zeiger müssen auf die angegebenen Längen gültig sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_chars_cmp(
    a: *const u16,
    alen: u64,
    b: *const u16,
    blen: u64,
) -> i32 {
    cmp_padded(units(a, alen), units(b, blen))
}

/// Neuer String aus Zeichen (kopiert, Leerzeichen bleiben erhalten).
///
/// # Safety
/// Die Zeiger müssen auf die angegebenen Längen gültig sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_str_new(ptr: *const u16, len: u64) -> *mut RtStr {
    Box::into_raw(Box::new(RtStr {
        data: units(ptr, len).to_vec(),
    }))
}

/// Neuer String aus einem Zeichenfeld; nachgestellte Leerzeichen werden entfernt.
///
/// # Safety
/// Die Zeiger müssen auf die angegebenen Längen gültig sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_str_from_chars(ptr: *const u16, len: u64) -> *mut RtStr {
    let mut v = units(ptr, len).to_vec();
    while v.last() == Some(&BLANK) {
        v.pop();
    }
    Box::into_raw(Box::new(RtStr { data: v }))
}

/// Zuweisung String -> Zeichenfeld: abschneiden bzw. auffüllen.
///
/// # Safety
/// `dst` muss auf `dlen` Zeichen zeigen, `h` ist ein gültiges Handle oder null.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_str_to_chars(dst: *mut u16, dlen: u64, h: *const RtStr) {
    let src: &[u16] = h.as_ref().map(|s| s.data.as_slice()).unwrap_or(&[]);
    move_into(dst, dlen, src);
}

/// Verkettung zweier Strings; liefert einen neuen Handle.
///
/// # Safety
/// Beide Handles müssen gültig oder null sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_str_concat(a: *const RtStr, b: *const RtStr) -> *mut RtStr {
    let mut v = a.as_ref().map(|s| s.data.clone()).unwrap_or_default();
    if let Some(s) = b.as_ref() {
        v.extend_from_slice(&s.data);
    }
    Box::into_raw(Box::new(RtStr { data: v }))
}

/// Vergleich zweier Strings: exakt, d.h. ungleich wenn Länge oder Inhalt abweichen.
/// Ergebnis -1, 0 oder 1 (lexikografisch).
///
/// # Safety
/// Beide Handles müssen gültig oder null sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_str_cmp(a: *const RtStr, b: *const RtStr) -> i32 {
    let x: &[u16] = a.as_ref().map(|s| s.data.as_slice()).unwrap_or(&[]);
    let y: &[u16] = b.as_ref().map(|s| s.data.as_slice()).unwrap_or(&[]);
    sign(x.cmp(y))
}

/// Interne Tabelle (`TYPE TABLE OF`). Jede Zeile hat einen eigenen, festen Speicherplatz,
/// damit Zeiger auf Zeilen (Feldsymbole) auch nach `APPEND` gültig bleiben.
pub struct RtTable {
    elem_bytes: usize,
    rows: Vec<Box<[u64]>>,
}

/// Neue leere Tabelle für Zeilen der Größe `elem_bytes`.
#[no_mangle]
pub extern "C" fn abap_rt_table_new(elem_bytes: u64) -> *mut RtTable {
    Box::into_raw(Box::new(RtTable {
        elem_bytes: elem_bytes.max(1) as usize,
        rows: Vec::new(),
    }))
}

/// Hängt eine Zeile an; kopiert `bytes` Bytes von `src`.
///
/// # Safety
/// `t` muss eine Tabelle aus diesem Crate sein, `src` gültig für `bytes` Bytes.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_table_append(t: *mut RtTable, src: *const u8, bytes: u64) {
    let Some(t) = t.as_mut() else { return };
    let mut row = vec![0u64; t.elem_bytes.div_ceil(8)].into_boxed_slice();
    let n = (bytes as usize).min(t.elem_bytes);
    if !src.is_null() && n > 0 {
        std::ptr::copy_nonoverlapping(src, row.as_mut_ptr() as *mut u8, n);
    }
    t.rows.push(row);
}

/// Anzahl der Zeilen.
///
/// # Safety
/// `t` muss eine gültige Tabelle sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_table_count(t: *const RtTable) -> i64 {
    t.as_ref().map(|t| t.rows.len() as i64).unwrap_or(0)
}

/// Zeiger auf Zeile `idx` (0-basiert). Bleibt gültig, bis die Tabelle geleert oder freigegeben wird.
///
/// # Safety
/// `t` muss eine gültige Tabelle sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_table_row(t: *mut RtTable, idx: i64) -> *mut u8 {
    match t.as_mut() {
        Some(t) if idx >= 0 && (idx as usize) < t.rows.len() => {
            t.rows[idx as usize].as_mut_ptr() as *mut u8
        }
        _ => std::ptr::null_mut(),
    }
}

/// Entfernt alle Zeilen.
///
/// # Safety
/// `t` muss eine gültige Tabelle sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_table_clear(t: *mut RtTable) {
    if let Some(t) = t.as_mut() {
        t.rows.clear();
    }
}

/// Laufzeitfehler: Feldsymbol ist nicht zugewiesen. Beendet das Programm mit Status 1.
#[no_mangle]
pub extern "C" fn abap_rt_fs_unassigned() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Laufzeitfehler: Feldsymbol ist nicht zugewiesen (GETWA_NOT_ASSIGNED)");
    std::process::exit(1)
}

/// Neues Objekt: zeroed Speicher der Größe `size` Bytes (8-Byte-ausgerichtet). Lebt bis Programmende.
#[no_mangle]
pub extern "C" fn abap_rt_obj_new(size: u64) -> *mut u8 {
    let size = (size as usize).max(8);
    let layout = std::alloc::Layout::from_size_align(size, 8).expect("gültiges Layout");
    unsafe { std::alloc::alloc_zeroed(layout) }
}

/// Laufzeitfehler: Objektreferenz ist nicht gesetzt. Beendet das Programm mit Status 1.
#[no_mangle]
pub extern "C" fn abap_rt_unbound() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Laufzeitfehler: Objektreferenz ist nicht gesetzt (CX_SY_REF_IS_INITIAL)");
    std::process::exit(1)
}
