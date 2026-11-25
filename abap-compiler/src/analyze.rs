//! Auswertung der Übersetzungsfehler: welche Namen sind unbekannt, und was sind sie vermutlich?
//!
//! Jeder Fehler mit Namensbezug wird einer Klasse zugeordnet:
//! - Compiler-Lücken: Name ist im Quelltext deklariert, der Compiler kennt ihn nicht
//!   (Parameter, Attribute, Konstanten, eingebaute Namen, Schlüsselwörter, Funktionen)
//! - Externe Klassen und Interfaces: aus den Anforderungen der Verwendung (siehe `inspect::abap::requirements`)
//! - Wirklich unbekannt: alles übrige; die vermutete Art kommt nur aus den Verwendungsmustern

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use clap::Args;
use regex::{Regex, RegexBuilder};
use serde::Serialize;

use crate::ast::CompileError;
use crate::codegen::Codegen;
use crate::inspect::abap::requirements::{derive, Requirement, TypeKind};
use crate::lower::parse_source;

const BUILTIN: &[&str] = &["me", "abap_true", "abap_false", "abap_undefined", "sy", "syst", "space", "super"];
const KEYWORDS: &[&str] = &[
    "type", "like", "value", "ref", "to", "of", "table", "standard", "sorted", "hashed", "data", "object",
    "exception", "any",
];
const FUNCTIONS: &[&str] = &[
    "to_upper", "to_lower", "concat_lines_of", "escape", "condense", "strlen", "lines", "boolc", "xstrlen", "match",
    "replace", "find", "substring", "shift_left", "shift_right", "segment", "char_to_hex", "xsdbool", "numofchar",
    "to_mixed", "translate", "count", "bit_set", "nmax", "nmin", "ceil", "floor", "round", "trunc", "frac", "abs",
    "sign", "sqrt", "exp", "log", "ipow", "to_string", "line_exists", "line_index", "reverse", "distance", "contains",
    "matches",
];

/// Parameter der Kommandozeile.
#[derive(Debug, Args)]
pub struct Options {
    /// ABAP-Programm, das übersetzt und ausgewertet wird
    pub input: PathBuf,
    /// Vollständigen Bericht als JSON in diese Datei schreiben
    #[arg(long, value_name = "DATEI")]
    pub json: Option<PathBuf>,
    /// Anzahl der Namen, die in der Übersicht gezeigt werden
    #[arg(long, default_value_t = 15)]
    pub top: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ClassStat {
    pub names: usize,
    pub messages: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownName {
    pub name: String,
    pub classification: &'static str,
    pub inferred_kind: String,
    /// Anzahl der Verwendungszeilen je Muster
    pub usage_evidence: BTreeMap<&'static str, usize>,
    /// Anzahl der Meldungen je Fehlerart
    pub compiler_messages: BTreeMap<&'static str, usize>,
    pub message_count: usize,
    /// Strukturen, zu denen eine Komponente gemeldet wurde
    pub component_of: Vec<String>,
    /// Zeile der Deklaration im Quelltext (nur bei Compiler-Lücken)
    pub declared_at_line: Option<usize>,
    /// Art laut Anforderungsanalyse (nur bei externen Klassen und Interfaces)
    pub kind_from_requirements: Option<&'static str>,
    pub methods: Vec<String>,
    pub implements: Vec<String>,
    /// Beispielzeilen: (Zeile, Text)
    pub samples: Vec<(usize, String)>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub total_messages: usize,
    /// Meldungen mit Namensbezug, je Klasse
    pub by_classification: BTreeMap<&'static str, ClassStat>,
    pub names: Vec<UnknownName>,
    /// Meldungen ohne Namensbezug, nach Meldungsmuster gezählt
    pub other_messages: BTreeMap<String, usize>,
}

/// Fehlerart mit Namen aus einer Compilermeldung.
struct NameMessage {
    category: &'static str,
    name: String,
    component_of: Option<String>,
}

const MESSAGE_PATTERNS: &[(&str, &str)] = &[
    ("nicht_deklariert", r"^Feld '([^']+)' ist nicht deklariert$"),
    ("keine_objektreferenz", r"^'([^']+)' ist keine Objektreferenz$"),
    ("keine_tabelle", r"^'([^']+)' ist keine interne Tabelle$"),
    ("create_object_keine_referenz", r"^CREATE OBJECT erwartet eine Objektreferenz: '([^']+)'$"),
    ("methode_unbekannt", r"^Methode '([^']+)' unbekannt$"),
    ("attribut_unbekannt", r"^Attribut '([^']+)' unbekannt$"),
    ("komponente_unbekannt", r"^Komponente '([^']+)' unbekannt in '([^']+)'$"),
];

fn message_regexes() -> &'static [(&'static str, Regex)] {
    static RE: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    RE.get_or_init(|| {
        MESSAGE_PATTERNS
            .iter()
            .map(|(c, p)| (*c, Regex::new(p).expect("gültiges Muster")))
            .collect()
    })
}

