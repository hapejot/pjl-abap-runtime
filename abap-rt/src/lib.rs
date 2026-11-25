//! Laufzeitbibliothek für kompilierte ABAP-Programme.
//! Alle Funktionen haben C-ABI und werden vom generierten Code aufgerufen.
//!
//! Zeichen werden intern als UTF-16-Einheiten (2 Byte, wie in Unicode-SAP-Systemen) gehalten.
//! Längen in den Funktionen sind immer Zeichenzahlen, nicht Bytes.
//! Zeichenfelder (`TYPE c`) sind Puffer fester Länge, der Compiler kennt die Länge.
//! Strings (`TYPE string`) sind Handles auf `RtStr`. Sie werden nicht freigegeben (kein GC).

use std::io::{self, Write};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::atomic::AtomicU64;

const BLANK: u16 = 0x20;

/// Opaker Handle für `TYPE string`.
pub struct RtStr {
    data: Vec<u16>,
}

/// Listzustand: Bit 0 = es wurde schon ausgegeben, Bit 1 = die aktuelle Zeile hat Inhalt.
static LIST: AtomicU8 = AtomicU8::new(0);
const STARTED: u8 = 1;
const DIRTY: u8 = 2;
static EXCEPTION_CLASS: AtomicU64 = AtomicU64::new(0);
static EXCEPTION_OBJECT: AtomicU64 = AtomicU64::new(0);

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

#[no_mangle]
pub extern "C" fn abap_rt_exception_raise(object: *mut u8, class_id: u64) {
    EXCEPTION_OBJECT.store(object as u64, Ordering::SeqCst);
    EXCEPTION_CLASS.store(class_id, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn abap_rt_exception_pending() -> i32 {
    (EXCEPTION_CLASS.load(Ordering::SeqCst) != 0) as i32
}

#[no_mangle]
pub extern "C" fn abap_rt_exception_class() -> u64 {
    EXCEPTION_CLASS.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn abap_rt_exception_object() -> *mut u8 {
    EXCEPTION_OBJECT.load(Ordering::SeqCst) as *mut u8
}

#[no_mangle]
pub extern "C" fn abap_rt_exception_clear() {
    EXCEPTION_CLASS.store(0, Ordering::SeqCst);
    EXCEPTION_OBJECT.store(0, Ordering::SeqCst);
}

/// # Safety
/// `name` zeigt auf `len` UTF-16-Einheiten.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_external_call(name: *const u16, len: u64) {
    let function = String::from_utf16_lossy(units(name, len));
    eprintln!("abapc runtime: externen Funktionsbaustein '{function}' nicht implementiert");
    std::process::exit(78);
}

/// Einfaches ABAP-CP-Matching mit `*` als Wildcard.
///
/// # Safety
/// Beide Handles müssen aus dieser Laufzeit stammen oder null sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_pattern_match(
    value: *const RtStr,
    pattern: *const RtStr,
) -> i32 {
    let Some(value) = value.as_ref() else {
        return 0;
    };
    let Some(pattern) = pattern.as_ref() else {
        return 0;
    };
    wildcard_match(&value.data, &pattern.data) as i32
}

fn wildcard_match(value: &[u16], pattern: &[u16]) -> bool {
    let (mut value_index, mut pattern_index) = (0, 0);
    let mut star = None;
    let mut checkpoint = 0;
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'+' as u16
                || pattern[pattern_index] == value[value_index])
        {
            value_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' as u16 {
            star = Some(pattern_index);
            pattern_index += 1;
            checkpoint = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            checkpoint += 1;
            value_index = checkpoint;
        } else {
            return false;
        }

        /// # Safety
        /// Beide Handles müssen aus dieser Laufzeit stammen oder null sein.
        #[no_mangle]
        pub unsafe extern "C" fn abap_rt_str_contains(
            value: *const RtStr,
            needle: *const RtStr,
        ) -> i32 {
            let Some(value) = value.as_ref() else {
                return 0;
            };
            let Some(needle) = needle.as_ref() else {
                return 0;
            };
            if needle.data.is_empty() {
                return 1;
            }

            /// # Safety
            /// Beide Handles müssen aus dieser Laufzeit stammen oder null sein.
            #[no_mangle]
            pub unsafe extern "C" fn abap_rt_str_contains_any(
                value: *const RtStr,
                chars: *const RtStr,
            ) -> i32 {
                let Some(value) = value.as_ref() else {
                    return 0;
                };
                let Some(chars) = chars.as_ref() else {
                    return 0;
                };
                value
                    .data
                    .iter()
                    .any(|unit| chars.data.contains(unit)) as i32
            }

            /// # Safety
            /// Beide Handles müssen aus dieser Laufzeit stammen oder null sein.
            #[no_mangle]
            pub unsafe extern "C" fn abap_rt_str_contains_only(
                value: *const RtStr,
                chars: *const RtStr,
            ) -> i32 {
                let Some(value) = value.as_ref() else {
                    return 0;
                };
                let Some(chars) = chars.as_ref() else {
                    return 0;
                };
                value.data.iter().all(|unit| chars.data.contains(unit)) as i32
            }

            /// # Safety
            /// Das Handle muss aus dieser Laufzeit stammen oder null sein.
            #[no_mangle]
            pub unsafe extern "C" fn abap_rt_str_length(value: *const RtStr) -> u64 {
                value.as_ref().map_or(0, |string| string.data.len() as u64)
            }

            /// # Safety
            /// Das Handle muss aus dieser Laufzeit stammen oder null sein.
            #[no_mangle]
            pub unsafe extern "C" fn abap_rt_str_slice(
                value: *const RtStr,
                offset: i32,
                length: i32,
            ) -> *mut RtStr {
                let data = value.as_ref().map_or(&[][..], |string| string.data.as_slice());
                let start = offset.max(0) as usize;
                let end = if length < 0 {
                    data.len()
                } else {
                    start.saturating_add(length as usize).min(data.len())
                };
                let slice = if start >= data.len() {
                    &[]
                } else {
                    &data[start..end]
                };
                Box::into_raw(Box::new(RtStr { data: slice.to_vec() }))
            }
            value
                .data
                .windows(needle.data.len())
                .any(|window| window == needle.data)
                as i32
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' as u16 {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
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

#[no_mangle]
pub extern "C" fn abap_rt_int_to_str(value: i32) -> *mut RtStr {
    Box::into_raw(Box::new(RtStr {
        data: value.to_string().encode_utf16().collect(),
    }))
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

/// Laufzeitfehler: Typ eines dynamischen Feldsymbols passt nicht zum erwarteten Typ (CASTING).
#[no_mangle]
pub extern "C" fn abap_rt_type_mismatch() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Laufzeitfehler: Typ des Feldsymbols passt nicht (CASTING)");
    std::process::exit(1)
}
