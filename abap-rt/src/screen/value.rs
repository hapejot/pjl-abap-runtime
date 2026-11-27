//! Umwandlung zwischen ABAP-Werten und Anzeigetext (Abschnitt 4.7) und Prüfung der Eingabe (Abschnitte 6.3, 6.4).
//!
//! Die Funktionen sind rein bis auf `read`, `write` und `write_units`, die den Speicher eines Feldes lesen
//! bzw. schreiben. Dezimalzahlen laufen über `decimal::Dec`, Speicherform und Fehler über `Dec::unpack`/`pack`.

use super::model::{AbapType, Field};
use crate::decimal::Dec;
use crate::packed::PackedError;

/// Leerzeichen als UTF-16-Einheit.
pub const BLANK: u16 = 0x20;
/// Anzeigebreite einer ganzen Zahl: `-2147483648`.
pub const INT_WIDTH: usize = 11;

/// ABAP-Wert eines Feldes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Zeichen (UTF-16-Einheiten) für `c`, `n`, `d`, `t`.
    Text(Vec<u16>),
    /// Ganze Zahl (`i`).
    Int(i32),
    /// Gepackte Dezimalzahl (`p`), auf die Skala des Feldes gebracht.
    Packed(Dec),
}

/// Grund, warum eine Eingabe nicht übernommen wird.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    /// `REQUIRED` und der Wert ist initial.
    Required,
    /// Kein gültiges Datum (`d`).
    Date,
    /// Keine gültige Uhrzeit (`t`).
    Time,
    /// Keine gültige Zahl (`n`, `i`, `p`).
    Number,
    /// Wert passt nicht in das Feld (`i` außerhalb des Bereichs, `p` zu viele Stellen).
    OutOfRange,
}

/// Anzeigebreite eines Feldes in Zeichen. Bei `i` und `p` berechnet die Laufzeit die Breite aus dem Typ.
pub fn display_width(ty: AbapType, length: usize, decimals: usize) -> usize {
    match ty {
        AbapType::Int => INT_WIDTH,
        // Ziffern (2 je Byte, minus Vorzeichen-Halbbyte), optional Dezimalpunkt, Vorzeichen.
        AbapType::Packed => 2 * length - 1 + usize::from(decimals > 0) + 1,
        _ => length,
    }
}

/// Meldungstext für einen Speicherfehler. Gleiche Texte wie in `lib.rs`.
fn packed_message(err: PackedError) -> &'static str {
    match err {
        PackedError::InvalidNibble => "invalid packed number",
        PackedError::Overflow => "Overflow in packed number",
    }
}

/// Liest den Wert aus dem Speicher des Feldes.
///
/// # Safety
/// `field.data` muss auf `field.mem_len()` gültige Bytes zeigen (Vertrag des Prologs).
pub unsafe fn read(field: &Field) -> Result<Value, &'static str> {
    let bytes = std::slice::from_raw_parts(field.data.0 as *const u8, field.mem_len());
    Ok(match field.ty {
        AbapType::Char | AbapType::Num | AbapType::Date | AbapType::Time => Value::Text(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_ne_bytes(*c))
                .collect(),
        ),
        AbapType::Int => Value::Int(i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        AbapType::Packed => {
            Value::Packed(Dec::unpack(bytes, field.decimals).map_err(packed_message)?)
        }
    })
}

/// Schreibt den Wert in den Speicher des Feldes. Der Wert muss zum Typ des Feldes passen (siehe `parse`).
///
/// # Safety
/// `field.data` muss auf `field.mem_len()` beschreibbare Bytes zeigen (Vertrag des Prologs).
pub unsafe fn write(field: &Field, value: &Value) -> Result<(), &'static str> {
    let bytes = std::slice::from_raw_parts_mut(field.data.0, field.mem_len());
    match (field.ty, value) {
        (AbapType::Char | AbapType::Num | AbapType::Date | AbapType::Time, Value::Text(units)) => {
            for (i, slot) in bytes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *slot = units.get(i).copied().unwrap_or(BLANK).to_ne_bytes();
            }
        }
        (AbapType::Int, Value::Int(v)) => bytes.copy_from_slice(&v.to_ne_bytes()),
        (AbapType::Packed, Value::Packed(d)) => {
            let packed = d.pack(field.length).map_err(packed_message)?;
            bytes.copy_from_slice(&packed);
        }
        _ => return Err("internal error: value does not match the type of the dynpro field"),
    }
    Ok(())
}

