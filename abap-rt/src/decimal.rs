//! Dezimalarithmetik beliebiger Länge auf Ziffernbasis (ABAP DEC / P).
//!
//! Eine Zahl ist Vorzeichen, Ziffern (wichtigste zuerst, je 0..=9) und Skala (Nachkommastellen).
//! Der Wert ist `Ziffern × 10^-Skala`. Gerechnet wird ausschließlich mit Ziffern, ohne Binärdarstellung.
//! Die Speicherform (gepacktes BCD) wird direkt gelesen und geschrieben.

use std::cmp::Ordering;

use crate::packed::PackedError;

/// Zusätzliche Nachkommastellen, die eine Division über die größere Eingabeskala hinaus berechnet.
const DIV_EXTRA_SCALE: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dec {
    pub neg: bool,
    /// Ziffern ohne führende Nullen (mindestens eine Ziffer, bei 0 genau `[0]`).
    pub digits: Vec<u8>,
    pub scale: usize,
}

impl Dec {
    /// Null mit der angegebenen Skala.
    pub fn zero(scale: usize) -> Dec {
        Dec {
            neg: false,
            digits: vec![0],
            scale,
        }
    }

    /// Entfernt führende Nullen und setzt das Vorzeichen bei 0 auf positiv.
    fn normalized(mut self) -> Dec {
        let first = self
            .digits
            .iter()
            .position(|&d| d != 0)
            .unwrap_or(self.digits.len());
        if first == self.digits.len() {
            self.digits = vec![0];
        } else {
            self.digits.drain(..first);
        }
        if self.digits == [0] {
            self.neg = false;
        }
        self
    }

    /// Liest eine gepackte Zahl mit `bytes` Bytes und der Skala `scale`.
    pub fn unpack(bytes: &[u8], scale: usize) -> Result<Dec, PackedError> {
        let Some((last, head)) = bytes.split_last() else {
            return Ok(Dec::zero(scale));
        };
        let mut digits = Vec::with_capacity(bytes.len() * 2 - 1);
        for byte in head {
            digits.push(byte >> 4);
            digits.push(byte & 0x0F);
        }
        digits.push(last >> 4);
        if digits.iter().any(|&d| d > 9) {
            return Err(PackedError::InvalidNibble);
        }
        let neg = match last & 0x0F {
            0x0A | 0x0C | 0x0E | 0x0F => false,
            0x0B | 0x0D => true,
            _ => return Err(PackedError::InvalidNibble),
        };
        Ok(Dec { neg, digits, scale }.normalized())
    }

