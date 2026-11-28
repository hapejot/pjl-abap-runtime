//! Umwandlung zwischen Hostvariablen, Rohwerten aus dem Adapter und ABAP-Feldern.
//!
//! Regeln (`doc/sql-spezifikation.md`, Abschnitte 4.4 und 11, dazu `decimal.rs` und `packed.rs`):
//! - Zeichenfelder sind UTF-16 in nativer Byte-Reihenfolge, 2 Byte je Einheit. Beim Lesen werden nachgestellte
//!   Leerzeichen entfernt, beim Schreiben wird rechts abgeschnitten oder mit Leerzeichen aufgefüllt.
//! - `NUMC` wird beim Schreiben links mit `0` auf die Feldlänge gebracht. Nicht-Ziffern sind ein Fehler.
//! - `NULL` wird zum Initialwert des Zielfelds.
//! - Gepackte Zahlen werden über `Dec` gelesen und geschrieben. Nachkommastellen werden abgeschnitten
//!   (wie `Dec::rescale`), Überlauf ist ein Fehler.
//! - `TYPE i` ist 4 Byte, Little Endian. Werte außerhalb von `i32` sind ein Fehler.

use abap_sql_ir::{AbapType, CharType};

use super::adapter::{ParamValue, RawValue};
use crate::decimal::Dec;

/// Leerzeichen als UTF-16-Einheit.
const BLANK: u16 = 0x20;

/// Fehler beim Schreiben eines Feldes. Der Aufrufer stellt die Meldung mit Zeile und Feldname zusammen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvError {
    /// Die Kombination aus Rohwert und Zieltyp ist in Phase 1 nicht vorgesehen.
    NotSupported,
    /// Der Wert passt nicht in das Feld (Format, Bereich, Überlauf).
    Failed(String),
}

/// Anzahl Bytes, die ein Feld dieses Typs im Zielbereich belegt.
pub fn field_size(ty: &AbapType) -> usize {
    match ty {
        AbapType::Int => 4,
        AbapType::Char { length, .. } => 2 * *length as usize,
        AbapType::Packed { bytes, .. } => *bytes as usize,
    }
}

/// Liest den Wert einer Hostvariable aus ihren Bytes (genau `field_size(ty)` Bytes).
pub fn host_value(bytes: &[u8], ty: &AbapType) -> Result<ParamValue, String> {
    match ty {
        AbapType::Int => {
            let raw: [u8; 4] = bytes
                .try_into()
                .map_err(|_| "integer host variable has wrong size".to_string())?;
            Ok(ParamValue::Int(i32::from_le_bytes(raw) as i64))
        }
        AbapType::Char { .. } => Ok(ParamValue::Text(char_text(bytes)?)),
        AbapType::Packed { scale, .. } => {
            let dec = Dec::unpack(bytes, *scale as usize)
                .map_err(|_| "invalid packed number".to_string())?;
            Ok(ParamValue::Text(dec_text(&dec)))
        }
    }
}

/// Text eines Zeichenfelds ohne nachgestellte Leerzeichen.
fn char_text(bytes: &[u8]) -> Result<String, String> {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_ne_bytes(*pair)).collect();
    let text = String::from_utf16(&units)
        .map_err(|_| "character host variable is not valid UTF-16".to_string())?;
    Ok(text.trim_end_matches(' ').to_string())
}

/// Wert eines Zeilenfelds für eine Schreibanweisung (ABAP-Feld in die Datenbank).
///
/// Wie beim Zuweisen an das Feld: `NUMC` wird links mit `0` auf die Feldlänge gebracht. Leer ergibt
/// nur Nullen, Nicht-Ziffern sind ein Fehler. Zeichenfelder verlieren die nachgestellten Leerzeichen, wie
/// beim Lesen. Gepackte Zahlen und ganze Zahlen wie in `host_value`.
pub fn row_value(bytes: &[u8], ty: &AbapType) -> Result<ParamValue, String> {
    match ty {
        AbapType::Char {
            length,
            dtype: CharType::Numc,
        } => {
            let text = char_text(bytes)?;
            if !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("'{text}' is not numeric for TYPE n"));
            }
            Ok(ParamValue::Text(format!(
                "{text:0>width$}",
                width = *length as usize
            )))
        }
        _ => host_value(bytes, ty),
    }
}

