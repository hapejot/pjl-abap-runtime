//! Modell eines Dynpros (Abschnitte 4 und 8.2): Felder mit Adressen der ABAP-Variablen, Texte, Tasten,
//! Rahmen, GUI-Status und die Modul-Bindungen je Ablaufphase.

/// Attribut `INPUT` (Bit 0). Ohne `OUTPUT` ist ein Feld ohnehin Eingabefeld.
pub const INPUT: u32 = 1;
/// Attribut `OUTPUT`: nur Anzeige, die Übertragung zurück ins Programm entfällt.
pub const OUTPUT: u32 = 2;
/// Attribut `REQUIRED`: leere Eingabe wird vor dem PAI abgewiesen.
pub const REQUIRED: u32 = 4;
/// Attribut `INVISIBLE`: nicht angezeigt, nicht editierbar.
pub const INVISIBLE: u32 = 8;
/// Attribut `PASSWORD`: Anzeige als `*`.
pub const PASSWORD: u32 = 16;
/// Attribut `INTENSIFIED`: hervorgehoben.
pub const INTENSIFIED: u32 = 32;
/// Farbe in den Bits 8 bis 10 (Wert 1 bis 7).
pub const COLOR_SHIFT: u32 = 8;
pub const COLOR_MASK: u32 = 0x700;

/// Zeiger auf eine globale ABAP-Variable des Programms.
///
/// Die Zeiger kommen aus dem Prolog und zeigen in den Datenbereich des Programms, der bis zum Programmende
/// gültig bleibt. Das Programm läuft in einem Thread; die Zeiger werden nur dort gelesen und geschrieben.
#[derive(Clone, Copy, Debug)]
pub struct VarPtr(pub *mut u8);

// SAFETY: Die Zeiger zeigen in den Datenbereich des Programms und werden nur im Programm-Thread
// dereferenziert. Die Laufzeit verlangt Send/Sync nur, weil der Zustand in einem `static Mutex` liegt.
unsafe impl Send for VarPtr {}
// SAFETY: wie bei `Send`. Geteilte Referenzen auf `VarPtr` erlauben keinen Zugriff ohne `unsafe`.
unsafe impl Sync for VarPtr {}

/// ABAP-Typ eines Dynpro-Feldes (Abschnitt 4.7). Die Codes sind die der ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbapType {
    /// `c`: Zeichen, Länge in Zeichen.
    Char,
    /// `n`: NUMC, nur Ziffern.
    Num,
    /// `d`: DATS, Länge 8.
    Date,
    /// `t`: TIMS, Länge 6.
    Time,
    /// `i`: ganze Zahl, 4 Bytes.
    Int,
    /// `p` / DEC: gepackt, Länge in Bytes.
    Packed,
}

impl AbapType {
    /// Typ aus dem Code der ABI: 0 c, 1 n, 2 d, 3 t, 4 i, 5 p.
    pub fn from_code(code: u32) -> Option<AbapType> {
        Some(match code {
            0 => AbapType::Char,
            1 => AbapType::Num,
            2 => AbapType::Date,
            3 => AbapType::Time,
            4 => AbapType::Int,
            5 => AbapType::Packed,
            _ => return None,
        })
    }

    /// Speicherbedarf in Bytes bei der Länge `length`. Zeichen sind UTF-16-Einheiten (2 Bytes).
    pub fn mem_len(self, length: usize) -> usize {
        match self {
            AbapType::Char | AbapType::Num | AbapType::Date | AbapType::Time => length * 2,
            AbapType::Int => 4,
            AbapType::Packed => length,
        }
    }
}

/// Feld: Anzeige- oder Eingabeposition, gebunden an eine globale ABAP-Variable.
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    /// Zeile im Dynpro-Bereich, 1-basiert.
    pub row: u32,
    /// Spalte im Dynpro-Bereich, 1-basiert.
    pub col: u32,
    pub ty: AbapType,
    /// Länge: Zeichen bei `c`, `n`, `d`, `t`; Bytes bei `p`; 4 bei `i`.
    pub length: usize,
    /// Nachkommastellen (nur `p`).
    pub decimals: usize,
    pub attrs: u32,
    pub data: VarPtr,
    /// Anzeigebreite in Zeichen (Abschnitt 4.7).
    pub width: usize,
}

impl Field {
    /// Speicherbedarf der ABAP-Variablen in Bytes.
    pub fn mem_len(&self) -> usize {
        self.ty.mem_len(self.length)
    }

