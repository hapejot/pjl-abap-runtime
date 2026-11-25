//! Gruppiert abapGit-Quelldateien zu Entwicklungsobjekten und sammelt deren Deklarationen.

use std::collections::{BTreeMap, BTreeSet};

use crate::inspect::abap::{declarations, parse_l1, Declaration};

#[derive(Debug, Clone)]
pub struct ObjectDecls {
    pub obj_type: String,
    pub name: String,
    /// (Dateiname, Deklaration), in Dateireihenfolge
    pub decls: Vec<(String, Declaration)>,
}

/// abapGit kodiert Namespace-Slashes als '#' (#ns#obj -> /ns/obj).
fn decode_name(raw: &str) -> String {
    raw.replace('#', "/").to_lowercase()
}

/// Eine geparste ABAP-Quelldatei mit ihrem Objekt.
struct SourceFile {
    obj_key: (String, String),
    file: String,
    decls: Vec<Declaration>,
}

/// Namen, unter denen eine Datei per INCLUDE ansprechbar ist.
/// `zcl_x.prog.abap` -> `zcl_x`; `zfg.fugr.lzfgtop.abap` -> `zfg` und `lzfgtop`.
fn stems(file: &str) -> Vec<String> {
    let parts: Vec<&str> = file.split('.').collect();
    let mut out = vec![parts[0].to_lowercase()];
    // Funktionsgruppen-Dateien tragen den Include-Namen im dritten Teil; bei Programmen
    // (z.B. screen_0100) ist dieser Teil kein Name und wird nicht indexiert.
    if parts.len() >= 4 && parts[1].eq_ignore_ascii_case("fugr") {
        out.push(parts[2].to_lowercase());
    }
    out
}

/// Ersetzt INCLUDE-Deklarationen durch die Deklarationen der inkludierten Datei.
/// `visited` verhindert Zyklen. Inkludierte Dateien werden in `consumed` vermerkt.
fn expand(
    decls: &[Declaration],
    sources: &[SourceFile],
    index: &BTreeMap<String, Vec<usize>>,
    visited: &mut Vec<usize>,
    consumed: &mut BTreeSet<usize>,
) -> Vec<Declaration> {
    let mut out = Vec::with_capacity(decls.len());
    for d in decls {
        let mut d = d.clone();
        if d.kind == "INCLUDE" {
            let target = index
                .get(&d.name)
                .and_then(|cands| cands.iter().copied().find(|j| !visited.contains(j)));
            if let Some(j) = target {
                visited.push(j);
                consumed.insert(j);
                d.children = expand(&sources[j].decls, sources, index, visited, consumed);
                visited.pop();
            }
        }
        out.push(d);
    }
    out
}

fn set_file(d: &mut Declaration, file: &str) {
    d.file = file.to_string();
    for c in d.children.iter_mut() {
        set_file(c, file);
    }
}

/// Eintrag für die Navigation: Methode, Funktion, Form, Klasse usw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Einrückungstiefe (0 = direkt unter dem Objekt)
    pub depth: usize,
    /// z.B. "METHODS run", "FORM fill", "FUNCTION zf", "CLASS zcl_x"
    pub label: String,
    /// Dateiname, in dem der Eintrag steht
    pub file: String,
    /// Zeile (1-basiert) der Deklaration in der Datei
    pub line: usize,
    /// Vollständige Deklaration, z.B. Methodensignatur (nur zur Anzeige)
    pub detail: String,
    /// Zeile der Implementierung; Ziel der Navigation, falls vorhanden
    pub impl_line: Option<usize>,
}

impl Entry {
    /// Zeile, zu der die Navigation springt: Implementierung, sonst Deklaration.
    pub fn target_line(&self) -> usize {
        self.impl_line.unwrap_or(self.line)
    }
}

const ENTRY_KINDS: &[&str] = &[
    "METHODS",
    "CLASS-METHODS",
    "FORM",
    "FUNCTION",
    "MODULE",
    "EVENT",
];

fn walk(d: &Declaration, depth: usize, out: &mut Vec<Entry>) {
    match d.kind.as_str() {
        // Inkludierter Inhalt steht auf derselben Ebene wie der INCLUDE
        "INCLUDE" => {
            for c in &d.children {
                walk(c, depth, out);
            }
        }
        "CLASS" | "INTERFACE" => {
            out.push(Entry {
                depth,
                label: format!("{} {}", d.kind, d.name),
                file: d.file.clone(),
                line: d.line,
                detail: d.detail.clone(),
                impl_line: None,
            });
            for c in &d.children {
                walk(c, depth + 1, out);
            }
        }
        k if ENTRY_KINDS.contains(&k) => out.push(Entry {
            depth,
            label: format!("{} {}", d.kind, d.name),
            file: d.file.clone(),
            line: d.line,
            detail: d.detail.clone(),
            impl_line: d.impl_line,
        }),
        _ => {}
    }
}

