//! SQL-Zwischenstruktur (IR) für Open-SQL-Anweisungen (`doc/sql-spezifikation.md`, Abschnitt 5).
//!
//! Der Compiler erzeugt je Datenbankanweisung genau eine `SqlRequest` und legt deren JSON-Text als
//! Konstante ins Programm. Die Laufzeit (`abap-rt`) dekodiert sie und gibt sie an einen Adapter weiter.
//! Die IR enthält keinen SQL-Text und keine Hostwerte. Dieses Crate hat keine Laufzeitabhängigkeiten.
//!
//! Diese Fassung deckt Phase 2 ab: `SELECT` (Einzelsatz, Liste, Cursor), `INSERT`, `UPDATE`, `DELETE`,
//! `MODIFY`, `COMMIT WORK` und `ROLLBACK WORK`.
//!
//! Konvention für Zeilenquellen: Die Werte der Zeilenfelder werden nach den Hostparametern `params` der
//! Anweisung angehängt, in der Reihenfolge von `RowSource::fields`. Hat die Anweisung `n` Hostparameter,
//! ist das Feld `k` der Zeile der Parameter `n + k`.

use serde::{Deserialize, Serialize};

/// Version des IR-Formats. Compiler und Laufzeit prüfen sie beim Dekodieren.
pub const IR_VERSION: u32 = 1;

/// Eine Datenbankanweisung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SqlRequest {
    /// Version des Formats, muss `IR_VERSION` sein
    pub ir_version: u32,
    /// Quelltextzeile, für Fehlermeldungen
    pub line: u32,
    /// Verbindungsname; in Phase 1 immer `"DEFAULT"` (der Adapter wird über die Tabelle gewählt)
    pub connection: String,
    pub op: SqlOp,
}

/// Art der Anweisung. Phase 2: `SELECT`, die Schreibanweisungen und `COMMIT`/`ROLLBACK WORK`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum SqlOp {
    Select(SelectOp),
    Insert(InsertOp),
    Update(UpdateOp),
    Delete(DeleteOp),
    Modify(ModifyOp),
    /// `COMMIT WORK`: schreibt die offenen LUWs aller Verbindungen fest.
    Commit,
    /// `ROLLBACK WORK`: verwirft die offenen LUWs aller Verbindungen.
    Rollback,
}

/// Zeilenquelle einer Schreibanweisung (`FROM wa` oder `FROM TABLE itab`).
///
/// Die Werte stehen im ABAP-Zielbereich des Aufrufs (Arbeitsbereich bei `Single`, Zeilen einer internen
/// Tabelle bei `Table`). Jedes Feld hat Spalte, ABAP-Typ und Offset im Zeilenpuffer der Größe `size`.
/// Der Kern liest die Werte und hängt sie als Parameter an die Hostparameter der Anweisung an:
/// die Werte der Felder stehen nach `params` in der Reihenfolge von `fields`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowSource {
    pub mode: RowMode,
    pub fields: Vec<RowField>,
    /// Größe einer Zeile in Bytes
    pub size: u64,
}

/// Ob die Zeilenquelle ein Arbeitsbereich oder eine interne Tabelle ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowMode {
    Single,
    Table,
}

/// Ein Feld einer Zeilenquelle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowField {
    /// DDIC-Name der Spalte (Kleinbuchstaben)
    pub column: String,
    /// ABAP-Typ des Feldes im Zeilenpuffer
    pub ty: AbapType,
    /// Byte-Offset im Zeilenpuffer
    pub offset: u64,
}

/// `INSERT INTO tab VALUES wa`, `INSERT tab FROM wa`, `INSERT tab FROM TABLE itab`.
/// Ohne `accepting_duplicate_keys` ist eine Dublette ein Fehler; bei der Einzelzeile ist es `SY-SUBRC = 4`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertOp {
    pub table: String,
    pub rows: RowSource,
    /// `ACCEPTING DUPLICATE KEYS`: Dubletten werden übersprungen und ergeben `SY-SUBRC = 4`
    pub accepting_duplicate_keys: bool,
}

/// `UPDATE tab SET spalte = wert, ... [WHERE ...]` (Form `SET`) oder `UPDATE tab FROM wa` (Form `FROM`,
/// Einzelsatz nach Schlüssel). Die Form `FROM` hat `rows`, `assignments` und `filter` sind dann leer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateOp {
    pub table: String,
    pub assignments: Vec<Assignment>,
    pub filter: Option<Expr>,
    pub rows: Option<RowSource>,
    /// Hostvariablen der Anweisung (ohne die Werte der Zeilenquelle)
    pub params: Vec<Param>,
}

/// `DELETE FROM tab [WHERE ...]`, `DELETE tab FROM wa` (nach Schlüssel), `DELETE FROM TABLE itab`
/// (nach Schlüssel je Zeile). `rows` gesetzt bedeutet Schlüsselform, dann ist `filter` leer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteOp {
    pub table: String,
    pub filter: Option<Expr>,
    pub rows: Option<RowSource>,
    pub params: Vec<Param>,
}