    /// Sichtbar, also nicht `INVISIBLE`.
    pub fn is_visible(&self) -> bool {
        self.attrs & INVISIBLE == 0
    }

    /// Eingabefeld: sichtbar und nicht `OUTPUT`.
    pub fn is_input(&self) -> bool {
        self.is_visible() && self.attrs & OUTPUT == 0
    }

    pub fn is_required(&self) -> bool {
        self.attrs & REQUIRED != 0
    }

    pub fn is_password(&self) -> bool {
        self.attrs & PASSWORD != 0
    }
}

/// Text auf dem Dynpro (`TEXT`).
#[derive(Clone, Debug)]
pub struct Text {
    pub row: u32,
    pub col: u32,
    pub text: String,
    pub attrs: u32,
}

/// Drucktaste (`BUTTON`). Die Anzeige ist `[ text ]`.
#[derive(Clone, Debug)]
pub struct Button {
    pub row: u32,
    pub col: u32,
    pub text: String,
    pub fcode: String,
}

impl Button {
    /// Anzeigetext der Taste.
    pub fn label(&self) -> String {
        format!("[ {} ]", self.text)
    }
}

/// Rahmen (`FRAME`), Positionen im Dynpro-Bereich, beide Ecken eingeschlossen.
#[derive(Clone, Debug)]
pub struct Frame {
    pub row1: u32,
    pub col1: u32,
    pub row2: u32,
    pub col2: u32,
    pub title: String,
}

/// Ablaufphase eines Dynpros (Abschnitt 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Pbo,
    Pai,
}

/// Modul-Bindung. `at_exit` gilt für `MODULE ... AT EXIT-COMMAND` (Abschnitt 4.2, ABI).
#[derive(Clone, Copy, Debug)]
pub struct Module {
    pub func: extern "C" fn(),
    pub at_exit: bool,
}

/// Bedingung eines Feld- oder Kettenmoduls (Abschnitt 12a). `Always` heißt: ohne Zusatz, das Modul läuft immer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Always,
    /// `ON INPUT`: Wert nach der Übertragung ungleich Initialwert.
    OnInput,
    /// `ON REQUEST`: Eingabemarkierung des Feldes ist gesetzt.
    OnRequest,
    /// `ON CHAIN-INPUT`: mindestens ein Kettenfeld ungleich Initialwert.
    ChainInput,
    /// `ON CHAIN-REQUEST`: mindestens ein Kettenfeld mit Eingabemarkierung.
    ChainRequest,
}

impl Cond {
    /// Bedingung eines `FIELD`-Eintrags aus der ABI: 0 ohne, 1 ON INPUT, 2 ON REQUEST.
    pub fn from_field_code(code: u32) -> Option<Cond> {
        Some(match code {
            0 => Cond::Always,
            1 => Cond::OnInput,
            2 => Cond::OnRequest,
            _ => return None,
        })
    }

    /// Bedingung eines Kettenmoduls aus der ABI: 3 ON CHAIN-INPUT, 4 ON CHAIN-REQUEST.
    pub fn from_chain_code(code: u32) -> Option<Cond> {
        Some(match code {
            3 => Cond::ChainInput,
            4 => Cond::ChainRequest,
            _ => return None,
        })
    }
}

/// Eintrag der geordneten PAI-Liste eines Dynpros (Abschnitt 12a). Die Reihenfolge ist die des Prologs.
#[derive(Clone, Debug)]
pub enum PaiEntry {
    /// Modul ohne Feldbezug. Ein `AT EXIT-COMMAND`-Modul (`at_exit`) läuft nur in Schritt 1 des PAI.
    Module(Module),
    /// `FIELD name [MODULE m [ON INPUT | ON REQUEST]]`. `chain` 0 ohne Kette, sonst die Kettennummer.
    Field {
        name: String,
        func: Option<extern "C" fn()>,
        cond: Cond,
        chain: u32,
    },
    /// `MODULE m ON CHAIN-INPUT | ON CHAIN-REQUEST` innerhalb der Kette `chain` (ab 1).
    Chain {
        chain: u32,
        func: extern "C" fn(),
        cond: Cond,
    },
}

/// Wertehilfe-Eintrag (`PROCESS ON VALUE-REQUEST`, Spec 12b): das Modul, das F4 auf dem Feld auslöst.
/// Der Feldname ist die Schreibweise des `FIELD`-Elements und wird case-insensitiv verglichen.
#[derive(Clone, Debug)]
pub struct PovEntry {
    pub name: String,
    pub func: extern "C" fn(),
}