/// Schreibt Zeichen in ein Zeichenfeld mit `len` Zeichen: abschneiden oder mit Leerzeichen auffüllen.
///
/// # Safety
/// `dst` muss auf `len` UTF-16-Einheiten zeigen.
pub unsafe fn write_units(dst: *mut u8, len: usize, src: &[u16]) {
    let bytes = std::slice::from_raw_parts_mut(dst, len * 2);
    for (i, slot) in bytes.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        *slot = src.get(i).copied().unwrap_or(BLANK).to_ne_bytes();
    }
}

/// Zeichen für eine UTF-16-Einheit. Ersatzzeichen (einzelne Surrogate) werden durch U+FFFD ersetzt.
fn unit_char(unit: u16) -> char {
    char::from_u32(unit as u32).unwrap_or('\u{FFFD}')
}

/// Rechtsbündiger Anzeigetext in der Breite `width`. Zu lange Texte werden links abgeschnitten.
fn right_aligned(text: &str, width: usize) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() >= width {
        return chars[chars.len() - width..].to_vec();
    }
    let mut out = vec![' '; width - chars.len()];
    out.extend(chars);
    out
}

/// Anzeigetext des Werts in der Breite `width` (Abschnitt 4.7). Zeichen links, Zahlen rechts.
pub fn format(value: &Value, width: usize) -> Vec<char> {
    match value {
        Value::Text(units) => {
            let mut chars: Vec<char> = units.iter().map(|&u| unit_char(u)).collect();
            chars.resize(width, ' ');
            chars
        }
        Value::Int(v) => right_aligned(&v.to_string(), width),
        Value::Packed(d) => right_aligned(&d.format(), width),
    }
}

/// Ist der Wert initial? `c`: nur Leerzeichen. `n`, `d`, `t`: nur Nullen. `i` und `p`: Null.
pub fn is_initial(field: &Field, value: &Value) -> bool {
    match value {
        Value::Text(units) => match field.ty {
            AbapType::Char => units.iter().all(|&u| u == BLANK),
            _ => units.iter().all(|&u| u == b'0' as u16),
        },
        Value::Int(v) => *v == 0,
        Value::Packed(d) => d.digits.iter().all(|&x| x == 0),
    }
}

/// Darf an Position `pos` das Zeichen `ch` getippt werden? Prüft den Typ (Abschnitt 6.3).
/// `buf` ist der Anzeigepuffer des Feldes; das Zeichen an `pos` wird überschrieben.
pub fn accepts(field: &Field, buf: &[char], pos: usize, ch: char) -> bool {
    if ch.is_control() || ch as u32 > 0xFFFF {
        return false;
    }
    let elsewhere = |c: char| buf.iter().enumerate().any(|(i, &b)| i != pos && b == c);
    match field.ty {
        AbapType::Char => true,
        AbapType::Num | AbapType::Date | AbapType::Time => ch.is_ascii_digit(),
        AbapType::Int => ch.is_ascii_digit() || (ch == '-' && !elsewhere('-')),
        AbapType::Packed => {
            ch.is_ascii_digit()
                || (ch == '-' && !elsewhere('-'))
                || (ch == '.' && field.decimals > 0 && !elsewhere('.'))
        }
    }
}

