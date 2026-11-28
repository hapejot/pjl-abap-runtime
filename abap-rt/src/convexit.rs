//! Konvertierungsroutinen (Conversion exits) aus der Domäne, FR-005 Stufe 1.
//!
//! Umgesetzt ist nur `ALPHA` (`CONVERSION_EXIT_ALPHA_OUTPUT` und `CONVERSION_EXIT_ALPHA_INPUT`).
//! Belegt im SAP-Testlauf (`doc/feature-requests/sap-belegtests-fr004-fr005.md`, T8 und T11) und in der
//! ALPHA-Doku (Auszug): Ausgabe entfernt führende Nullen bei rein numerischen Werten, das Ergebnis ist
//! linksbündig. Eingabe füllt rein numerische Werte links mit Nullen bis zur Feldlänge auf.
//!
//! Festlegungen (nicht im Doku-Auszug belegt, siehe FR-005 "Offenes"):
//! - Nicht-numerische Werte bleiben bei der Ausgabe unverändert.
//! - Ein Wert aus nur Nullen wird zu `0`.
//! - Nur die Ziffern `0` bis `9` gelten als numerisch. Vorzeichen und Buchstaben machen den Wert nicht
//!   numerisch. Nachfolgende Leerzeichen zählen nicht (belegt durch T11, MATNR mit Leerzeichen hinten).
//!   Führende Leerzeichen machen den Wert nicht numerisch.
//! - Eingabe eines nicht-numerischen Werts: linksbündig, rechts mit Leerzeichen; ist er länger als das
//!   Feld, wird er abgeschnitten.

use crate::{move_into, out, units, BLANK};

/// `true` für einen nicht leeren Wert aus den Ziffern `0` bis `9`.
fn is_numeric(value: &[u16]) -> bool {
    !value.is_empty()
        && value
            .iter()
            .all(|&c| (b'0' as u16..=b'9' as u16).contains(&c))
}

/// Der numerische Kern eines Werts: der Wert ohne nachfolgende Leerzeichen, falls der Rest rein numerisch
/// ist. Nachfolgende Leerzeichen zählen nicht (belegt durch SAP-Testlauf T11: MATNR mit gespeicherten
/// Nullen gibt `42` aus, obwohl das Feld hinten Leerzeichen hat). `None` bei allen anderen Werten.
fn numeric_core(value: &[u16]) -> Option<&[u16]> {
    let end = value.iter().rposition(|&c| c != BLANK).map_or(0, |i| i + 1);
    let core = &value[..end];
    is_numeric(core).then_some(core)
}

/// Ausgabe-Umwandlung ALPHA: der Wert ohne führende Nullen (nur bei rein numerischen Werten), linksbündig,
/// rechts mit Leerzeichen auf die Länge der Eingabe. Die Länge des Ergebnisses ist immer `value.len()`.
pub fn alpha_output_field(value: &[u16]) -> Vec<u16> {
    let mut result: Vec<u16> = match numeric_core(value) {
        Some(core) => match core.iter().position(|&c| c != b'0' as u16) {
            Some(i) => core[i..].to_vec(),
            // Nur Nullen: Festlegung, Ergebnis ist "0"
            None => vec![b'0' as u16],
        },
        None => value.to_vec(),
    };
    result.resize(value.len(), BLANK);
    result
}

/// Eingabe-Umwandlung ALPHA auf die Feldlänge `len`. Rein numerische Werte, die nicht länger als das Feld
/// sind, werden links mit Nullen aufgefüllt (rechtsbündig). Alle anderen Werte bleiben links, mit
/// Leerzeichen aufgefüllt oder auf `len` abgeschnitten (Festlegung).
pub fn alpha_input_field(value: &[u16], len: usize) -> Vec<u16> {
    if let Some(core) = numeric_core(value) {
        if core.len() <= len {
            let mut result = vec![b'0' as u16; len - core.len()];
            result.extend_from_slice(core);
            return result;
        }
    }
    let mut result: Vec<u16> = value.iter().take(len).copied().collect();
    result.resize(len, BLANK);
    result
}

