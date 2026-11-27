//! Externe Bibliotheken: Beschreibung von Objekten außerhalb des Exports (Klassen, Interfaces,
//! Funktionsbausteine, Datenelemente, Konstanten), persistiert als JSON.
//!
//! Die Metadaten stammen von einem konkreten System und werden nicht eingecheckt (siehe `.gitignore`).
//! Objekte, die in keiner Bibliothek stehen, kommen in die Bibliothek `unbekannt`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Woher die Metadaten stammen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Origin {
    /// Aus einem SAP-System abgeleitet; Systemkennung und Release gehören zur Datei
    Sap { system: String, release: String },
    /// Von Hand gepflegt
    Manual,
    /// Objekte ohne bekannte Quelle
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Class,
    Interface,
    FunctionModule,
    DataElement,
    Domain,
    Table,
    Constant,
    Unknown,
}

/// Was der Compiler mit dem Objekt tun kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Es gibt eine Laufzeitimplementierung
    Implemented,
    /// Signatur bekannt, Verhalten nicht implementiert
    SignatureOnly,
    /// Bekannt, aber nicht verfügbar
    Unavailable,
    /// Keine Angabe
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Importing,
    Exporting,
    Changing,
    Returning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
    /// Typname (DDIC-Name, Systemtyp oder `REF TO <Name>`)
    pub typ: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_static: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    pub typ: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_static: bool,
}

/// Datenelement oder Domäne: Datentyp mit Länge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Element {
    pub datatype: String,
    pub length: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub decimals: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub name: String,
    pub kind: ObjectKind,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superclass: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<Method>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<Attribute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<Element>,
}