/// Wandelt den Anzeigepuffer in einen ABAP-Wert (Abschnitt 4.7). Leere Eingaben sind hier kein Fehler;
/// die Prüfung von `REQUIRED` übernimmt `input`.
///
/// Festlegungen (Annahmen, siehe Bericht):
/// - `n`: Leerzeichen entfallen, der Rest wird rechtsbündig mit Nullen aufgefüllt. Nur Ziffern erlaubt.
/// - `d`, `t`: Leerer Puffer ist initial (`00000000`, `000000`). Teilweise leere Puffer sind ungültig.
/// - `i`, `p`: Vorzeichen vorne oder hinten. Leerer Puffer ist 0. Nachkommastellen werden abgeschnitten, nicht gerundet.
pub fn parse(field: &Field, buf: &[char]) -> Result<Value, InputError> {
    let stripped: String = buf.iter().filter(|&&c| c != ' ').collect();
    match field.ty {
        AbapType::Char => Ok(Value::Text(buf.iter().map(|&c| c as u32 as u16).collect())),
        AbapType::Num => {
            if !stripped.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InputError::Number);
            }
            Ok(Value::Text(zero_filled(&stripped, field.length)))
        }
        AbapType::Date => {
            let text = if stripped.is_empty() {
                "00000000".to_string()
            } else {
                buf.iter().collect()
            };
            if text == "00000000" {
                return Ok(units_of(&text));
            }
            if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InputError::Date);
            }
            let year: u32 = text[0..4].parse().map_err(|_| InputError::Date)?;
            let month: u32 = text[4..6].parse().map_err(|_| InputError::Date)?;
            let day: u32 = text[6..8].parse().map_err(|_| InputError::Date)?;
            if !valid_date(year, month, day) {
                return Err(InputError::Date);
            }
            Ok(units_of(&text))
        }
        AbapType::Time => {
            let text = if stripped.is_empty() {
                "000000".to_string()
            } else {
                buf.iter().collect()
            };
            if text == "000000" {
                return Ok(units_of(&text));
            }
            if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InputError::Time);
            }
            let hour: u32 = text[0..2].parse().map_err(|_| InputError::Time)?;
            let minute: u32 = text[2..4].parse().map_err(|_| InputError::Time)?;
            let second: u32 = text[4..6].parse().map_err(|_| InputError::Time)?;
            if hour > 23 || minute > 59 || second > 59 {
                return Err(InputError::Time);
            }
            Ok(units_of(&text))
        }
        AbapType::Int => {
            if stripped.is_empty() {
                return Ok(Value::Int(0));
            }
            let (neg, body) = split_sign(&stripped);
            if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InputError::Number);
            }
            let magnitude: i64 = body.parse().map_err(|_| InputError::OutOfRange)?;
            let value = if neg { -magnitude } else { magnitude };
            i32::try_from(value)
                .map(Value::Int)
                .map_err(|_| InputError::OutOfRange)
        }
        AbapType::Packed => {
            if stripped.is_empty() {
                return Ok(Value::Packed(Dec::zero(field.decimals)));
            }
            let (neg, body) = split_sign(&stripped);
            let (int_part, frac_part) = body.split_once('.').unwrap_or((body, ""));
            if frac_part.contains('.') || (int_part.is_empty() && frac_part.is_empty()) {
                return Err(InputError::Number);
            }
            let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
            if !all_digits(int_part) || !all_digits(frac_part) {
                return Err(InputError::Number);
            }
            let digits: Vec<u8> = int_part
                .bytes()
                .chain(frac_part.bytes())
                .map(|b| b - b'0')
                .collect();
            let dec = Dec {
                neg,
                digits,
                scale: frac_part.len(),
            }
            .rescale(field.decimals);
            if dec.pack(field.length).is_err() {
                return Err(InputError::OutOfRange);
            }
            Ok(Value::Packed(dec))
        }
    }
}

/// Vorzeichen vorne oder hinten abtrennen. Gibt (negativ, Rest) zurück.
fn split_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_suffix('-') {
        (true, rest)
    } else {
        (false, s)
    }
}

/// Ziffern rechtsbündig in `len` Zeichen, links mit Nullen aufgefüllt.
fn zero_filled(digits: &str, len: usize) -> Vec<u16> {
    let mut units = vec![b'0' as u16; len.saturating_sub(digits.len())];
    units.extend(digits.bytes().map(|b| b as u16));
    units.truncate(len);
    units
}

/// ASCII-Text als UTF-16-Einheiten.
fn units_of(text: &str) -> Value {
    Value::Text(text.bytes().map(|b| b as u16).collect())
}

