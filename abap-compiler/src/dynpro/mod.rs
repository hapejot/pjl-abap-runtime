//! Dynpro-Beschreibung (`.dynp`): Modell, Parser (`parse.rs`) und Prüfung gegen das ABAP-Programm (`check.rs`).
//! Grundlage: `doc/dynpro-spezifikation.md`, Abschnitte 4 (Format), 4.6 (Prüfungen) und 4.7 (Feldtypen).
//! Meldungen sind englisch (CLAUDE.md).

mod check;
mod parse;

pub use check::check;
pub(crate) use check::component;
pub use parse::parse_dynp;

use std::fmt;

/// Fehler oder Warnung mit Position in einer Quelldatei (`.dynp` oder ABAP-Programm).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynproError {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub warning: bool,
}

impl DynproError {
    /// Übersetzungsfehler an einer Position.
    pub fn error(file: &str, line: usize, col: usize, message: impl Into<String>) -> Self {
        DynproError {
            file: file.to_string(),
            line,
            col,
            message: message.into(),
            warning: false,
        }
    }

    /// Warnung an einer Position; der Übersetzungslauf scheitert nicht.
    pub fn warning(file: &str, line: usize, col: usize, message: impl Into<String>) -> Self {
        DynproError {
            warning: true,
            ..Self::error(file, line, col, message)
        }
    }
}

impl fmt::Display for DynproError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.warning { "warning" } else { "error" };
        write!(
            f,
            "{}:{}:{}: {kind}: {}",
            self.file, self.line, self.col, self.message
        )
    }
}

/// Typ eines Dynpro-Feldes, aus der Deklaration des ABAP-Programms (`DATA ... TYPE ...`).
/// Das IR führt `c`, `n`, `d` und `t` alle als Zeichenfeld; für Dynpros wird der Typ deshalb getrennt festgehalten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// `c` mit Länge in Zeichen
    Char(usize),
    /// `n` mit Länge in Ziffern
    Numc(usize),
    /// `d` (DATS): 8 Zeichen
    Date,
    /// `t` (TIMS): 6 Zeichen
    Time,
    /// `i`: ganze Zahl mit Vorzeichen
    Int,
    /// `p` mit Länge in Bytes und Nachkommastellen
    Packed { bytes: usize, decimals: usize },
    /// `string`: im Dynpro nicht erlaubt (Spec 4.7)
    String,
    /// Strukturen, Referenzen, DDIC-Typen und Ähnliches: im Dynpro nicht unterstützt
    Other,
}

impl FieldType {
    /// Typkennung für `abap_rt_screen_field`: 0 c, 1 n, 2 d, 3 t, 4 i, 5 p. `None` bei nicht erlaubten Typen.
    pub fn abi_code(self) -> Option<u32> {
        match self {
            FieldType::Char(_) => Some(0),
            FieldType::Numc(_) => Some(1),
            FieldType::Date => Some(2),
            FieldType::Time => Some(3),
            FieldType::Int => Some(4),
            FieldType::Packed { .. } => Some(5),
            FieldType::String | FieldType::Other => None,
        }
    }

    /// Länge für `abap_rt_screen_field`: Zeichen bei c und n, Ziffern bei i (10) und p (2 × Bytes − 1).
    pub fn abi_length(self) -> Option<usize> {
        match self {
            FieldType::Char(n) | FieldType::Numc(n) => Some(n),
            FieldType::Date => Some(8),
            FieldType::Time => Some(6),
            FieldType::Int => Some(10),
            FieldType::Packed { bytes, .. } => Some(2 * bytes - 1),
            FieldType::String | FieldType::Other => None,
        }
    }

    /// Nachkommastellen für `abap_rt_screen_field`; nur bei p ungleich 0.
    pub fn abi_decimals(self) -> usize {
        match self {
            FieldType::Packed { decimals, .. } => decimals,
            _ => 0,
        }
    }

    /// Breite auf dem Bildschirm in Zellen. Bei i das Vorzeichen mit eingerechnet, bei p Vorzeichen und Dezimaltrenner.
    pub fn width(self) -> Option<usize> {
        match self {
            FieldType::Char(n) | FieldType::Numc(n) => Some(n),
            FieldType::Date => Some(8),
            FieldType::Time => Some(6),
            FieldType::Int => Some(11),
            FieldType::Packed { bytes, decimals } => {
                Some(2 * bytes - 1 + 1 + usize::from(decimals > 0))
            }
            FieldType::String | FieldType::Other => None,
        }
    }
}