/// Schreibt einen Rohwert in das Feld `dst` (genau `field_size(ty)` Bytes).
pub fn write_field(dst: &mut [u8], ty: &AbapType, raw: &RawValue) -> Result<(), ConvError> {
    match ty {
        AbapType::Int => {
            let value = match raw {
                RawValue::Null => 0,
                RawValue::Int(v) => i32::try_from(*v)
                    .map_err(|_| ConvError::Failed(format!("value {v} out of range for TYPE i")))?,
                RawValue::Text(s) => parse_int(s).map_err(ConvError::Failed)?,
            };
            dst.copy_from_slice(&value.to_le_bytes());
            Ok(())
        }
        AbapType::Char { dtype, .. } => match raw {
            RawValue::Null => write_text(dst, initial_text(*dtype), *dtype),
            RawValue::Text(s) => write_text(dst, s, *dtype),
            RawValue::Int(_) => Err(ConvError::NotSupported),
        },
        AbapType::Packed { bytes, scale } => {
            let dec = match raw {
                RawValue::Null => Dec::zero(*scale as usize),
                RawValue::Int(v) => dec_from_int(*v),
                RawValue::Text(s) => parse_decimal(s).map_err(ConvError::Failed)?,
            };
            write_packed(dst, &dec, *bytes as usize, *scale as usize)
        }
    }
}

/// Initialwert eines Zeichenfelds bei `NULL`: `c` blank, `n` Nullen, `d` und `t` wie ABAP-Initial.
fn initial_text(dtype: CharType) -> &'static str {
    match dtype {
        CharType::Char => "",
        CharType::Numc => "",
        CharType::Dats => "00000000",
        CharType::Tims => "000000",
    }
}

/// Schreibt Text in ein Zeichenfeld. `NUMC` wird links mit `0` aufgefüllt, sonst rechts mit Leerzeichen.
fn write_text(dst: &mut [u8], text: &str, dtype: CharType) -> Result<(), ConvError> {
    let length = dst.len() / 2;
    let mut units: Vec<u16> = text.encode_utf16().collect();
    if dtype == CharType::Numc {
        let zero = b'0' as u16;
        if units.iter().any(|u| !(zero..=b'9' as u16).contains(u)) {
            return Err(ConvError::Failed(format!(
                "'{text}' is not numeric for TYPE n"
            )));
        }
        if units.len() < length {
            let mut padded = vec![zero; length - units.len()];
            padded.extend(units);
            units = padded;
        }
    }
    let (pairs, _) = dst.as_chunks_mut::<2>();
    for (index, pair) in pairs.iter_mut().enumerate() {
        let unit = units.get(index).copied().unwrap_or(BLANK);
        pair.copy_from_slice(&unit.to_ne_bytes());
    }
    Ok(())
}

/// Schreibt eine Dezimalzahl als gepackte Zahl mit `bytes` Bytes und Skala `scale`.
fn write_packed(dst: &mut [u8], dec: &Dec, bytes: usize, scale: usize) -> Result<(), ConvError> {
    if bytes == 0 {
        return Err(ConvError::Failed("invalid packed field length".to_string()));
    }
    let packed = dec
        .rescale(scale)
        .pack(bytes)
        .map_err(|_| ConvError::Failed("Overflow in packed number".to_string()))?;
    dst.copy_from_slice(&packed);
    Ok(())
}