    /// Schreibt die Zahl in `len` Bytes. Die Skala muss bereits stimmen (siehe `rescale`).
    pub fn pack(&self, len: usize) -> Result<Vec<u8>, PackedError> {
        let capacity = 2 * len - 1;
        let value = self.clone().normalized();
        if value.digits.len() > capacity {
            return Err(PackedError::Overflow);
        }
        let mut nibbles = vec![0u8; 2 * len];
        let offset = 2 * len - 1 - value.digits.len();
        nibbles[offset..2 * len - 1].copy_from_slice(&value.digits);
        nibbles[2 * len - 1] = if value.neg { 0x0D } else { 0x0C };
        Ok(nibbles
            .chunks(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect())
    }

    /// Bringt die Zahl auf die Skala `scale`: Nullen anhängen oder Nachkommaziffern weglassen (ohne Runden).
    pub fn rescale(&self, scale: usize) -> Dec {
        let mut digits = self.digits.clone();
        if scale > self.scale {
            digits.extend(std::iter::repeat(0).take(scale - self.scale));
        } else {
            let drop = self.scale - scale;
            let keep = digits.len().saturating_sub(drop);
            digits.truncate(keep);
            if digits.is_empty() {
                digits.push(0);
            }
        }
        Dec {
            neg: self.neg,
            digits,
            scale,
        }
        .normalized()
    }

    /// Summe. Das Ergebnis hat die größere der beiden Skalen.
    pub fn add(&self, other: &Dec) -> Dec {
        let scale = self.scale.max(other.scale);
        let a = self.rescale(scale);
        let b = other.rescale(scale);
        if a.neg == b.neg {
            Dec {
                neg: a.neg,
                digits: mag_add(&a.digits, &b.digits),
                scale,
            }
            .normalized()
        } else {
            match mag_cmp(&a.digits, &b.digits) {
                Ordering::Less => Dec {
                    neg: b.neg,
                    digits: mag_sub(&b.digits, &a.digits),
                    scale,
                }
                .normalized(),
                _ => Dec {
                    neg: a.neg,
                    digits: mag_sub(&a.digits, &b.digits),
                    scale,
                }
                .normalized(),
            }
        }
    }

    /// Differenz `self - other`.
    pub fn sub(&self, other: &Dec) -> Dec {
        let mut negated = other.clone();
        negated.neg = !negated.neg;
        self.add(&negated)
    }

    /// Produkt. Die Skala ist die Summe der Skalen; die Ziffern werden nicht gekürzt.
    pub fn mul(&self, other: &Dec) -> Dec {
        Dec {
            neg: self.neg != other.neg,
            digits: mag_mul(&self.digits, &other.digits),
            scale: self.scale + other.scale,
        }
        .normalized()
    }

    /// Quotient. Das Ergebnis hat `DIV_EXTRA_SCALE` Nachkommastellen mehr als die größere Skala;
    /// das Abschneiden auf die Zielskala passiert beim Speichern. Division durch 0 ergibt `None`.
    pub fn div(&self, other: &Dec) -> Option<Dec> {
        if other.digits == [0] {
            return None;
        }
        let scale = self.scale.max(other.scale) + DIV_EXTRA_SCALE;
        let shift = scale + other.scale - self.scale;
        let mut numerator = self.digits.clone();
        numerator.extend(std::iter::repeat(0).take(shift));
        let digits = mag_div(&numerator, &other.digits);
        Some(
            Dec {
                neg: self.neg != other.neg,
                digits,
                scale,
            }
            .normalized(),
        )
    }

    /// Rest wie ABAP `MOD`: `self - other * trunc(self / other)`. Das Vorzeichen folgt dem Dividenden.
    /// Der Quotient ist exakt, weil die Division genug Nachkommastellen liefert. Division durch 0 ergibt `None`.
    pub fn rem(&self, other: &Dec) -> Option<Dec> {
        let quotient = self.div(other)?.rescale(0);
        Some(self.sub(&other.mul(&quotient)))
    }

    /// Vergleich des Zahlenwerts (unabhängig von der Skala).
    pub fn cmp_value(&self, other: &Dec) -> Ordering {
        let scale = self.scale.max(other.scale);
        let a = self.rescale(scale);
        let b = other.rescale(scale);
        match (a.neg, b.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => mag_cmp(&a.digits, &b.digits),
            (true, true) => mag_cmp(&b.digits, &a.digits),
        }
    }

    /// Text wie bei `WRITE`: Dezimalpunkt bei Nachkommastellen, Vorzeichen nachgestellt.
    pub fn format(&self) -> String {
        let mut digits: String = self.digits.iter().map(|d| (b'0' + d) as char).collect();
        if self.scale > 0 {
            if digits.len() <= self.scale {
                digits = format!("{digits:0>width$}", width = self.scale + 1);
            }
            let (int_part, frac) = digits.split_at(digits.len() - self.scale);
            digits = format!("{int_part}.{frac}");
        }
        if self.neg {
            digits.push('-');
        }
        digits
    }
}

/// Vergleich zweier Ziffernfolgen ohne führende Nullen.
fn mag_cmp(a: &[u8], b: &[u8]) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Summe zweier Ziffernfolgen.
fn mag_add(a: &[u8], b: &[u8]) -> Vec<u8> {
    let n = a.len().max(b.len());
    let mut out = vec![0u8; n + 1];
    let mut carry = 0u8;
    for k in 0..n {
        let x = if k < a.len() { a[a.len() - 1 - k] } else { 0 };
        let y = if k < b.len() { b[b.len() - 1 - k] } else { 0 };
        let s = x + y + carry;
        out[n - k] = s % 10;
        carry = s / 10;
    }
    out[0] = carry;
    out
}

/// Differenz zweier Ziffernfolgen mit `a >= b`.
fn mag_sub(a: &[u8], b: &[u8]) -> Vec<u8> {
    let n = a.len();
    let mut out = vec![0u8; n];
    let mut borrow = 0i8;
    for k in 0..n {
        let x = a[n - 1 - k] as i8;
        let y = if k < b.len() {
            b[b.len() - 1 - k] as i8
        } else {
            0
        };
        let mut d = x - y - borrow;
        if d < 0 {
            d += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[n - 1 - k] = d as u8;
    }
    out
}

/// Langdivision zweier Ziffernfolgen (ganzzahlig, `den` ungleich 0).
/// Jede Ziffer des Zählers wird an den Rest angehängt, dann wird der Divisor so oft abgezogen, wie er passt.
fn mag_div(num: &[u8], den: &[u8]) -> Vec<u8> {
    let mut quotient = Vec::with_capacity(num.len());
    let mut rem: Vec<u8> = vec![0];
    for &digit in num {
        if rem == [0] {
            rem = vec![digit];
        } else {
            rem.push(digit);
        }
        let mut q = 0u8;
        while mag_cmp(&rem, den) != Ordering::Less {
            rem = strip_zeros(mag_sub(&rem, den));
            q += 1;
        }
        quotient.push(q);
    }
    strip_zeros(quotient)
}

/// Entfernt führende Nullen (bei reiner Null bleibt `[0]`).
fn strip_zeros(digits: Vec<u8>) -> Vec<u8> {
    match digits.iter().position(|&d| d != 0) {
        Some(first) => digits[first..].to_vec(),
        None => vec![0],
    }
}

/// Schulmethode für das Produkt zweier Ziffernfolgen.
fn mag_mul(a: &[u8], b: &[u8]) -> Vec<u8> {
    let (n, m) = (a.len(), b.len());
    let mut acc = vec![0u32; n + m];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            acc[i + j + 1] += x as u32 * y as u32;
        }
    }
    for k in (1..n + m).rev() {
        acc[k - 1] += acc[k] / 10;
        acc[k] %= 10;
    }
    acc.into_iter().map(|d| d as u8).collect()
}

