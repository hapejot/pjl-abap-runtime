//! Laufzeitbibliothek für kompilierte ABAP-Programme.
//! Alle Funktionen haben C-ABI und werden vom generierten Code aufgerufen.
//!
//! Zeichen werden intern als UTF-16-Einheiten (2 Byte, wie in Unicode-SAP-Systemen) gehalten.
//! Längen in den Funktionen sind immer Zeichenzahlen, nicht Bytes.
//! Zeichenfelder (`TYPE c`) sind Puffer fester Länge, der Compiler kennt die Länge.
//! Strings (`TYPE string`) sind Handles auf `RtStr`. Sie werden nicht freigegeben (kein GC).

pub mod convexit;
pub mod decimal;
pub mod packed;
pub mod screen;
pub mod sql;

use std::io::{self, Write};
use std::sync::atomic::AtomicU64;
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
    eprintln!("abapc runtime: external function module '{function}' not implemented");
    std::process::exit(78);
}

/// Einfaches ABAP-CP-Matching mit `*` als Wildcard.
///
/// # Safety
/// Beide Handles müssen aus dieser Laufzeit stammen oder null sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_pattern_match(value: *const RtStr, pattern: *const RtStr) -> i32 {
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
                value.data.iter().any(|unit| chars.data.contains(unit)) as i32
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
                let data = value
                    .as_ref()
                    .map_or(&[][..], |string| string.data.as_slice());
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
                Box::into_raw(Box::new(RtStr {
                    data: slice.to_vec(),
                }))
            }
            value
                .data
                .windows(needle.data.len())
                .any(|window| window == needle.data) as i32
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

/// WRITE für ganze Zahlen. Projektfestlegung (siehe doc/sprache.md Abschnitt 7): Die Ziffern sind
/// rechtsbündig in 10 Stellen. Negative Zahlen haben das Minuszeichen danach, dadurch 11 Zeichen.
/// Positive Zahlen haben 10 Zeichen, das Alignment zwischen Feldern kommt über die Tabs.
#[no_mangle]
pub extern "C" fn abap_rt_write_int(value: i32) {
    let mut s = format!("{:>10}", value.unsigned_abs());
    if value < 0 {
        s.push('-');
    }

    out(s.as_bytes());
}

#[no_mangle]
pub extern "C" fn abap_rt_int_to_str(value: i32) -> *mut RtStr {
    Box::into_raw(Box::new(RtStr {
        data: value.to_string().encode_utf16().collect(),
    }))
}

/// Zeichenfolge für die Zuweisung einer ganzen Zahl (`TYPE i`) an ein Zeichenfeld der Länge `len` (FR-004).
///
/// Regel aus `doc/feature-requests/FR-004-zahl-in-zeichenfeld.md`, Abschnitt "Beobachtetes Verhalten"
/// (SAP-Testlauf S/4HANA 2025, Kernel 916). `d` ist die Anzahl der Ziffern von |value|; 0 hat eine Ziffer.
/// - positiv oder 0, d < L: Ziffern rechtsbündig in L-1 Stellen, letzte Stelle Leerzeichen
/// - positiv, d = L: nur die Ziffern, keine Vorzeichenstelle
/// - positiv, d > L: `*` vorne, danach die rechten L-1 Ziffern
/// - negativ, d < L: Ziffern rechtsbündig in L-1 Stellen, letzte Stelle `-`
/// - negativ, d >= L: `*` vorne, danach die rechten L-2 Ziffern, letzte Stelle `-`; bei L = 1 nur `*`
///
/// Kein Fehler und kein Abbruch bei Überlauf. Die Umwandlung ist keine Ausgabe mit `WRITE`.
/// Das Ergebnis hat immer genau `len` Zeichen (bei `len` = 0 ist es leer).
fn int_to_char_field(value: i32, len: usize) -> Vec<u16> {
    if len == 0 {
        return Vec::new();
    }
    let digits = value.unsigned_abs().to_string();
    let d = digits.len();
    let text = if value >= 0 {
        if d < len {
            format!("{digits:>width$} ", width = len - 1)
        } else if d == len {
            digits
        } else {
            format!("*{}", &digits[d - (len - 1)..])
        }
    } else if d < len {
        format!("{digits:>width$}-", width = len - 1)
    } else if len == 1 {
        "*".to_string()
    } else {
        format!("*{}-", &digits[d - (len - 2)..])
    };
    text.encode_utf16().collect()
}