/// Ganze Zahl aus Text (Leerzeichen am Rand erlaubt), Bereich von `TYPE i`.
pub fn parse_int(text: &str) -> Result<i32, String> {
    let trimmed = text.trim_matches(' ');
    let value: i64 = trimmed
        .parse()
        .map_err(|_| format!("'{trimmed}' is not a valid integer"))?;
    i32::try_from(value).map_err(|_| format!("value {value} out of range for TYPE i"))
}

/// Dezimalzahl aus Text: optionales Vorzeichen, Ziffern, optional `.` mit Nachkommaziffern.
/// Das Ergebnis hat die Skala der Nachkommaziffern.
pub fn parse_decimal(text: &str) -> Result<Dec, String> {
    let trimmed = text.trim_matches(' ');
    let (neg, rest) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let (int_part, frac_part) = rest.split_once('.').unwrap_or((rest, ""));
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (int_part.is_empty() && frac_part.is_empty())
        || !all_digits(int_part)
        || !all_digits(frac_part)
    {
        return Err(format!("'{trimmed}' is not a valid decimal number"));
    }
    let digits: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes())
        .map(|b| b - b'0')
        .collect();
    let scale = frac_part.len();
    // rescale auf die eigene Skala normalisiert führende Nullen und das Vorzeichen bei 0.
    Ok(Dec { neg, digits, scale }.rescale(scale))
}

/// Ganze Zahl als Dezimalzahl mit Skala 0.
fn dec_from_int(value: i64) -> Dec {
    let digits: Vec<u8> = value
        .unsigned_abs()
        .to_string()
        .bytes()
        .map(|b| b - b'0')
        .collect();
    Dec {
        neg: value < 0,
        digits,
        scale: 0,
    }
    .rescale(0)
}