/// Inhalt einer `.dynp`-Datei in Quellreihenfolge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynproFile {
    pub file: String,
    pub items: Vec<DynproItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynproItem {
    Dynpro(Dynpro),
    Status(GuiStatus),
}

/// Dynpro mit Nummer, Größe, Elementen und Ablauflogik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dynpro {
    pub number: u32,
    pub line: usize,
    /// `KIND MODAL` (Phase 2: nur als Vollbild dargestellt)
    pub modal: bool,
    pub rows: usize,
    pub cols: usize,
    pub title: String,
    pub elements: Vec<Element>,
    /// Ablauflogik in Quelltextfolge (Spec 12a): PBO- und PAI-Module, `FIELD`- und `CHAIN`-Einträge der PAI.
    pub flow: Vec<FlowItem>,
}

/// Element mit Position. `row` und `col` sind 1-basiert (Koordinaten im Dynpro-Bereich).
/// `line` und `kw_col` (Spalte des Schlüsselworts) dienen den Fehlermeldungen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub line: usize,
    pub kw_col: usize,
    pub row: usize,
    pub col: usize,
    pub kind: ElementKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementKind {
    /// `TEXT`: Ausgabetext
    Text {
        text: String,
        color: Option<u8>,
        intensified: bool,
    },
    /// `FIELD`: gebunden an eine globale Variable
    Field { name: String, attrs: FieldAttrs },
    /// `BUTTON`: Drucktaste mit Funktionscode
    Button { text: String, code: String },
    /// `FRAME`: Rahmen von (row, col) bis (row2, col2)
    Frame {
        row2: usize,
        col2: usize,
        title: String,
    },
}

/// Feldattribute. Ohne `INPUT` und `OUTPUT` ist das Feld `INPUT` (Entscheidung, Spec 4.8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldAttrs {
    pub input: bool,
    pub output: bool,
    pub required: bool,
    pub invisible: bool,
    pub intensified: bool,
    pub password: bool,
    pub color: Option<u8>,
}

/// Bit des Attributs `PASSWORD` in der Attributmaske der Laufzeit (Spec 8.2). Die Anzeige ist Sache der Laufzeit.
pub const ATTR_PASSWORD: u32 = 1 << 16;

impl FieldAttrs {
    /// Bitmaske für die Laufzeit: INPUT=1, OUTPUT=2, REQUIRED=4, INVISIBLE=8, INTENSIFIED=32,
    /// Farbe in Bits 8..10, PASSWORD in Bit 16.
    pub fn bits(&self) -> u32 {
        let mut bits = 0;
        if self.input {
            bits |= 1;
        }
        if self.output {
            bits |= 2;
        }
        if self.required {
            bits |= 4;
        }
        if self.invisible {
            bits |= 8;
        }
        if self.intensified {
            bits |= 32;
        }
        if self.password {
            bits |= ATTR_PASSWORD;
        }
        bits | color_bits(self.color)
    }
}

/// Farbe in die Bits 8..10 der Attributmaske.
pub fn color_bits(color: Option<u8>) -> u32 {
    color.map_or(0, |c| u32::from(c) << 8)
}

/// Eintrag der Ablauflogik in Quelltextfolge (Spec 12a). Die Reihenfolge der Liste ist die Reihenfolge im Quelltext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowItem {
    /// `MODULE m [AT EXIT-COMMAND]` in PBO oder PAI, ohne Feldbezug
    Module(FlowEntry),
    /// `FIELD f [MODULE m [ON ...]]` in PAI
    Field(FieldFlow),
    /// `MODULE m ON CHAIN-INPUT` bzw. `ON CHAIN-REQUEST` als letzte Modulzeile einer `CHAIN`
    ChainModule(ChainModule),
}