// ---- C-ABI für den generierten Code. Werte sind Handles auf `Dec` (kein GC, wie bei Strings).

fn fail(message: &str) -> ! {
    let _ = std::io::Write::flush(&mut std::io::stdout());
    eprintln!("Runtime error: {message}");
    std::process::exit(1)
}

thread_local! {
    /// Alle Zwischenwerte, die seit dem letzten `release` entstanden sind.
    static TEMPS: std::cell::RefCell<Vec<*mut Dec>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn handle(value: Dec) -> *mut Dec {
    let p = Box::into_raw(Box::new(value));
    TEMPS.with(|t| t.borrow_mut().push(p));
    p
}

/// Marke: Anzahl der Zwischenwerte im Register. Jede Anweisung nimmt sich eine Marke.
#[no_mangle]
pub extern "C" fn abap_rt_dec_mark() -> i64 {
    TEMPS.with(|t| t.borrow().len() as i64)
}

/// Gibt alle Zwischenwerte oberhalb der Marke frei. Werte in Speicherfeldern sind davon nicht betroffen,
/// weil sie beim Speichern als gepackte Bytes kopiert wurden.
#[no_mangle]
pub extern "C" fn abap_rt_dec_release(mark: i64) {
    TEMPS.with(|t| {
        let mut temps = t.borrow_mut();
        while temps.len() > mark.max(0) as usize {
            if let Some(p) = temps.pop() {
                // SAFETY: jeder Zeiger wurde in `handle` über Box::into_raw erzeugt und nur hier freigegeben.
                unsafe { drop(Box::from_raw(p)) };
            }
        }
    });
}

/// Liest das Handle. Die Handles stammen ausschließlich aus diesem Modul.
unsafe fn value<'a>(h: *const Dec) -> &'a Dec {
    &*h
}

/// Gepacktes Feld mit `len` Bytes und Skala `scale` als Handle lesen.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_unpack(ptr: *const u8, len: u64, scale: u64) -> *mut Dec {
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    match Dec::unpack(bytes, scale as usize) {
        Ok(d) => handle(d),
        Err(PackedError::InvalidNibble) => fail("invalid packed number"),
        Err(PackedError::Overflow) => fail("Overflow in packed number"),
    }
}