/// Dynpro mit Nummer, Größe, Titel, Elementen und Ablauflogik.
pub struct Dynpro {
    pub no: u32,
    /// `KIND MODAL`. Die Darstellung als Fenster ist Phase 2; in Phase 1 nur gespeichert.
    pub modal: bool,
    /// Größe des Dynpro-Bereichs (höchstens 20 Zeilen).
    pub rows: u32,
    /// Größe des Dynpro-Bereichs (höchstens 80 Spalten).
    pub cols: u32,
    pub title: String,
    pub fields: Vec<Field>,
    pub texts: Vec<Text>,
    pub buttons: Vec<Button>,
    pub frames: Vec<Frame>,
    pub pbo: Vec<Module>,
    /// Geordnete PAI-Liste (Abschnitt 12a). Module ohne Feldbezug, Feld- und Kettenmodule in Quelltextfolge.
    pub pai: Vec<PaiEntry>,
    /// Wertehilfe-Einträge (Spec 12b), getrennt von der PAI-Liste. Höchstens ein Eintrag je Feld.
    pub pov: Vec<PovEntry>,
}

impl Dynpro {
    pub fn new(no: u32, modal: bool, rows: u32, cols: u32, title: String) -> Dynpro {
        Dynpro {
            no,
            modal,
            rows,
            cols,
            title,
            fields: Vec::new(),
            texts: Vec::new(),
            buttons: Vec::new(),
            frames: Vec::new(),
            pbo: Vec::new(),
            pai: Vec::new(),
            pov: Vec::new(),
        }
    }

    /// Dynpro-Nummer mit vier Stellen, wie in Meldungen üblich.
    pub fn label(&self) -> String {
        format!("{:04}", self.no)
    }

    /// Index des Feldes mit dem Namen (ohne Unterschied bei Groß- und Kleinschreibung).
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Liegt das Rechteck mit Anfang (`row`, `col`) und Breite `width` im Dynpro-Bereich?
    pub fn contains_rect(&self, row: u32, col: u32, width: usize) -> bool {
        row >= 1
            && row <= self.rows
            && col >= 1
            && (col as usize) + width.max(1) - 1 <= self.cols as usize
    }
}

/// Funktionstasten-Typ aus dem GUI-Status (`TYPE B`, `C`, `E`). Ohne Typ: `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FKind {
    None,
    /// `B`: Zurück.
    Back,
    /// `C`: Abbrechen.
    Cancel,
    /// `E`: Beenden.
    Exit,
}

impl FKind {
    /// Typ aus dem Code der ABI: 0 keine, 1 B, 2 C, 3 E.
    pub fn from_code(code: u32) -> Option<FKind> {
        Some(match code {
            0 => FKind::None,
            1 => FKind::Back,
            2 => FKind::Cancel,
            3 => FKind::Exit,
            _ => return None,
        })
    }

    /// Typ B, C oder E: Die Prüfungen bei der Übertragung entfallen (Abschnitt 6.4).
    pub fn is_exit(self) -> bool {
        self != FKind::None
    }
}

/// Funktionstaste eines GUI-Status (`FKEY`).
#[derive(Clone, Debug)]
pub struct FKey {
    /// Tastennummer 1 bis 24.
    pub key: u32,
    pub text: String,
    pub fcode: String,
    pub kind: FKind,
}

/// GUI-Status (Funktionstasten), gesetzt mit `SET PF-STATUS`.
#[derive(Clone, Debug)]
pub struct GuiStatus {
    /// Name in Großbuchstaben.
    pub name: String,
    pub fkeys: Vec<FKey>,
    /// Ausgeschlossene Funktionscodes (`EXCLUDE`). Sie lösen nichts aus und werden nicht angezeigt (Abschnitt 6.2).
    /// Die Zuordnung gilt für diesen Status, nicht für das Dynpro.
    pub excluded: Vec<String>,
}

impl GuiStatus {
    pub fn fkey(&self, key: u32) -> Option<&FKey> {
        self.fkeys.iter().find(|k| k.key == key)
    }

    /// Funktionstaste mit dem Funktionscode.
    pub fn by_fcode(&self, fcode: &str) -> Option<&FKey> {
        self.fkeys.iter().find(|k| k.fcode == fcode)
    }

    /// Ist der Funktionscode in diesem Status ausgeschlossen?
    pub fn is_excluded(&self, fcode: &str) -> bool {
        self.excluded.iter().any(|e| e == fcode)
    }
}

/// Meldung für die Zeile 24 (Abschnitt 6.5). `kind` ist der ABAP-Meldungstyp als Zeichen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: char,
    pub text: String,
}