/// Zuweisung einer ganzen Zahl (`TYPE i`) an ein Zeichenfeld fester Länge `dlen` (FR-004).
/// Der Zielpuffer wird vollständig beschrieben: Ergebnis genau `dlen` Zeichen.
///
/// # Safety
/// `dst` muss auf `dlen` gültige UTF-16-Einheiten zeigen oder null sein (dann geschieht nichts).
#[no_mangle]
pub unsafe extern "C" fn abap_rt_int_to_chars(dst: *mut u16, dlen: u64, value: i32) {
    let text = int_to_char_field(value, dlen as usize);
    move_into(dst, dlen, &text);
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

/// Tabellenzuweisung `dst = src`: der Inhalt des Ziels wird durch alle Zeilen der Quelle ersetzt.
/// Zuweisung an sich selbst lässt den Inhalt unverändert.
///
/// # Safety
/// Beide Handles müssen gültige Tabellen aus diesem Crate sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_table_assign(dst: *mut RtTable, src: *const RtTable) {
    let (Some(d), Some(s)) = (dst.as_mut(), src.as_ref()) else {
        return;
    };
    if std::ptr::eq(d, s) {
        return;
    }
    d.rows = s.rows.clone();
}

/// Laufzeitfehler: Feldsymbol ist nicht zugewiesen. Beendet das Programm mit Status 1.
#[no_mangle]
pub extern "C" fn abap_rt_fs_unassigned() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Runtime error: field symbol is not assigned (GETWA_NOT_ASSIGNED)");
    std::process::exit(1)
}

/// Neues Objekt: zeroed Speicher der Größe `size` Bytes (8-Byte-ausgerichtet). Lebt bis Programmende.
#[no_mangle]
pub extern "C" fn abap_rt_obj_new(size: u64) -> *mut u8 {
    let size = (size as usize).max(8);
    let layout = std::alloc::Layout::from_size_align(size, 8).expect("valid layout");
    unsafe { std::alloc::alloc_zeroed(layout) }
}

/// Laufzeitfehler: Objektreferenz ist nicht gesetzt. Beendet das Programm mit Status 1.
#[no_mangle]
pub extern "C" fn abap_rt_unbound() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Runtime error: object reference is not set (CX_SY_REF_IS_INITIAL)");
    std::process::exit(1)
}

/// Laufzeitfehler: Typ eines dynamischen Feldsymbols passt nicht zum erwarteten Typ (CASTING).
#[no_mangle]
pub extern "C" fn abap_rt_type_mismatch() -> ! {
    let _ = io::stdout().flush();
    eprintln!("Runtime error: type of field symbol does not match (CASTING)");
    std::process::exit(1)
}

/// Laufzeitfehler bei ganzzahliger Arithmetik: 0 = Überlauf von `TYPE i`, 1 = Division durch 0.
#[no_mangle]
pub extern "C" fn abap_rt_arith_error(kind: i32) -> ! {
    let _ = io::stdout().flush();
    match kind {
        0 => eprintln!("Runtime error: arithmetic overflow for integer (TYPE i)"),
        _ => eprintln!("Runtime error: division by 0"),
    }
    std::process::exit(1)
}

#[cfg(test)]
mod table_tests {
    use super::*;

    /// Liest alle Zeilen als Bytes, damit der Inhalt ohne Kenntnis der Zeilenstruktur verglichen werden kann.
    unsafe fn rows_of(t: *const RtTable) -> Vec<u8> {
        let count = abap_rt_table_count(t);
        let mut out = Vec::new();
        for idx in 0..count {
            let row = abap_rt_table_row(t as *mut RtTable, idx);
            out.extend_from_slice(std::slice::from_raw_parts(row, 1));
        }
        out
    }

    unsafe fn append_byte(t: *mut RtTable, value: u8) {
        abap_rt_table_append(t, &value as *const u8, 1);
    }

    #[test]
    fn table_assign_replaces_target_rows_with_copy_of_source() {
        unsafe {
            let dst = abap_rt_table_new(1);
            let src = abap_rt_table_new(1);
            append_byte(dst, 9);
            append_byte(src, 1);
            append_byte(src, 2);

            abap_rt_table_assign(dst, src);

            assert_eq!(rows_of(dst), vec![1, 2]);
            assert_eq!(rows_of(src), vec![1, 2], "source keeps its rows");
            append_byte(src, 3);
            assert_eq!(
                rows_of(dst),
                vec![1, 2],
                "later changes of the source do not reach the copy"
            );
        }
    }