/// Handle in das Feld mit `len` Bytes und Skala `scale` schreiben (Nachkommastellen werden abgeschnitten).
#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_pack(h: *const Dec, ptr: *mut u8, len: u64, scale: u64) {
    let d = value(h).rescale(scale as usize);
    if len == 0 {
        fail("Overflow in packed number");
    }
    match d.pack(len as usize) {
        Ok(bytes) => std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()),
        Err(_) => fail("Overflow in packed number"),
    }
}

/// Ganze Zahl (Skala 0) als Handle.
#[no_mangle]
pub extern "C" fn abap_rt_dec_from_int(v: i32) -> *mut Dec {
    let digits: Vec<u8> = v
        .unsigned_abs()
        .to_string()
        .bytes()
        .map(|b| b - b'0')
        .collect();
    handle(
        Dec {
            neg: v < 0,
            digits,
            scale: 0,
        }
        .normalized(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_add(a: *const Dec, b: *const Dec) -> *mut Dec {
    handle(value(a).add(value(b)))
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_sub(a: *const Dec, b: *const Dec) -> *mut Dec {
    handle(value(a).sub(value(b)))
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_mul(a: *const Dec, b: *const Dec) -> *mut Dec {
    handle(value(a).mul(value(b)))
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_div(a: *const Dec, b: *const Dec) -> *mut Dec {
    match value(a).div(value(b)) {
        Some(d) => handle(d),
        None => fail("division by 0"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_mod(a: *const Dec, b: *const Dec) -> *mut Dec {
    match value(a).rem(value(b)) {
        Some(d) => handle(d),
        None => fail("division by 0"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_neg(a: *const Dec) -> *mut Dec {
    let mut d = value(a).clone();
    d.neg = !d.neg;
    handle(d.normalized())
}

/// Vergleich: -1, 0 oder 1.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_cmp(a: *const Dec, b: *const Dec) -> i32 {
    match value(a).cmp_value(value(b)) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Ganzzahliger Wert (Nachkommastellen fallen weg). Passt der Wert nicht in `i32`, bricht das Programm ab.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_to_int(a: *const Dec) -> i32 {
    let d = value(a).rescale(0);
    if d.digits.len() > 10 {
        fail("Overflow converting to integer");
    }
    let magnitude = d.digits.iter().fold(0i64, |acc, &x| acc * 10 + x as i64);
    if magnitude > i32::MAX as i64 + 1 || (!d.neg && magnitude > i32::MAX as i64) {
        fail("Overflow converting to integer");
    }
    if d.neg {
        (-magnitude) as i32
    } else {
        magnitude as i32
    }
}

/// Ausgabe wie `WRITE`, rechtsbündig in `width` Zeichen.
#[no_mangle]
pub unsafe extern "C" fn abap_rt_dec_write(a: *const Dec, width: u64) {
    let text = value(a).format();
    crate::out(format!("{:>w$}", text, w = width as usize).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_truncates_at_target_scale() {
        assert_eq!(
            dec("1", 0).div(&dec("3", 0)).unwrap().rescale(2).format(),
            "0.33"
        );
        assert_eq!(
            dec("10", 0).div(&dec("4", 0)).unwrap().rescale(1).format(),
            "2.5"
        );
        assert_eq!(
            dec("-7", 0).div(&dec("2", 0)).unwrap().rescale(1).format(),
            "3.5-"
        );
    }

    #[test]
    fn division_with_scales_and_large_values() {
        assert_eq!(
            dec("150", 2)
                .div(&dec("15", 1))
                .unwrap()
                .rescale(0)
                .format(),
            "1"
        );
        let big = dec(&format!("1{}", "0".repeat(30)), 0);
        let small = dec(&format!("1{}", "0".repeat(10)), 0);
        assert_eq!(
            big.div(&small).unwrap().rescale(0).format(),
            format!("1{}", "0".repeat(20))
        );
    }

    #[test]
    fn modulo_follows_dividend_sign() {
        assert_eq!(dec("725", 1).rem(&dec("2", 0)).unwrap().format(), "0.5"); // 72.5 MOD 2
        assert_eq!(dec("-725", 1).rem(&dec("2", 0)).unwrap().format(), "0.5-"); // -72.5 MOD 2
        assert_eq!(dec("10", 0).rem(&dec("5", 0)).unwrap().format(), "0");
        assert_eq!(dec("7", 0).rem(&dec("0", 0)), None);
    }

    #[test]
    fn release_frees_only_temporaries_above_mark() {
        let outer = abap_rt_dec_from_int(1);
        let mark = abap_rt_dec_mark();
        let _inner = abap_rt_dec_from_int(2);
        assert_eq!(abap_rt_dec_mark(), mark + 1);
        abap_rt_dec_release(mark);
        assert_eq!(abap_rt_dec_mark(), mark);
        // das äußere Handle ist weiter gültig
        assert_eq!(unsafe { abap_rt_dec_to_int(outer) }, 1);
        abap_rt_dec_release(0);
    }

    #[test]
    fn division_by_zero_is_none() {
        assert!(dec("5", 0).div(&dec("0", 2)).is_none());
    }

    fn dec(text: &str, scale: usize) -> Dec {
        let digits: Vec<u8> = text
            .bytes()
            .filter(|b| b.is_ascii_digit())
            .map(|b| b - b'0')
            .collect();
        let neg = text.starts_with('-');
        Dec { neg, digits, scale }.normalized()
    }

    #[test]
    fn unpack_and_pack_round_trip() {
        let bytes = Dec::unpack(&[0x12, 0x34, 0x5C], 2).unwrap();
        assert_eq!(bytes.format(), "123.45");
        let neg = dec("-1250", 2);
        let packed = neg.pack(3).unwrap();
        assert_eq!(packed, vec![0x01, 0x25, 0x0D]);
    }

    #[test]
    fn pack_rejects_overflow() {
        assert_eq!(dec("123456", 0).pack(3), Err(PackedError::Overflow));
        assert!(dec("12345", 0).pack(3).is_ok());
    }

    #[test]
    fn addition_aligns_scales() {
        let r = dec("150", 2).add(&dec("25", 1));
        assert_eq!(r.format(), "4.00");
    }

    #[test]
    fn subtraction_changes_sign() {
        let r = dec("100", 2).sub(&dec("250", 2));
        assert_eq!(r.format(), "1.50-");
    }

    #[test]
    fn subtraction_to_zero_is_unsigned() {
        let r = dec("250", 2).sub(&dec("250", 2));
        assert_eq!(r.format(), "0.00");
    }

    #[test]
    fn multiplication_adds_scales() {
        let r = dec("125", 1).mul(&dec("2", 0));
        assert_eq!(r.format(), "25.0");
        let s = dec("150", 2).mul(&dec("-2", 0));
        assert_eq!(s.format(), "3.00-");
    }

    #[test]
    fn multiplication_beyond_i64() {
        let nines = dec(&"9".repeat(20), 0);
        let r = nines.mul(&nines);
        assert_eq!(
            r.format(),
            "99999999999999999998".to_string() + "00000000000000000001"
        );
    }

    #[test]
    fn rescale_truncates_without_rounding() {
        assert_eq!(dec("1299", 3).rescale(1).format(), "1.2");
        assert_eq!(dec("5", 0).rescale(2).format(), "5.00");
    }

    #[test]
    fn compares_across_scales_and_signs() {
        assert_eq!(dec("150", 2).cmp_value(&dec("15", 1)), Ordering::Equal);
        assert_eq!(dec("-1", 0).cmp_value(&dec("0", 0)), Ordering::Less);
        assert_eq!(dec("-5", 0).cmp_value(&dec("-3", 0)), Ordering::Less);
    }
}