impl Object {
    /// Objekt ohne Angaben außer Name, Art und Status.
    pub fn new(name: &str, kind: ObjectKind, status: Status) -> Self {
        Object {
            name: name.to_lowercase(),
            kind,
            status,
            superclass: None,
            interfaces: Vec::new(),
            methods: Vec::new(),
            attributes: Vec::new(),
            element: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub origin: Origin,
    /// Objekte, Schlüssel ist der Name in Kleinbuchstaben
    pub objects: BTreeMap<String, Object>,
}

/// Name der Sammelbibliothek für Objekte ohne bekannte Quelle.
pub const UNKNOWN_LIBRARY: &str = "unbekannt";

impl Library {
    pub fn new(name: &str, origin: Origin) -> Self {
        Library {
            name: name.to_string(),
            origin,
            objects: BTreeMap::new(),
        }
    }

    /// Sammelbibliothek für Objekte ohne Quelle.
    pub fn unknown() -> Self {
        Library::new(UNKNOWN_LIBRARY, Origin::Unknown)
    }

    /// Fügt ein Objekt hinzu oder ersetzt es. Der Schlüssel ist der Name in Kleinbuchstaben.
    pub fn insert(&mut self, object: Object) {
        self.objects.insert(object.name.to_lowercase(), object);
    }

    /// Objekt nachschlagen, unabhängig von der Schreibweise.
    pub fn get(&self, name: &str) -> Option<&Object> {
        self.objects.get(&name.to_lowercase())
    }

    /// Notiert ein Objekt in der Sammelbibliothek, falls es in dieser Bibliothek noch nicht steht.
    /// Vorhandene Einträge bleiben unverändert.
    pub fn record_unknown(&mut self, name: &str, kind: ObjectKind) {
        self.objects
            .entry(name.to_lowercase())
            .or_insert_with(|| Object::new(name, kind, Status::Unknown));
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("JSON output failed: {e}"))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("Invalid library: {e}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json()?;
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Library {
        let mut lib = Library::new(
            "sap-test",
            Origin::Sap {
                system: "DEV".into(),
                release: "758".into(),
            },
        );
        let mut tree = Object::new("CL_GUI_ALV_TREE", ObjectKind::Class, Status::SignatureOnly);
        tree.superclass = Some("cl_gui_alv_tree_base".into());
        tree.methods.push(Method {
            name: "get_outtab_line".into(),
            is_static: false,
            params: vec![Param {
                name: "ir_row".into(),
                kind: ParamKind::Importing,
                typ: "lvc_nkey".into(),
                optional: true,
            }],
        });
        lib.insert(tree);
        lib.insert(Object {
            element: Some(Element {
                datatype: "CHAR".into(),
                length: 10,
                decimals: 0,
            }),
            ..Object::new("PROGNAME", ObjectKind::DataElement, Status::Implemented)
        });
        lib
    }

    #[test]
    fn lookup_ignores_case_and_keys_are_lowercase() {
        let lib = sample();
        assert!(lib.objects.contains_key("cl_gui_alv_tree"));
        assert_eq!(lib.get("CL_GUI_ALV_TREE").unwrap().kind, ObjectKind::Class);
        assert!(lib.get("gibt_es_nicht").is_none());
    }

    #[test]
    fn json_round_trip_keeps_all_metadata() {
        let lib = sample();
        let json = lib.to_json().unwrap();
        let back = Library::from_json(&json).unwrap();
        assert_eq!(back, lib);
        assert!(json.contains("\"system\": \"DEV\""));
    }

    #[test]
    fn unknown_objects_go_to_unknown_library_once() {
        let mut unknown = Library::unknown();
        unknown.record_unknown("ri_html", ObjectKind::Unknown);
        unknown.record_unknown("RI_HTML", ObjectKind::Class);
        assert_eq!(unknown.name, UNKNOWN_LIBRARY);
        assert_eq!(unknown.objects.len(), 1);
        // der erste Eintrag bleibt erhalten
        assert_eq!(unknown.get("ri_html").unwrap().kind, ObjectKind::Unknown);
        assert_eq!(unknown.get("ri_html").unwrap().status, Status::Unknown);
    }

    #[test]
    fn invalid_json_is_reported() {
        assert!(Library::from_json("{ kaputt").is_err());
    }

    #[test]
    fn save_and_load_from_file() {
        let dir = std::env::temp_dir().join(format!("abapc-lib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lib.json");
        let lib = sample();
        lib.save(&path).unwrap();
        assert_eq!(Library::load(&path).unwrap(), lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// --- Ableitung aus ABAP-Quelltext --------------------------------------------------------------

use crate::inspect::abap::{
    parse_l1, ClassDef, ClassKind, MemberKind, MethodSig, Statement, TopItem, Visibility,
};

/// Liest die öffentlichen Typen eines Quelltexts (Klassen und Interfaces) als Objekte.
/// Nicht öffentliche Member und Ereignisse werden nicht übernommen.
pub fn objects_from_source(src: &str) -> Vec<Object> {
    let prog = parse_l1(src);
    prog.items
        .into_iter()
        .filter_map(|item| match item {
            TopItem::Class(c) if c.kind != ClassKind::Implementation => Some(object_from_class(&c)),
            _ => None,
        })
        .collect()
}

/// Baut eine Bibliothek aus `(Dateiname, Quelltext)`-Paaren. Spätere Dateien ersetzen gleichnamige Objekte.
pub fn library_from_sources(name: &str, origin: Origin, files: &[(String, String)]) -> Library {
    let mut lib = Library::new(name, origin);
    for (_, src) in files {
        for object in objects_from_source(src) {
            lib.insert(object);
        }
    }
    lib
}

fn object_from_class(c: &ClassDef) -> Object {
    let kind = if c.kind == ClassKind::Interface {
        ObjectKind::Interface
    } else {
        ObjectKind::Class
    };
    let mut object = Object::new(&c.name, kind, Status::SignatureOnly);
    if kind == ObjectKind::Class {
        object.superclass = inheriting_from(&c.head);
    }
    for member in &c.members {
        if member.visibility != Visibility::Public {
            continue;
        }
        match &member.kind {
            MemberKind::Method(sig) => object.methods.push(method_from_sig(sig)),
            MemberKind::Interfaces => {
                if let Some(name) = member.statement.tokens.get(1) {
                    object.interfaces.push(name.text.to_lowercase());
                }
            }
            MemberKind::Data | MemberKind::Constants => {
                if let Some(attribute) = attribute_from_statement(&member.statement) {
                    object.attributes.push(attribute);
                }
            }
            _ => {}
        }
    }
    object
}

/// Oberklasse aus `CLASS x DEFINITION ... INHERITING FROM y`.
fn inheriting_from(head: &Statement) -> Option<String> {
    let pos = head.tokens.iter().position(|t| t.is_word("FROM"))?;
    head.tokens.get(pos + 1).map(|t| t.text.to_lowercase())
}

fn method_from_sig(sig: &MethodSig) -> Method {
    let mut params = Vec::new();
    for (section, raw) in &sig.sections {
        let kind = match section.as_str() {
            "IMPORTING" => ParamKind::Importing,
            "EXPORTING" => ParamKind::Exporting,
            "CHANGING" => ParamKind::Changing,
            "RETURNING" => ParamKind::Returning,
            _ => continue, // RAISING, EXCEPTIONS gehören nicht zur Schnittstelle der Parameter
        };
        params.extend(params_from_raw(raw, kind));
    }
    Method {
        name: sig.name.to_lowercase(),
        is_static: sig.is_class,
        params,
    }
}

/// Parameter aus dem Rohtext eines Abschnitts, z.B. `VALUE ( rv ) TYPE string iv TYPE REF TO zcl_x OPTIONAL`.
fn params_from_raw(raw: &str, kind: ParamKind) -> Vec<Param> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let mut params = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        i += 1;
        // Nur Bezeichner sind Parameternamen; VALUE(x), Klammern, Ausrufezeichen und Operatoren fallen weg
        if !is_identifier(word) || word.eq_ignore_ascii_case("VALUE") {
            continue;
        }
        // Schließende Klammer von VALUE(x) überspringen
        while words.get(i).is_some_and(|w| *w == ")") {
            i += 1;
        }
        let mut typ = String::new();
        let mut optional = false;
        if words
            .get(i)
            .is_some_and(|w| w.eq_ignore_ascii_case("TYPE") || w.eq_ignore_ascii_case("LIKE"))
        {
            let keyword = words[i].to_lowercase();
            i += 1;
            if words.get(i).is_some_and(|w| w.eq_ignore_ascii_case("REF"))
                && words
                    .get(i + 1)
                    .is_some_and(|w| w.eq_ignore_ascii_case("TO"))
            {
                typ = format!(
                    "ref to {}",
                    words.get(i + 2).copied().unwrap_or("").to_lowercase()
                );
                i += 3;
            } else if words.get(i).is_some_and(|w| is_table_keyword(w)) {
                // TYPE [STANDARD|SORTED|HASHED|INDEX] TABLE [OF x]
                let mut kind = String::new();
                if words.get(i).is_some_and(|w| {
                    ["STANDARD", "SORTED", "HASHED", "INDEX"]
                        .iter()
                        .any(|k| w.eq_ignore_ascii_case(k))
                }) {
                    kind = words[i].to_lowercase();
                    i += 1;
                }
                if words
                    .get(i)
                    .is_some_and(|w| w.eq_ignore_ascii_case("TABLE"))
                {
                    kind = if kind.is_empty() {
                        "table".into()
                    } else {
                        format!("{kind} table")
                    };
                    i += 1;
                }
                if words.get(i).is_some_and(|w| w.eq_ignore_ascii_case("OF")) {
                    i += 1;
                    typ = format!(
                        "table of {}",
                        words.get(i).copied().unwrap_or("").to_lowercase()
                    );
                    i += 1;
                } else {
                    typ = kind;
                }
            } else if let Some(w) = words.get(i) {
                typ = type_name(&keyword, w);
                i += 1;
            }
        }
        while let Some(w) = words.get(i) {
            if w.eq_ignore_ascii_case("OPTIONAL") {
                optional = true;
                i += 1;
            } else if w.eq_ignore_ascii_case("DEFAULT") || w.eq_ignore_ascii_case("PREFERRED") {
                // Wert: ein Wort oder ein Ausdruck wie klasse=>konstante
                i += 1;
                if words.get(i + 1).is_some_and(|x| *x == "=>" || *x == "~") {
                    i += 2;
                }
                i += 1;
            } else {
                break;
            }
        }
        params.push(Param {
            name: word.to_lowercase(),
            kind,
            typ,
            optional,
        });
    }
    params
}

/// Typangabe: `TYPE x` ergibt `x`, `LIKE x` ergibt `like x`.
fn type_name(keyword: &str, name: &str) -> String {
    if keyword == "like" {
        format!("like {}", name.to_lowercase())
    } else {
        name.to_lowercase()
    }
}

fn is_table_keyword(word: &str) -> bool {
    ["STANDARD", "SORTED", "HASHED", "INDEX", "TABLE", "OF"]
        .iter()
        .any(|k| word.eq_ignore_ascii_case(k))
}

fn is_identifier(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '<')
}

/// `DATA name TYPE typ` bzw. `CLASS-DATA name TYPE typ`.
fn attribute_from_statement(s: &Statement) -> Option<Attribute> {
    let name = s.tokens.get(1)?.text.to_lowercase();
    let pos = s
        .tokens
        .iter()
        .position(|t| t.is_word("TYPE") || t.is_word("LIKE"))?;
    let keyword = s.tokens[pos].text.to_lowercase();
    let rest = &s.tokens[pos + 1..];
    let typ = if rest.first().is_some_and(|t| t.is_word("REF"))
        && rest.get(1).is_some_and(|t| t.is_word("TO"))
    {
        format!("ref to {}", rest.get(2)?.text.to_lowercase())
    } else {
        type_name(&keyword, &rest.first()?.text)
    };
    Some(Attribute {
        name,
        typ,
        is_static: matches!(s.keyword().as_str(), "CLASS-DATA" | "CONSTANTS"),
    })
}

// --- Kommandozeile: abapc library build ---------------------------------------------------------

use std::path::PathBuf;

use clap::Args;

/// Argumente von `abapc library build`.
#[derive(Debug, Args)]
pub struct BuildOptions {
    /// Ordner mit `.abap`-Dateien oder abapGit-ZIP
    pub input: PathBuf,
    /// Name der Bibliothek
    #[arg(long)]
    pub name: String,
    /// Systemkennung, z.B. H25
    #[arg(long)]
    pub system: String,
    /// Release des Systems
    #[arg(long, default_value = "unbekannt")]
    pub release: String,
    /// Ausgabedatei (JSON)
    #[arg(short = 'o', long)]
    pub output: PathBuf,
}

fn read_sources_from_dir(dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            read_sources_from_dir(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("abap"))
        {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.push((path.display().to_string(), text));
        }
    }
    Ok(())
}

/// Erzeugt die Bibliothek und schreibt sie als JSON.
pub fn build(opts: BuildOptions) -> Result<(), String> {
    let files = if opts.input.is_dir() {
        let mut files = Vec::new();
        read_sources_from_dir(&opts.input, &mut files)?;
        files
    } else {
        let file = std::fs::File::open(&opts.input)
            .map_err(|e| format!("{}: {e}", opts.input.display()))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;
        crate::inspect::cli::read_abap_files(&mut archive)
    };
    let origin = Origin::Sap {
        system: opts.system.clone(),
        release: opts.release.clone(),
    };
    let lib = library_from_sources(&opts.name, origin, &files);
    lib.save(&opts.output)?;
    println!(
        "{} source files, {} objects written to {}",
        files.len(),
        lib.objects.len(),
        opts.output.display()
    );
    Ok(())
}

#[cfg(test)]
mod source_tests {
    use super::*;

    const SRC: &str = "CLASS zcl_demo DEFINITION INHERITING FROM cl_base PUBLIC.\n\
  PUBLIC SECTION.\n\
    CONSTANTS kind_x TYPE c VALUE 'X'.\n\
    CLASS-DATA counter TYPE i.\n\
    CLASS-METHODS make RETURNING VALUE(ro_obj) TYPE REF TO zcl_demo.\n\
    METHODS run\n\
      IMPORTING\n\
        VALUE(iv_mode) TYPE i DEFAULT cl_base=>mode_single\n\
        it_rows TYPE STANDARD TABLE OF string OPTIONAL\n\
        iv_like LIKE counter\n\
      EXPORTING\n\
        ev_done TYPE abap_bool.\n\
    INTERFACES if_some.\n\
  PRIVATE SECTION.\n\
    METHODS hidden.\n\
ENDCLASS.\n\
CLASS zcl_demo IMPLEMENTATION.\n\
ENDCLASS.\n";

    #[test]
    fn public_members_are_derived_from_class_definition() {
        let objects = objects_from_source(SRC);
        assert_eq!(objects.len(), 1);
        let class = &objects[0];
        assert_eq!(class.name, "zcl_demo");
        assert_eq!(class.kind, ObjectKind::Class);
        assert_eq!(class.superclass.as_deref(), Some("cl_base"));
        assert_eq!(class.interfaces, vec!["if_some"]);
        // private Methoden gehören nicht zur öffentlichen Schnittstelle
        assert!(class.methods.iter().all(|m| m.name != "hidden"));
        assert!(class
            .attributes
            .iter()
            .any(|a| a.name == "kind_x" && a.is_static));
        assert!(class
            .attributes
            .iter()
            .any(|a| a.name == "counter" && a.is_static && a.typ == "i"));
    }

    #[test]
    fn parameter_forms_keep_their_types() {
        let objects = objects_from_source(SRC);
        let class = &objects[0];
        let make = class.methods.iter().find(|m| m.name == "make").unwrap();
        assert!(make.is_static);
        assert_eq!(make.params[0].typ, "ref to zcl_demo");
        assert_eq!(make.params[0].kind, ParamKind::Returning);

        let run = class.methods.iter().find(|m| m.name == "run").unwrap();
        let by_name = |n: &str| run.params.iter().find(|p| p.name == n).unwrap();
        assert_eq!(by_name("iv_mode").typ, "i");
        assert_eq!(by_name("it_rows").typ, "table of string");
        assert!(by_name("it_rows").optional);
        assert_eq!(by_name("iv_like").typ, "like counter");
        assert_eq!(by_name("ev_done").kind, ParamKind::Exporting);
        assert!(run.params.iter().all(|p| !p.typ.is_empty()));
    }
}