/// Methoden, Funktionen, Formen usw. eines Objekts in Quelltextreihenfolge, mit Einrückung.
pub fn entries(o: &ObjectDecls) -> Vec<Entry> {
    let mut out = Vec::new();
    for (_, d) in &o.decls {
        walk(d, 0, &mut out);
    }
    out
}

fn collect_files(d: &Declaration, out: &mut Vec<String>) {
    if !out.contains(&d.file) && !d.file.is_empty() {
        out.push(d.file.clone());
    }
    for c in &d.children {
        collect_files(c, out);
    }
}

/// Alle Dateien, aus denen Deklarationen des Objekts stammen (inkl. Includes).
pub fn source_files(o: &ObjectDecls) -> Vec<String> {
    let mut out = Vec::new();
    for (_, d) in &o.decls {
        collect_files(d, &mut out);
    }
    out
}

/// Objekt-Schlüssel (Typ, Name) aus einem Dateipfad. Nur ABAP-Dateien mit abapGit-Schema.
pub fn object_key(path: &str) -> Option<(String, String)> {
    let file = path.rsplit('/').next().unwrap_or(path);
    if !file.ends_with(".abap") {
        return None;
    }
    let parts: Vec<&str> = file.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((parts[1].to_uppercase(), decode_name(parts[0])))
}

/// Ordnet `(Pfad, Quelltext)`-Paare Objekten zu. Nicht-ABAP-Dateien werden ignoriert.
/// INCLUDEs werden an ihrer Stelle eingesetzt; die inkludierten Dateien erscheinen
/// dann nicht mehr als eigene Einträge.
pub fn collect(files: &[(String, String)]) -> Vec<ObjectDecls> {
    let mut sources: Vec<SourceFile> = Vec::new();
    for (path, src) in files {
        let Some(obj_key) = object_key(path) else {
            continue;
        };
        let file = path.rsplit('/').next().unwrap_or(path).to_string();
        let mut decls = declarations(&parse_l1(src));
        for d in decls.iter_mut() {
            set_file(d, &file);
        }
        sources.push(SourceFile {
            obj_key,
            file,
            decls,
        });
    }

    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in sources.iter().enumerate() {
        for stem in stems(&s.file) {
            index.entry(stem).or_default().push(i);
        }
    }

    // Wurzeln sind Dateien, die von niemandem inkludiert werden. In reinen Zyklen
    // gibt es keine solche Datei; dort wird die kleinste Datei als Wurzel erzwungen.
    let mut targets: BTreeSet<usize> = BTreeSet::new();
    for (i, s) in sources.iter().enumerate() {
        for d in s.decls.iter().filter(|d| d.kind == "INCLUDE") {
            if let Some(cands) = index.get(&d.name) {
                targets.extend(cands.iter().copied().filter(|&j| j != i));
            }
        }
    }
    let mut forced: BTreeSet<usize> = BTreeSet::new();
    let (roots, expanded) = loop {
        let roots: Vec<usize> = (0..sources.len())
            .filter(|i| !targets.contains(i) || forced.contains(i))
            .collect();
        let mut consumed: BTreeSet<usize> = BTreeSet::new();
        let mut expanded: BTreeMap<usize, Vec<Declaration>> = BTreeMap::new();
        for &i in &roots {
            let mut visited = vec![i];
            let decls = expand(
                &sources[i].decls,
                &sources,
                &index,
                &mut visited,
                &mut consumed,
            );
            expanded.insert(i, decls);
        }
        let lost = (0..sources.len()).find(|i| !roots.contains(i) && !consumed.contains(i));
        match lost {
            None => break (roots, expanded),
            Some(i) => {
                forced.insert(i);
            }
        }
    };

    let mut map: BTreeMap<(String, String), ObjectDecls> = BTreeMap::new();
    for i in roots {
        let s = &sources[i];
        let (obj_type, name) = s.obj_key.clone();
        let entry = map
            .entry((obj_type.clone(), name.clone()))
            .or_insert_with(|| ObjectDecls {
                obj_type,
                name,
                decls: Vec::new(),
            });
        entry
            .decls
            .extend(expanded[&i].iter().cloned().map(|d| (s.file.clone(), d)));
    }

    map.into_values().collect()
}

fn render_decl(out: &mut String, d: &Declaration, depth: usize) {
    out.push_str(&format!(
        "{}{} {} (L{})\n",
        "  ".repeat(depth),
        d.kind,
        d.name,
        d.line
    ));
    for c in &d.children {
        render_decl(out, c, depth + 1);
    }
}

/// Textdarstellung: Objekt, darunter pro Datei die Top-Level-Deklarationen.
pub fn render(objs: &[ObjectDecls]) -> String {
    let mut out = String::new();
    for o in objs {
        out.push_str(&format!("{} {}\n", o.obj_type, o.name));
        let mut last_file: Option<&str> = None;
        for (file, d) in &o.decls {
            if last_file != Some(file.as_str()) {
                out.push_str(&format!("  [{file}]\n"));
                last_file = Some(file.as_str());
            }
            render_decl(&mut out, d, 2);
        }
    }
    out
}