/// Ablauflogik-Eintrag: `PROCESS` mit `MODULE` ohne Feldbezug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowEntry {
    pub phase: Phase,
    pub module: String,
    /// `AT EXIT-COMMAND`: Modul läuft auch bei Funktionscodes der Exit-Taste
    pub at_exit: bool,
    /// Bedingung `ON ...`. Nur bei Feldbezug oder Kette erlaubt; die Prüfung steht in `check.rs`.
    pub cond: Cond,
    pub line: usize,
}

/// `FIELD name [MODULE m [ON ...]]` in der PAI-Folge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldFlow {
    /// Name, wie er in der `.dynp` steht (muss einem `FIELD`-Element des Dynpros entsprechen)
    pub name: String,
    pub module: Option<String>,
    pub cond: Cond,
    /// Kettennummer ab 1 innerhalb des Dynpros, wenn die Anweisung in einer `CHAIN` steht
    pub chain: Option<usize>,
    pub line: usize,
}

/// `MODULE m ON CHAIN-INPUT` bzw. `ON CHAIN-REQUEST` am Ende einer `CHAIN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainModule {
    pub chain: usize,
    pub module: String,
    pub cond: Cond,
    pub line: usize,
}

/// Bedingung eines PAI-Eintrags (Spec 12a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    /// Ohne Zusatz: das Modul läuft immer
    Plain,
    /// `ON INPUT`: Feld ungleich Initialwert
    Input,
    /// `ON REQUEST`: Feld seit dem letzten PBO vom Benutzer geändert
    Request,
    /// `ON CHAIN-INPUT`: mindestens ein Kettenfeld ungleich Initialwert
    ChainInput,
    /// `ON CHAIN-REQUEST`: mindestens ein Kettenfeld seit dem letzten PBO vom Benutzer geändert
    ChainRequest,
}

impl Cond {
    /// Kennung für `abap_rt_screen_pai_field` (0 ohne, 1 ON INPUT, 2 ON REQUEST) und
    /// `abap_rt_screen_pai_chain` (3 ON CHAIN-INPUT, 4 ON CHAIN-REQUEST), Spec 12a.
    pub fn abi_code(self) -> u32 {
        match self {
            Cond::Plain => 0,
            Cond::Input => 1,
            Cond::Request => 2,
            Cond::ChainInput => 3,
            Cond::ChainRequest => 4,
        }
    }

    /// Schreibweise in Meldungen, z. B. `ON INPUT`. Für `Plain` leer.
    pub fn text(self) -> &'static str {
        match self {
            Cond::Plain => "",
            Cond::Input => "ON INPUT",
            Cond::Request => "ON REQUEST",
            Cond::ChainInput => "ON CHAIN-INPUT",
            Cond::ChainRequest => "ON CHAIN-REQUEST",
        }
    }
}

/// Ablaufphase: PBO (`PROCESS BEFORE OUTPUT`) oder PAI (`PROCESS AFTER INPUT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Pbo,
    Pai,
}

/// GUI-Status mit Funktionstasten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiStatus {
    pub name: String,
    pub line: usize,
    pub keys: Vec<FKey>,
    /// `EXCLUDE funktionscode`: Funktionscodes, die der Status ausblendet (Phase 2)
    pub excludes: Vec<Exclude>,
}

/// Eintrag `EXCLUDE funktionscode` in einem GUI-Status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exclude {
    pub code: String,
    pub line: usize,
}

/// Funktionstaste `FKEY F1 'Text' CODE code [TYPE B|C|E]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FKey {
    /// Nummer der Taste, 1 bis 24 (F13 bis F24 werden erst mit Shift+F in Phase 2 ausgelöst)
    pub key: u32,
    pub text: String,
    pub code: String,
    pub kind: FKeyType,
    pub line: usize,
}

/// Art der Funktionstaste: keine, Zurück (B), Abbrechen (C), Beenden (E).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FKeyType {
    None,
    Back,
    Cancel,
    Exit,
}

impl FKeyType {
    /// Kennung für `abap_rt_status_fkey`: 0 keine, 1 B, 2 C, 3 E.
    pub fn abi_code(self) -> u32 {
        match self {
            FKeyType::None => 0,
            FKeyType::Back => 1,
            FKeyType::Cancel => 2,
            FKeyType::Exit => 3,
        }
    }
}

/// Länge von `sy-ucomm` (Spec 4.6): Funktionscodes höchstens so lang.
pub const UCOMM_LENGTH: usize = 70;