/// `MODIFY tab FROM wa` bzw. `MODIFY tab FROM TABLE itab`: Einfügen oder Ändern nach Schlüssel. Der
/// Schlüssel kommt aus der Konfiguration (`key` der Tabelle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModifyOp {
    pub table: String,
    pub rows: RowSource,
}

/// Eine Zuweisung der `SET`-Liste. Der Wert ist eine Hostvariable oder ein Literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    /// DDIC-Name der Spalte (Kleinbuchstaben)
    pub column: String,
    pub value: Expr,
}

/// `SELECT [SINGLE] <Spalten> FROM <tab> [WHERE ...] [ORDER BY ...] [UP TO n ROWS] INTO <ziel>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectOp {
    /// DDIC-Name der Tabelle in Großbuchstaben (z. B. `ZFLUG`). Der Adapter-Name kommt aus der Konfiguration.
    pub table: String,
    /// Ergebnisspalten in Reihenfolge der Projektion. `SELECT *` ist vom Compiler bereits aufgelöst.
    pub columns: Vec<ResultColumn>,
    /// Filter (WHERE); `None` ohne Bedingung
    pub filter: Option<Expr>,
    /// Sortierung (ORDER BY), in Reihenfolge
    pub order_by: Vec<OrderItem>,
    /// `UP TO n ROWS`; `None` ohne Begrenzung
    pub up_to: Option<u64>,
    /// Wohin das Ergebnis geht
    pub result: ResultMode,
    /// Größe des Zielbereichs in Bytes: Arbeitsbereich bzw. eine Zeile der internen Tabelle.
    /// Der Compiler füllt alle Komponenten des Ziels, daher enthält der Zielbereich nach dem
    /// Schreiben nur Werte aus dem Ergebnis.
    pub target_size: u64,
    /// Hostvariablen in Reihenfolge. `Expr::Param { index }` verweist auf `params[index]`.
    pub params: Vec<Param>,
    /// Komponenten des Zielbereichs mit Typ und Offset (Spaltenname = Komponente, bei einem Feld der Name
    /// des Feldes). Ein Ziel mit `INTO CORRESPONDING FIELDS OF TABLE` beginnt jede neue Zeile mit diesen
    /// Initialwerten, damit nicht zugeordnete Komponenten initial sind. Fehlt die Liste, bleibt der Puffer 0.
    #[serde(default)]
    pub target_fields: Vec<RowField>,
}

/// Ziel des Ergebnisses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultMode {
    /// Ein Satz in einen Arbeitsbereich oder ein Feld (`SELECT SINGLE` und `SELECT ... INTO` ohne Tabelle).
    /// Ohne Treffer bleibt das Ziel unverändert, `SY-SUBRC = 4`.
    Single,
    /// `INTO TABLE`: die interne Tabelle wird zuerst geleert, dann mit allen Treffern gefüllt.
    Table,
    /// `APPENDING TABLE`: Treffer werden an die Tabelle angehängt.
    Append,
    /// `SELECT ... ENDSELECT`: Cursor, je Zeile ein Schleifendurchlauf. Nur für `abap_rt_sql_cursor_*`.
    Loop,
}

/// Eine Ergebnisspalte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultColumn {
    /// DDIC-Name der Spalte in Kleinbuchstaben (z. B. `carrid`). Der physische Name steht in der Konfiguration.
    pub name: String,
    /// DDIC-Typ der Spalte (Quelle)
    pub source: AbapType,
    /// Typ des Zielfelds (ABAP-Typ der Variablen, in die der Wert geschrieben wird)
    pub target: AbapType,
    /// Byte-Offset des Zielfelds im Zielbereich
    pub offset: u64,
}

/// Sortierung nach einer Spalte.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderItem {
    /// DDIC-Name der Spalte (Kleinbuchstaben)
    pub column: String,
    /// `true` für `DESCENDING`
    pub descending: bool,
}

/// Hostvariable mit ihrem ABAP-Typ. Die Laufzeit liest den Wert über den Zeiger aus dem Aufruf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    /// Name im Quelltext (nur für Fehlermeldungen und Dump)
    pub name: String,
    pub ty: AbapType,
}

/// ABAP-Typ eines Felds, soweit Phase 1 ihn abbildet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AbapType {
    /// `TYPE i`: 4 Byte, Vorzeichen, Little Endian
    Int,
    /// Zeichenfeld fester Länge in Zeichen (2 Byte je Zeichen, UTF-16 in der Laufzeit)
    Char {
        length: u32,
        #[serde(default)]
        dtype: CharType,
    },
    /// Gepackte Dezimalzahl (`TYPE p`): Bytezahl und Nachkommastellen
    Packed { bytes: u32, scale: u32 },
}