fn quoted_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new("'[^']*'").expect("gültiges Muster"))
}

fn name_message(msg: &str) -> Option<NameMessage> {
    for (category, re) in message_regexes() {
        if let Some(c) = re.captures(msg) {
            return Some(NameMessage {
                category,
                name: c[1].to_lowercase(),
                component_of: c.get(2).map(|m| m.as_str().to_lowercase()),
            });
        }
    }
    None
}

/// Meldungsmuster ohne konkrete Namen, z.B. `Zahl kann nicht ...`.
fn message_template(msg: &str) -> String {
    quoted_re().replace_all(msg, "'_'").into_owned()
}

/// Sammelt alle Übersetzungsfehler eines Programms: zuerst das Lowering, danach die Codegen-Phase.
pub fn collect_errors(src: &str) -> Vec<CompileError> {
    match parse_source(src) {
        Err(errors) => errors,
        Ok(ir) => {
            let ctx = inkwell::context::Context::create();
            let mut cg = Codegen::new(&ctx);
            cg.emit_program(&ir).err().unwrap_or_default()
        }
    }
}

fn ci(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .expect("gültiges Muster")
}

fn kind_name(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Class => "class",
        TypeKind::Interface => "interface",
        TypeKind::ClassOrInterface => "class_or_interface",
        TypeKind::Conflict => "conflict",
    }
}

/// Verwendungsmuster je Name: Muster, Gewicht.
fn usage_rules(name: &str) -> Vec<(&'static str, Regex, usize)> {
    let e = regex::escape(name);
    vec![
        ("klasse_statisch", ci(&format!(r"\b{e}\s*=>")), 3),
        (
            "klasse_oder_interface",
            ci(&format!(r"(REF\s+TO|NEW|CATCH|RAISE\s+EXCEPTION\s+TYPE|CREATE\s+OBJECT\s+\S+\s+TYPE)\s+{e}\b")),
            3,
        ),
        ("interface", ci(&format!(r"\b{e}~")), 4),
        (
            "interne_tabelle",
            ci(&format!(
                r"(LOOP\s+AT|READ\s+TABLE|INTO\s+TABLE|APPEND\s+(TO\s+)?|INSERT\s+(TABLE\s+)?|DELETE\s+(TABLE\s+)?|LINES\s+OF)\s+{e}\b"
            )),
            2,
        ),
        ("objektreferenz", ci(&format!(r"\b{e}\s*->")), 2),
        ("struktur", ci(&format!(r"\b{e}-\w")), 1),
        ("zeichenfeld_teil", ci(&format!(r"\b{e}\s*\(\s*\d+\s*\)")), 1),
    ]
}

