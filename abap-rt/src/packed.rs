//! Fehler beim Lesen und Schreiben gepackter Zahlen (DEC, ABAP-Typ P). Die Arithmetik steht in `decimal`.

#[derive(Debug, PartialEq, Eq)]
pub enum PackedError {
    /// Ungültiges Halbbyte: Ziffer über 9 oder unbekanntes Vorzeichen.
    InvalidNibble,
    /// Wert passt nicht in das Feld.
    Overflow,
}