/// Dezimaltext mit `.` und führendem Minuszeichen, exakt bis zur Skala (ohne Rundung).
pub fn dec_text(dec: &Dec) -> String {
    let mut digits: String = dec.digits.iter().map(|d| char::from(b'0' + d)).collect();
    if dec.scale > 0 {
        if digits.len() <= dec.scale {
            digits = format!("{digits:0>width$}", width = dec.scale + 1);
        }
        let (int_part, frac) = digits.split_at(digits.len() - dec.scale);
        digits = format!("{int_part}.{frac}");
    }
    if dec.neg {
        format!("-{digits}")
    } else {
        digits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn char_ty(length: u32, dtype: CharType) -> AbapType {
        AbapType::Char { length, dtype }
    }

    fn units_of(bytes: &[u8]) -> Vec<u16> {
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_ne_bytes(*p))
            .collect()
    }

    fn write(ty: &AbapType, raw: &RawValue) -> Result<Vec<u8>, ConvError> {
        let mut buf = vec![0xAAu8; field_size(ty)];
        write_field(&mut buf, ty, raw)?;
        Ok(buf)
    }

    fn text_of(bytes: &[u8]) -> String {
        String::from_utf16(&units_of(bytes)).expect("valid UTF-16")
    }

    #[test]
    fn char_value_shorter_than_field_is_padded_with_blanks() {
        let ty = char_ty(3, CharType::Char);
        let bytes = write(&ty, &RawValue::Text("AB".into())).unwrap();
        assert_eq!(text_of(&bytes), "AB ");
    }

    #[test]
    fn char_value_longer_than_field_is_cut_on_the_right() {
        let ty = char_ty(2, CharType::Char);
        let bytes = write(&ty, &RawValue::Text("ABCD".into())).unwrap();
        assert_eq!(text_of(&bytes), "AB");
    }

    #[test]
    fn numc_is_padded_with_zeros_on_the_left() {
        let ty = char_ty(4, CharType::Numc);
        let bytes = write(&ty, &RawValue::Text("12".into())).unwrap();
        assert_eq!(text_of(&bytes), "0012");
    }

    #[test]
    fn numc_with_non_digit_is_rejected() {
        let ty = char_ty(4, CharType::Numc);
        let err = write(&ty, &RawValue::Text("1A".into())).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("not numeric")));
    }

    #[test]
    fn null_becomes_blank_for_char_and_zero_for_numc() {
        let char_bytes = write(&char_ty(3, CharType::Char), &RawValue::Null).unwrap();
        assert_eq!(text_of(&char_bytes), "   ");
        let numc_bytes = write(&char_ty(3, CharType::Numc), &RawValue::Null).unwrap();
        assert_eq!(text_of(&numc_bytes), "000");
    }

    #[test]
    fn null_becomes_abap_initial_for_dats_and_tims() {
        let dats = write(&char_ty(8, CharType::Dats), &RawValue::Null).unwrap();
        assert_eq!(text_of(&dats), "00000000");
        let tims = write(&char_ty(6, CharType::Tims), &RawValue::Null).unwrap();
        assert_eq!(text_of(&tims), "000000");
    }

    #[test]
    fn null_becomes_zero_for_int_and_packed() {
        let int_bytes = write(&AbapType::Int, &RawValue::Null).unwrap();
        assert_eq!(int_bytes, vec![0, 0, 0, 0]);
        let packed = AbapType::Packed { bytes: 3, scale: 2 };
        let bytes = write(&packed, &RawValue::Null).unwrap();
        assert_eq!(Dec::unpack(&bytes, 2).unwrap().format(), "0.00");
    }

    #[test]
    fn int_from_text_parses_and_rejects_non_numbers() {
        let bytes = write(&AbapType::Int, &RawValue::Text(" 42 ".into())).unwrap();
        assert_eq!(bytes, 42i32.to_le_bytes().to_vec());
        let err = write(&AbapType::Int, &RawValue::Text("abc".into())).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("not a valid integer")));
    }

    #[test]
    fn int_overflow_is_an_error_for_text_and_integer_sources() {
        let err = write(&AbapType::Int, &RawValue::Text("3000000000".into())).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("out of range for TYPE i")));
        let err = write(&AbapType::Int, &RawValue::Int(i64::from(i32::MAX) + 1)).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("out of range for TYPE i")));
    }

    #[test]
    fn integer_source_into_char_target_is_not_supported() {
        let err = write(&char_ty(4, CharType::Char), &RawValue::Int(5)).unwrap_err();
        assert_eq!(err, ConvError::NotSupported);
    }

    #[test]
    fn packed_target_truncates_decimals_like_decimal_rescale() {
        let ty = AbapType::Packed { bytes: 3, scale: 2 };
        let bytes = write(&ty, &RawValue::Text("12.345".into())).unwrap();
        assert_eq!(Dec::unpack(&bytes, 2).unwrap().format(), "12.34");
    }

    #[test]
    fn packed_target_keeps_sign() {
        let ty = AbapType::Packed { bytes: 3, scale: 2 };
        let bytes = write(&ty, &RawValue::Text("-7.5".into())).unwrap();
        assert_eq!(dec_text(&Dec::unpack(&bytes, 2).unwrap()), "-7.50");
    }

    #[test]
    fn packed_target_overflow_is_an_error() {
        // 2 Bytes ergeben 3 Ziffern, 4 Ziffern passen nicht.
        let ty = AbapType::Packed { bytes: 2, scale: 0 };
        let err = write(&ty, &RawValue::Text("1234".into())).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("Overflow in packed number")));
    }

    #[test]
    fn packed_target_rejects_text_that_is_not_a_number() {
        let ty = AbapType::Packed { bytes: 3, scale: 2 };
        let err = write(&ty, &RawValue::Text("12,5".into())).unwrap_err();
        assert!(matches!(err, ConvError::Failed(m) if m.contains("not a valid decimal")));
    }

    #[test]
    fn host_char_drops_trailing_blanks() {
        let bytes: Vec<u8> = "LH  "
            .encode_utf16()
            .flat_map(|u| u.to_ne_bytes())
            .collect();
        let value = host_value(&bytes, &char_ty(4, CharType::Char)).unwrap();
        assert_eq!(value, ParamValue::Text("LH".into()));
    }

    #[test]
    fn host_int_is_little_endian() {
        let bytes = (-5i32).to_le_bytes();
        let value = host_value(&bytes, &AbapType::Int).unwrap();
        assert_eq!(value, ParamValue::Int(-5));
    }

    #[test]
    fn host_packed_is_exact_decimal_text() {
        // -1.25 mit 2 Bytes (3 Ziffern und Vorzeichen): Nibbles 1, 2, 5 und Vorzeichen Minus (0x0D).
        let bytes = [0x12u8, 0x5D];
        let ty = AbapType::Packed { bytes: 2, scale: 2 };
        let value = host_value(&bytes, &ty).unwrap();
        assert_eq!(value, ParamValue::Text("-1.25".into()));
    }

    #[test]
    fn host_packed_keeps_all_scale_digits() {
        let mut dst = vec![0u8; 3];
        write_packed(&mut dst, &dec_from_int(0), 3, 2).unwrap();
        let value = host_value(&dst, &AbapType::Packed { bytes: 3, scale: 2 }).unwrap();
        assert_eq!(value, ParamValue::Text("0.00".into()));
    }

    #[test]
    fn decimal_text_of_small_fraction_has_leading_zero() {
        let dec = parse_decimal("0.05").unwrap();
        assert_eq!(dec_text(&dec), "0.05");
    }

    #[test]
    fn field_sizes_follow_the_abap_type() {
        assert_eq!(field_size(&AbapType::Int), 4);
        assert_eq!(field_size(&char_ty(3, CharType::Char)), 6);
        assert_eq!(field_size(&AbapType::Packed { bytes: 5, scale: 0 }), 5);
    }

    fn units_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_ne_bytes()).collect()
    }

    #[test]
    fn row_value_of_numc_is_left_padded_with_zeros_to_the_field_length() {
        // NUMC(4) mit Inhalt "12  ": nachgestellte Leerzeichen fallen weg, dann links auf 4 Stellen mit 0.
        let value = row_value(&units_bytes("12  "), &char_ty(4, CharType::Numc)).unwrap();
        assert_eq!(value, ParamValue::Text("0012".into()));
    }

    #[test]
    fn row_value_of_empty_numc_is_all_zeros() {
        let value = row_value(&units_bytes("    "), &char_ty(3, CharType::Numc)).unwrap();
        assert_eq!(value, ParamValue::Text("000".into()));
    }

    #[test]
    fn row_value_of_numc_with_letters_is_rejected() {
        let err = row_value(&units_bytes("1A  "), &char_ty(4, CharType::Numc)).unwrap_err();
        assert_eq!(err, "'1A' is not numeric for TYPE n");
    }

    #[test]
    fn row_value_of_char_drops_trailing_blanks_like_reading_does() {
        let value = row_value(&units_bytes("LH  "), &char_ty(4, CharType::Char)).unwrap();
        assert_eq!(value, ParamValue::Text("LH".into()));
        // Führende Leerzeichen bleiben erhalten.
        let leading = row_value(&units_bytes(" L "), &char_ty(3, CharType::Char)).unwrap();
        assert_eq!(leading, ParamValue::Text(" L".into()));
    }

    #[test]
    fn row_value_of_int_and_packed_is_the_host_value() {
        let int = row_value(&(42i32).to_le_bytes(), &AbapType::Int).unwrap();
        assert_eq!(int, ParamValue::Int(42));
        let packed_bytes = [0x12u8, 0x5D];
        let packed = row_value(&packed_bytes, &AbapType::Packed { bytes: 2, scale: 2 }).unwrap();
        assert_eq!(packed, ParamValue::Text("-1.25".into()));
    }
}