/// Art aus den Verwendungsmustern. `candidates` sind die Zeilen, die den Namen enthalten.
fn infer_kind(name: &str, lines: &[&str], candidates: &[usize]) -> (String, BTreeMap<&'static str, usize>) {
    let rules = usage_rules(name);
    let mut evidence = BTreeMap::new();
    let mut best: Option<(&'static str, usize, usize)> = None;
    for (kind, re, weight) in &rules {
        let found = candidates.iter().filter(|&&i| re.is_match(lines[i])).count();
        if found > 0 {
            evidence.insert(*kind, found);
            let better = match best {
                None => true,
                Some((_, w, f)) => (*weight, found) > (w, f),
            };
            if better {
                best = Some((*kind, *weight, found));
            }
        }
    }
    let kind = best.map_or("nicht_ableitbar".to_string(), |(k, _, _)| k.to_string());
    (kind, evidence)
}

/// Deklarationen im Quelltext: Name -> erste Zeile. Erfasst auch Parameter `VALUE(x) TYPE`,
/// Konstanten, Attribute, Feldsymbole und Klassen-/Interface-Definitionen.
fn declarations(full: &str) -> HashMap<String, usize> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        ci(r"\b(\w+)\)?\s+(TYPE|LIKE)\b|FIELD-SYMBOLS\s*:?\s*(<\w+>)|\b(?:CONSTANTS|DATA|TYPES|CLASS-DATA)\s*:?\s*(\w+)\b|\b(?:CLASS|INTERFACE)\s+(\w+)\s+(?:DEFINITION|IMPLEMENTATION|PUBLIC|PROTECTED|PRIVATE|DEFERRED|LOAD|\.)")
    });
    let mut out = HashMap::new();
    let mut line = 1usize;
    let mut last = 0usize;
    for c in re.captures_iter(full) {
        let m = c.get(0).expect("Gesamttreffer");
        line += full[last..m.start()].matches('\n').count();
        last = m.start();
        let name = [1, 3, 4, 5]
            .iter()
            .find_map(|&g| c.get(g))
            .map(|g| g.as_str().to_lowercase());
        if let Some(name) = name {
            out.entry(name).or_insert(line);
        }
    }
    out
}

/// Index: Bezeichner (klein) -> Zeilen, in denen er vorkommt.
fn token_index(lines: &[&str]) -> HashMap<String, Vec<usize>> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[<>\w]+").expect("gültiges Muster"));
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for t in re.find_iter(line) {
            seen.insert(t.as_str().to_lowercase());
        }
        for token in seen {
            index.entry(token).or_default().push(i);
        }
    }
    index
}