/// Gültiges Datum im gregorianischen Kalender. Das Jahr 0 ist ungültig.
fn valid_date(year: u32, month: u32, day: u32) -> bool {
    if year == 0 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::model::{VarPtr, INPUT, REQUIRED};

    /// Feld ohne Speicher. Nur für Konvertierungen, die den Speicher nicht berühren.
    fn field(ty: AbapType, length: usize, decimals: usize) -> Field {
        Field {
            name: "F".into(),
            row: 1,
            col: 1,
            ty,
            length,
            decimals,
            attrs: INPUT,
            data: VarPtr(std::ptr::null_mut()),
            width: display_width(ty, length, decimals),
        }
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn text_of(value: &Value) -> String {
        match value {
            Value::Text(units) => String::from_utf16_lossy(units),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn character_field_keeps_length_and_trailing_blanks() {
        let f = field(AbapType::Char, 6, 0);
        let v = parse(&f, &chars("AB    ")).unwrap();
        assert_eq!(text_of(&v), "AB    ");
        assert_eq!(format(&v, 6), chars("AB    "));
        assert!(is_initial(&f, &parse(&f, &chars("      ")).unwrap()));
        assert!(!is_initial(&f, &parse(&f, &chars("0     ")).unwrap()));
    }

    #[test]
    fn numeric_text_accepts_digits_only_and_fills_zeros() {
        let f = field(AbapType::Num, 10, 0);
        assert_eq!(
            text_of(&parse(&f, &chars("4711      ")).unwrap()),
            "0000004711"
        );
        assert_eq!(
            text_of(&parse(&f, &chars("47 11     ")).unwrap()),
            "0000004711"
        );
        assert_eq!(parse(&f, &chars("47A1      ")), Err(InputError::Number));
        assert!(is_initial(&f, &parse(&f, &chars("          ")).unwrap()));
        assert_eq!(format(&Value::Text(vec![b'1' as u16]), 3), chars("1  "));
    }

    #[test]
    fn numeric_typing_rejects_letters_and_blanks() {
        let f = field(AbapType::Num, 4, 0);
        let buf = chars("0000");
        assert!(accepts(&f, &buf, 0, '7'));
        assert!(!accepts(&f, &buf, 0, 'x'));
        assert!(!accepts(&f, &buf, 0, ' '));
    }

    #[test]
    fn date_accepts_valid_calendar_dates() {
        let f = field(AbapType::Date, 8, 0);
        assert_eq!(text_of(&parse(&f, &chars("20240229")).unwrap()), "20240229");
        assert_eq!(text_of(&parse(&f, &chars("00000000")).unwrap()), "00000000");
        assert!(is_initial(&f, &parse(&f, &chars("        ")).unwrap()));
    }

    #[test]
    fn date_rejects_invalid_dates_and_partial_input() {
        let f = field(AbapType::Date, 8, 0);
        for bad in [
            "20230229", "20231301", "20230001", "20231200", "00001231", "2023123 ", "2023",
        ] {
            let buf: Vec<char> = if bad.len() == 8 {
                chars(bad)
            } else {
                chars(&format!("{bad:<8}"))
            };
            assert_eq!(parse(&f, &buf), Err(InputError::Date), "input {bad:?}");
        }
    }

    #[test]
    fn time_accepts_valid_times_and_rejects_others() {
        let f = field(AbapType::Time, 6, 0);
        assert_eq!(text_of(&parse(&f, &chars("235959")).unwrap()), "235959");
        assert!(is_initial(&f, &parse(&f, &chars("000000")).unwrap()));
        assert_eq!(parse(&f, &chars("240000")), Err(InputError::Time));
        assert_eq!(parse(&f, &chars("126000")), Err(InputError::Time));
        assert_eq!(parse(&f, &chars("1200  ")), Err(InputError::Time));
    }

    #[test]
    fn integer_parses_sign_before_or_after_and_displays_right_aligned() {
        let f = field(AbapType::Int, 4, 0);
        assert_eq!(parse(&f, &chars("-42        ")), Ok(Value::Int(-42)));
        assert_eq!(parse(&f, &chars("42-       ")), Ok(Value::Int(-42)));
        assert_eq!(parse(&f, &chars("          ")), Ok(Value::Int(0)));
        assert_eq!(format(&Value::Int(-42), 11), chars("        -42"));
        assert_eq!(format(&Value::Int(7), 11).len(), INT_WIDTH);
    }

    #[test]
    fn integer_rejects_non_numbers_and_out_of_range_values() {
        let f = field(AbapType::Int, 4, 0);
        assert_eq!(parse(&f, &chars("-         ")), Err(InputError::Number));
        assert_eq!(parse(&f, &chars("1-2       ")), Err(InputError::Number));
        assert_eq!(parse(&f, &chars("2147483648")), Err(InputError::OutOfRange));
        assert_eq!(parse(&f, &chars("-2147483648")), Ok(Value::Int(i32::MIN)));
        assert_eq!(parse(&f, &chars("2147483647")), Ok(Value::Int(i32::MAX)));
    }

    #[test]
    fn integer_typing_allows_one_minus_and_digits_only() {
        let f = field(AbapType::Int, 4, 0);
        let buf = chars("1234");
        assert!(accepts(&f, &buf, 0, '-'));
        assert!(!accepts(&f, &buf, 0, '.'));
        assert!(!accepts(&f, &chars("-234"), 1, '-'));
        assert!(!accepts(&f, &buf, 0, 'a'));
    }

    #[test]
    fn packed_parses_decimals_and_truncates_extra_digits() {
        let f = field(AbapType::Packed, 4, 2);
        let v = parse(&f, &chars("12.345    ")).unwrap();
        // 4 Bytes: 7 Ziffern, dazu Dezimalpunkt und Vorzeichen, also Breite 9.
        assert_eq!(
            format(&v, display_width(AbapType::Packed, 4, 2)),
            chars("    12.34")
        );
        let neg = parse(&f, &chars("-1.5      ")).unwrap();
        assert_eq!(
            format(&neg, display_width(AbapType::Packed, 4, 2)),
            chars("    1.50-")
        );
    }

    #[test]
    fn packed_rejects_too_many_digits_and_bad_signs() {
        let f = field(AbapType::Packed, 2, 0);
        // 2 Bytes: 3 Ziffern.
        assert!(parse(&f, &chars("999      ")).is_ok());
        assert_eq!(parse(&f, &chars("9999     ")), Err(InputError::OutOfRange));
        assert_eq!(parse(&f, &chars("1.2.3    ")), Err(InputError::Number));
        assert_eq!(parse(&f, &chars("--1      ")), Err(InputError::Number));
    }

    #[test]
    fn packed_typing_allows_decimal_point_only_with_decimals() {
        let f0 = field(AbapType::Packed, 4, 0);
        let f2 = field(AbapType::Packed, 4, 2);
        assert!(!accepts(&f0, &chars("1   "), 1, '.'));
        assert!(accepts(&f2, &chars("1   "), 1, '.'));
        assert!(!accepts(&f2, &chars("1.  "), 2, '.'));
    }

    #[test]
    fn packed_width_follows_type_length() {
        assert_eq!(display_width(AbapType::Packed, 8, 2), 15 + 1 + 1);
        assert_eq!(display_width(AbapType::Packed, 1, 0), 2);
        assert_eq!(display_width(AbapType::Int, 4, 0), INT_WIDTH);
        assert_eq!(display_width(AbapType::Date, 8, 0), 8);
    }

    #[test]
    fn required_check_treats_blank_and_zero_as_initial() {
        let mut f = field(AbapType::Num, 3, 0);
        f.attrs |= REQUIRED;
        let zero = parse(&f, &chars("   ")).unwrap();
        assert!(is_initial(&f, &zero));
        assert!(!is_initial(&f, &parse(&f, &chars("001")).unwrap()));
    }

    #[test]
    fn memory_round_trip_for_each_type() {
        // Speicher je Typ, Initialwerte wie im Compiler (Zeichen als Leerzeichen, p mit 0x0C).
        let mut text = vec![0u8; 8];
        unsafe { write_units(text.as_mut_ptr(), 4, &[b'A' as u16, b'B' as u16]) };
        let f_char = Field {
            data: VarPtr(text.as_mut_ptr()),
            ..field(AbapType::Char, 4, 0)
        };
        let v = unsafe { read(&f_char) }.unwrap();
        assert_eq!(text_of(&v), "AB  ");
        unsafe { write(&f_char, &Value::Text(vec![b'Z' as u16])) }.unwrap();
        assert_eq!(text_of(&unsafe { read(&f_char) }.unwrap()), "Z   ");

        let mut int = (5i32).to_ne_bytes().to_vec();
        let f_int = Field {
            data: VarPtr(int.as_mut_ptr()),
            ..field(AbapType::Int, 4, 0)
        };
        unsafe { write(&f_int, &Value::Int(-9)) }.unwrap();
        assert_eq!(unsafe { read(&f_int) }.unwrap(), Value::Int(-9));

        let mut packed = vec![0x00, 0x0C];
        let f_pack = Field {
            data: VarPtr(packed.as_mut_ptr()),
            ..field(AbapType::Packed, 2, 1)
        };
        assert_eq!(
            unsafe { read(&f_pack) }.unwrap().clone(),
            Value::Packed(Dec::zero(1))
        );
        let dec = parse(&f_pack, &chars("12.3   ")).unwrap();
        unsafe { write(&f_pack, &dec) }.unwrap();
        assert_eq!(packed, vec![0x12, 0x3C]);
        assert_eq!(
            format(&unsafe { read(&f_pack) }.unwrap(), 5),
            chars(" 12.3")
        );
    }

    #[test]
    fn invalid_packed_memory_is_an_error_not_a_panic() {
        let mut bytes = vec![0x00, 0x00];
        let f = Field {
            data: VarPtr(bytes.as_mut_ptr()),
            ..field(AbapType::Packed, 2, 0)
        };
        assert_eq!(unsafe { read(&f) }, Err("invalid packed number"));
    }
}