    #[test]
    fn table_assign_from_empty_source_empties_target() {
        unsafe {
            let dst = abap_rt_table_new(1);
            let src = abap_rt_table_new(1);
            append_byte(dst, 7);

            abap_rt_table_assign(dst, src);

            assert_eq!(abap_rt_table_count(dst), 0);
        }
    }

    #[test]
    fn table_assign_to_itself_keeps_rows() {
        unsafe {
            let t = abap_rt_table_new(1);
            append_byte(t, 4);
            append_byte(t, 5);

            abap_rt_table_assign(t, t);

            assert_eq!(rows_of(t), vec![4, 5]);
        }
    }

    #[test]
    fn table_assign_with_null_handle_does_nothing() {
        unsafe {
            let t = abap_rt_table_new(1);
            append_byte(t, 6);

            abap_rt_table_assign(t, std::ptr::null());
            abap_rt_table_assign(std::ptr::null_mut(), t);

            assert_eq!(rows_of(t), vec![6]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(value: i32, len: usize) -> String {
        String::from_utf16(&int_to_char_field(value, len)).expect("valid UTF-16")
    }

    fn blanks(n: usize) -> String {
        " ".repeat(n)
    }

    #[test]
    fn positive_with_fewer_digits_is_right_aligned_with_blank_at_the_end() {
        assert_eq!(field(42, 15), format!("{}42 ", blanks(12)));
        assert_eq!(field(0, 5), format!("{}0 ", blanks(3)));
        assert_eq!(field(1234, 5), "1234 ");
        assert_eq!(field(12345678, 9), "12345678 ");
    }

    #[test]
    fn zero_in_length_one_is_the_digit_without_sign_place() {
        assert_eq!(field(0, 1), "0");
    }

    #[test]
    fn positive_with_exactly_length_digits_fills_the_field() {
        assert_eq!(field(12345, 5), "12345");
        assert_eq!(field(123456789, 9), "123456789");
    }

    #[test]
    fn positive_overflow_marks_star_and_keeps_the_last_length_minus_one_digits() {
        assert_eq!(field(123456, 5), "*3456");
        assert_eq!(field(1234567, 2), "*7");
    }

    #[test]
    fn negative_with_fewer_digits_is_right_aligned_with_minus_at_the_end() {
        assert_eq!(field(-42, 15), format!("{}42-", blanks(12)));
        assert_eq!(field(-42, 10), format!("{}42-", blanks(7)));
        assert_eq!(field(-42, 3), "42-");
    }

    #[test]
    fn negative_with_digits_reaching_length_marks_star_and_keeps_last_length_minus_two_digits() {
        assert_eq!(field(-12345, 5), "*345-");
        assert_eq!(field(-123456, 5), "*456-");
        assert_eq!(field(-123, 3), "*3-");
        assert_eq!(field(-42, 2), "*-");
    }

    #[test]
    fn length_one_keeps_only_the_star_for_negative_and_overflow_values() {
        assert_eq!(field(-5, 1), "*");
        assert_eq!(field(12, 1), "*");
    }

    #[test]
    fn i32_limits_are_formatted_without_overflow_panic() {
        assert_eq!(field(i32::MAX, 10), "2147483647");
        assert_eq!(field(i32::MIN, 11), "2147483648-");
        assert_eq!(field(i32::MIN, 10), "*47483648-");
    }

    #[test]
    fn zero_length_field_receives_no_characters() {
        assert_eq!(field(5, 0), "");
        assert_eq!(field(-5, 0), "");
    }

    #[test]
    fn result_always_has_exactly_the_field_length() {
        let values = [
            0,
            1,
            -1,
            42,
            -42,
            12345,
            -12345,
            123456789,
            i32::MAX,
            i32::MIN,
        ];
        for len in 1..=12 {
            for value in values {
                assert_eq!(
                    int_to_char_field(value, len).len(),
                    len,
                    "value {value} in length {len}"
                );
            }
        }
    }

    #[test]
    fn c_function_fills_the_whole_buffer_and_ignores_null() {
        let mut buf = vec![b'x' as u16; 15];
        unsafe {
            abap_rt_int_to_chars(buf.as_mut_ptr(), 15, -42);
            assert_eq!(
                String::from_utf16(&buf).unwrap(),
                format!("{}42-", blanks(12))
            );
            abap_rt_int_to_chars(std::ptr::null_mut(), 15, 42);
        }
    }
}