/// Erzeugt den Bericht zu einem Programm.
pub fn analyze(src: &str, program_name: &str) -> Report {
    let errors = collect_errors(src);
    let lines: Vec<&str> = src.lines().collect();
    let index = token_index(&lines);
    let decls = declarations(src);

    // Anforderungen der Verwendung: Klassen und Interfaces, die im Programm definiert oder extern sind
    let file = format!("{program_name}.prog.abap");
    let reqs = derive(&[(file, src.to_string())]);
    let in_export: BTreeSet<String> = reqs
        .items
        .iter()
        .filter(|r| r.in_export)
        .map(|r| r.name.clone())
        .collect();
    let externals: BTreeMap<String, Requirement> = reqs
        .items
        .iter()
        .filter(|r| !r.in_export)
        .map(|r| (r.name.clone(), r.clone()))
        .collect();

    // Im Programm deklarierte Methoden: Aufruf ohne Empfänger ist ein implizites me->
    let method_re = ci(r"\b(?:CLASS-)?METHODS\s+(\w+)");
    let method_names: BTreeSet<String> = method_re
        .captures_iter(src)
        .map(|c| c[1].to_lowercase())
        .collect();

    let mut report = Report {
        total_messages: errors.len(),
        ..Default::default()
    };

    // Namensbezogene Meldungen gruppieren
    type Group = (BTreeMap<&'static str, usize>, Vec<usize>, BTreeSet<String>);
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for e in &errors {
        match name_message(&e.message) {
            Some(nm) => {
                let entry = groups.entry(nm.name.clone()).or_default();
                *entry.0.entry(nm.category).or_insert(0) += 1;
                if entry.1.len() < 3 {
                    entry.1.push(e.line);
                }
                if let Some(s) = nm.component_of {
                    entry.2.insert(s);
                }
            }
            None => *report.other_messages.entry(message_template(&e.message)).or_insert(0) += 1,
        }
    }

    for (name, (categories, sample_lines, structs)) in groups {
        let base = name.split('-').next().unwrap_or("").to_string();
        let message_count: usize = categories.values().sum();
        let (classification, declared_at, kind_req) =
            classify(&base, &decls, &in_export, &externals, &method_names);
        let candidates: &[usize] = index.get(&base).map_or(&[], |v| v.as_slice());
        let (inferred, evidence) = match kind_req {
            Some(k) => (k.to_string(), BTreeMap::new()),
            None => infer_kind(&base, &lines, candidates),
        };
        let (methods, implements) = externals
            .get(&base)
            .map(|r| (r.methods.clone(), r.implements.clone()))
            .unwrap_or_default();
        let samples = sample_lines
            .iter()
            .filter_map(|&n| lines.get(n.wrapping_sub(1)).map(|t| (n, t.trim().chars().take(140).collect())))
            .collect();
        let stat = report.by_classification.entry(classification).or_default();
        stat.names += 1;
        stat.messages += message_count;
        report.names.push(UnknownName {
            name,
            classification,
            inferred_kind: inferred,
            usage_evidence: evidence,
            compiler_messages: categories,
            message_count,
            component_of: structs.into_iter().collect(),
            declared_at_line: declared_at,
            kind_from_requirements: kind_req,
            methods,
            implements,
            samples,
        });
    }
    report
        .names
        .sort_by(|a, b| a.classification.cmp(b.classification).then(b.message_count.cmp(&a.message_count)));
    report
}

/// Ordnet einen Namen einer Klasse zu. Liefert Klasse, Deklarationszeile und Art aus den Anforderungen.
fn classify(
    base: &str,
    decls: &HashMap<String, usize>,
    in_export: &BTreeSet<String>,
    externals: &BTreeMap<String, Requirement>,
    method_names: &BTreeSet<String>,
) -> (&'static str, Option<usize>, Option<&'static str>) {
    if BUILTIN.contains(&base) {
        return ("compiler_luecke_builtin", None, None);
    }
    if KEYWORDS.contains(&base) {
        return ("compiler_luecke_syntax_keyword", None, None);
    }
    if FUNCTIONS.contains(&base) {
        return ("abap_eingebaute_funktion", None, None);
    }
    if in_export.contains(base) {
        return ("im_export_definiert_compiler_kennt_es_nicht", None, None);
    }
    if method_names.contains(base) {
        return ("im_export_methode_ohne_empfaenger", None, None);
    }
    if let Some(r) = externals.get(base) {
        return ("extern_klasse_oder_interface", None, Some(kind_name(r.kind)));
    }
    if let Some(&line) = decls.get(base) {
        return ("im_quelltext_deklariert_compiler_kennt_es_nicht", Some(line), None);
    }
    ("unbekannt_extern_oder_fehlend", None, None)
}

fn print_summary(report: &Report, top: usize) {
    println!("Übersetzungsfehler gesamt: {}", report.total_messages);
    let named: usize = report.by_classification.values().map(|s| s.messages).sum();
    println!("davon mit Namensbezug: {named} in {} Namen\n", report.names.len());
    println!("{:<46} {:>6} {:>10}", "Klasse", "Namen", "Meldungen");
    for (class, stat) in &report.by_classification {
        println!("{class:<46} {:>6} {:>10}", stat.names, stat.messages);
    }
    println!("\nNamen der Klasse unbekannt_extern_oder_fehlend (Top {top}):");
    for n in report
        .names
        .iter()
        .filter(|n| n.classification == "unbekannt_extern_oder_fehlend")
        .take(top)
    {
        println!("  {:<34} [{}] {} Meldungen", n.name, n.inferred_kind, n.message_count);
    }
    println!("\nMeldungen ohne Namensbezug (Top {top}):");
    let mut others: Vec<_> = report.other_messages.iter().collect();
    others.sort_by(|a, b| b.1.cmp(a.1));
    for (msg, count) in others.into_iter().take(top) {
        println!("  {count:>6}  {msg}");
    }
}

/// Führt die Auswertung aus und gibt die Übersicht aus.
pub fn run(opts: Options) {
    let src = match std::fs::read_to_string(&opts.input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("abapc: {}: {e}", opts.input.display());
            std::process::exit(1);
        }
    };
    let name = Path::new(&opts.input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program")
        .to_lowercase();
    let report = analyze(&src, &name);
    print_summary(&report, opts.top);
    if let Some(path) = &opts.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                if let Err(e) = std::fs::write(path, json) {
                    eprintln!("abapc: {}: {e}", path.display());
                    std::process::exit(1);
                }
                println!("\nBericht geschrieben: {}", path.display());
            }
            Err(e) => {
                eprintln!("abapc: JSON-Ausgabe fehlgeschlagen: {e}");
                std::process::exit(1);
            }
        }
    }
}