/// WRITE eines Zeichenfelds mit der Konvertierungsroutine ALPHA (Ausgabe). Die Ausgabe hat die volle Länge
/// des Feldes, wie bei `abap_rt_write_chars`.
///
/// # Safety
/// `ptr` muss auf `len` gültige UTF-16-Einheiten zeigen.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_write_convexit_alpha(ptr: *const u16, len: u64) {
    let field = alpha_output_field(units(ptr, len));
    out(String::from_utf16_lossy(&field).as_bytes());
}

/// Eingabe-Umwandlung ALPHA in das Zielfeld `dst` der Länge `dlen` (aus dem Quellwert `src` der Länge
/// `slen`). Der Zielpuffer wird vollständig beschrieben. Die Codegen-Stufe 1 ruft diese Funktion noch nicht
/// auf (Eingabe ist offen in FR-005).
///
/// # Safety
/// `dst` muss auf `dlen` gültige UTF-16-Einheiten zeigen oder null sein. `src` muss auf `slen` gültige
/// UTF-16-Einheiten zeigen oder null sein.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_convexit_alpha_input(
    dst: *mut u16,
    dlen: u64,
    src: *const u16,
    slen: u64,
) {
    let field = alpha_input_field(units(src, slen), dlen as usize);
    move_into(dst, dlen, &field);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn s(v: &[u16]) -> String {
        String::from_utf16_lossy(v)
    }

    #[test]
    fn alpha_output_removes_leading_zeros_of_numeric_value() {
        // SAP-Testlauf T11: MATNR mit gespeicherten Nullen wird als 42 ausgegeben
        let expected = format!("42{}", " ".repeat(16));
        assert_eq!(s(&alpha_output_field(&u("0000000000000042  "))), expected);
    }

    #[test]
    fn alpha_output_ignores_trailing_blanks_of_numeric_value() {
        // Festlegung, belegt durch T11: der Wert in MATNR hat hinten Leerzeichen
        assert_eq!(s(&alpha_output_field(&u("0042  "))), "42    ");
    }

    #[test]
    fn alpha_output_keeps_value_without_leading_zeros() {
        assert_eq!(s(&alpha_output_field(&u("123"))), "123");
    }

    #[test]
    fn alpha_output_of_only_zeros_gives_single_zero_festlegung() {
        assert_eq!(s(&alpha_output_field(&u("0000"))), "0   ");
    }

    #[test]
    fn alpha_output_keeps_non_numeric_value_festlegung() {
        assert_eq!(s(&alpha_output_field(&u("00AB12"))), "00AB12");
    }

    #[test]
    fn alpha_output_keeps_value_with_blank_festlegung() {
        assert_eq!(s(&alpha_output_field(&u(" 0042"))), " 0042");
    }

    #[test]
    fn alpha_output_result_has_field_length() {
        assert_eq!(alpha_output_field(&u("0042     ")).len(), 9);
        assert_eq!(alpha_output_field(&u("")).len(), 0);
    }

    #[test]
    fn alpha_input_pads_numeric_value_with_zeros_to_field_length() {
        // SAP-Testlauf T8: ALPHA_INPUT('42') in c18
        assert_eq!(s(&alpha_input_field(&u("42"), 18)), "000000000000000042");
    }

    #[test]
    fn alpha_input_pads_numeric_value_with_trailing_blanks_to_field_length() {
        assert_eq!(s(&alpha_input_field(&u("42  "), 6)), "000042");
    }

    #[test]
    fn alpha_input_numeric_value_of_field_length_is_unchanged() {
        assert_eq!(s(&alpha_input_field(&u("12345"), 5)), "12345");
    }

    #[test]
    fn alpha_input_keeps_non_numeric_value_left_aligned_festlegung() {
        assert_eq!(s(&alpha_input_field(&u("AB"), 4)), "AB  ");
    }

    #[test]
    fn alpha_input_truncates_non_numeric_value_longer_than_field_festlegung() {
        assert_eq!(s(&alpha_input_field(&u("ABCDEF"), 3)), "ABC");
    }

    #[test]
    fn alpha_input_numeric_value_longer_than_field_is_truncated_festlegung() {
        assert_eq!(s(&alpha_input_field(&u("1234567"), 5)), "12345");
    }
}