/// Art eines Zeichenfelds. Bestimmt die Auffüllung beim Lesen (siehe Spezifikation, Abschnitt 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CharType {
    /// `c`: rechts mit Leerzeichen aufgefüllt
    #[default]
    Char,
    /// `n`: nur Ziffern, links mit `0` aufgefüllt
    Numc,
    /// `d` (DATS): 8 Zeichen, `YYYYMMDD`
    Dats,
    /// `t` (TIMS): 6 Zeichen, `HHMMSS`
    Tims,
}

/// Filterausdruck (WHERE).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Expr {
    /// Spalte der Tabelle (DDIC-Name in Kleinbuchstaben)
    Column {
        name: String,
    },
    /// Hostvariable: Index in `SelectOp::params`
    Param {
        index: u32,
    },
    /// Konstante aus dem Quelltext, bereits typisiert
    Literal {
        value: Value,
    },
    Compare {
        op: CmpOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Between {
        value: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
    },
    In {
        value: Box<Expr>,
        list: Vec<Expr>,
    },
    Like {
        value: Box<Expr>,
        pattern: Box<Expr>,
    },
    And {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Or {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not {
        operand: Box<Expr>,
    },
}

/// Vergleichsoperator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Konstanter Wert aus dem Quelltext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Value {
    /// Ganzzahl (Literal ohne Anführungszeichen)
    Int { value: i64 },
    /// Zeichenkette (Literal in `'...'` oder `` `...` ``), ohne Begrenzer
    Char { value: String },
}

/// Kodiert eine Anweisung als JSON-Text (wie er im Programm und im Adapterprotokoll steht).
pub fn encode(request: &SqlRequest) -> String {
    serde_json::to_string(request).expect("SQL IR serializes to JSON")
}

/// Dekodiert JSON-Text und prüft die Formatversion.
pub fn decode(text: &str) -> Result<SqlRequest, String> {
    let request: SqlRequest =
        serde_json::from_str(text).map_err(|e| format!("invalid SQL IR: {e}"))?;
    if request.ir_version != IR_VERSION {
        return Err(format!(
            "SQL IR version {} not supported (expected {IR_VERSION})",
            request.ir_version
        ));
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SqlRequest {
        SqlRequest {
            ir_version: IR_VERSION,
            line: 7,
            connection: "DEFAULT".into(),
            op: SqlOp::Select(SelectOp {
                table: "ZFLUG".into(),
                columns: vec![ResultColumn {
                    name: "connid".into(),
                    source: AbapType::Char {
                        length: 4,
                        dtype: CharType::Numc,
                    },
                    target: AbapType::Char {
                        length: 4,
                        dtype: CharType::Numc,
                    },
                    offset: 6,
                }],
                filter: Some(Expr::And {
                    left: Box::new(Expr::Compare {
                        op: CmpOp::Eq,
                        left: Box::new(Expr::Column {
                            name: "carrid".into(),
                        }),
                        right: Box::new(Expr::Param { index: 0 }),
                    }),
                    right: Box::new(Expr::Not {
                        operand: Box::new(Expr::Literal {
                            value: Value::Int { value: 3 },
                        }),
                    }),
                }),
                order_by: vec![OrderItem {
                    column: "connid".into(),
                    descending: true,
                }],
                up_to: Some(10),
                result: ResultMode::Table,
                target_size: 8,
                target_fields: vec![],
                params: vec![Param {
                    name: "lv_carrid".into(),
                    ty: AbapType::Char {
                        length: 3,
                        dtype: CharType::Char,
                    },
                }],
            }),
        }
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let original = sample();
        let text = encode(&original);
        assert_eq!(decode(&text).expect("decodes"), original);
    }

    #[test]
    fn encoded_text_is_readable_json_with_kind_tags() {
        let text = encode(&sample());
        assert!(text.contains("\"op\":\"select\""), "{text}");
        assert!(text.contains("\"kind\":\"char\""), "{text}");
        assert!(text.contains("\"dtype\":\"numc\""), "{text}");
        assert!(text.contains("\"result\":\"table\""), "{text}");
        assert!(
            !text.to_uppercase().contains("SELECT "),
            "IR must not contain SQL text: {text}"
        );
    }

    #[test]
    fn char_type_defaults_to_char_when_missing() {
        let text = r#"{"kind":"char","length":3}"#;
        let ty: AbapType = serde_json::from_str(text).expect("parses");
        assert_eq!(
            ty,
            AbapType::Char {
                length: 3,
                dtype: CharType::Char
            }
        );
    }

    #[test]
    fn decode_rejects_other_ir_version() {
        let mut request = sample();
        request.ir_version = IR_VERSION + 1;
        let text = encode(&request);
        let err = decode(&text).unwrap_err();
        assert!(err.contains("version"), "{err}");
    }

    #[test]
    fn decode_rejects_broken_json_with_message() {
        let err = decode("{\"ir_version\":1").unwrap_err();
        assert!(err.starts_with("invalid SQL IR"), "{err}");
    }

    #[test]
    fn decode_rejects_unknown_operation() {
        let text = r#"{"ir_version":1,"line":1,"connection":"DEFAULT","op":{"op":"truncate"}}"#;
        assert!(decode(text).is_err());
    }
}
