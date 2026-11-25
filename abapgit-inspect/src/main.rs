//! abapgit-inspect: untersucht ZIP-Exporte aus abapGit.
//!
//! Aufruf:
//!   abapgit-inspect <datei.zip>                 Objektliste (Typ, Name, Dateien)
//!   abapgit-inspect <datei.zip> --files         alle Dateien im ZIP mit Groesse
//!   abapgit-inspect <datei.zip> --type CLAS     nur Objekte eines Typs
//!   abapgit-inspect <datei.zip> --show <pfad>   Inhalt einer Datei ausgeben
//!   abapgit-inspect <datei.zip> --model         Modell und Abhängigkeiten als JSON ausgeben

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

use clap::Parser;
use zip::ZipArchive;

fn read_entry(archive: &mut ZipArchive<File>, path: &str) -> String {
    let mut entry = archive.by_name(path).unwrap_or_else(|e| {
        eprintln!("Datei '{path}' nicht im ZIP gefunden: {e}");
        process::exit(1);
    });
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).expect("Lesefehler");
    let text = String::from_utf8_lossy(&buf).into_owned();
    text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
}

/// Untersucht ZIP-Exporte aus abapGit.
#[derive(Parser, Debug)]
#[command(name = "abapgit-inspect", version, about)]
struct Options {
    /// ZIP-Datei aus abapGit
    zip_path: String,

    /// Alle Dateien im ZIP mit Groesse auflisten
    #[arg(long)]
    files: bool,

    /// Nur Objekte dieses Typs (z.B. CLAS, DTEL, DOMA, DEVC)
    #[arg(long = "type", value_name = "TYP")]
    type_filter: Option<String>,

    /// Inhalt einer Datei im ZIP ausgeben (Pfad wie in --files)
    #[arg(long, value_name = "PFAD")]
    show: Option<String>,

    /// ABAP-Datei aus dem ZIP parsen und Gliederung ausgeben
    #[arg(long, value_name = "PFAD")]
    ast: Option<String>,

    /// Alle .abap-Dateien parsen und Fehler melden
    #[arg(long)]
    check: bool,

    /// Top-Level-Deklarationen je Entwicklungsobjekt ausgeben
    #[arg(long)]
    objects: bool,

    /// ABAP-, SQL-/DDIC-Modell und Abhängigkeiten aus dem ZIP als JSON ausgeben
    #[arg(long)]
    model: bool,

    /// Verwendungen als Kanten (Aufrufe, Vererbung, Typen je Member mit Datei und Zeile) als JSON ausgeben
    #[arg(long)]
    deps: bool,

    /// Extern benötigte Klassen und Interfaces, aggregiert je Typ, als JSON ausgeben
    #[arg(long)]
    external: bool,

    /// Interaktive Oberflaeche: Objektliste mit Filter und Quelltext
    #[arg(long)]
    tui: bool,
}

/// Alle `.abap`-Dateien als `(Pfad, Quelltext)`.
fn read_abap_files(archive: &mut ZipArchive<File>) -> Vec<(String, String)> {
    abap_entries(archive)
        .into_iter()
        .map(|n| {
            let src = read_entry(archive, &n);
            (n, src)
        })
        .collect()
}

fn read_table_metadata(archive: &mut ZipArchive<File>) -> Vec<(String, String)> {
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .filter(|n| n.to_ascii_lowercase().ends_with(".tabl.xml"))
        .collect();
    names
        .into_iter()
        .map(|name| {
            let xml = read_entry(archive, &name);
            (name, xml)
        })
        .collect()
}

/// Alle `.abap`-Dateien im ZIP in Archivreihenfolge.
fn abap_entries(archive: &mut ZipArchive<File>) -> Vec<String> {
    (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .filter(|n| n.ends_with(".abap"))
        .collect()
}

#[derive(Default)]
struct AbapObject {
    obj_type: String,
    name: String,
    files: Vec<(String, u64)>,
}

/// abapGit kodiert Namespace-Slashes als '#' im Dateinamen (#ns#obj -> /ns/obj).
fn decode_name(raw: &str) -> String {
    raw.replace('#', "/").to_lowercase()
}

/// Zerlegt einen Dateinamen wie `zcl_foo.clas.locals_imp.abap` in (Name, Typ).
/// Dateien ohne abapGit-Schema (z.B. `.abapgit.xml`) liefern None.
fn classify(file_name: &str) -> Option<(String, String)> {
    if file_name.starts_with('.') {
        return None;
    }
    let parts: Vec<&str> = file_name.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((decode_name(parts[0]), parts[1].to_uppercase()))
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

fn main() {
    let mut opts = Options::parse();
    opts.type_filter = opts.type_filter.map(|t| t.to_uppercase());

    let file = File::open(&opts.zip_path).unwrap_or_else(|e| {
        eprintln!("Kann '{}' nicht oeffnen: {e}", opts.zip_path);
        process::exit(1);
    });
    let mut archive = ZipArchive::new(file).unwrap_or_else(|e| {
        eprintln!("Kein gueltiges ZIP: {e}");
        process::exit(1);
    });

    // Einzelne Datei ausgeben
    if let Some(path) = &opts.show {
        let mut entry = archive.by_name(path).unwrap_or_else(|e| {
            eprintln!("Datei '{path}' nicht im ZIP gefunden: {e}");
            process::exit(1);
        });
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).expect("Lesefehler");
        let text = String::from_utf8_lossy(&buf);
        let mut out = io::stdout().lock();
        let _ = out.write_all(strip_bom(&text).as_bytes());
        return;
    }

    // ABAP-Datei parsen und Gliederung ausgeben
    if let Some(path) = &opts.ast {
        let src = read_entry(&mut archive, path);
        let prog = abapgit_inspect::abap::parse_l1(&src);
        print!("{}", abapgit_inspect::abap::outline(&prog));
        return;
    }

    // Top-Level-Deklarationen je Entwicklungsobjekt
    if opts.objects {
        let files = read_abap_files(&mut archive);
        let mut objs = abapgit_inspect::objects::collect(&files);
        if let Some(t) = &opts.type_filter {
            objs.retain(|o| &o.obj_type == t);
        }
        print!("{}", abapgit_inspect::objects::render(&objs));
        return;
    }

    if opts.model {
        let files = read_abap_files(&mut archive);
        let table_metadata = read_table_metadata(&mut archive);
        let inventory: Vec<String> = (0..archive.len())
            .filter_map(|i| {
                archive
                    .by_index(i)
                    .ok()
                    .filter(|entry| !entry.is_dir())
                    .map(|entry| entry.name().to_string())
            })
            .collect();
        let model = abapgit_inspect::model::analyze_with_ddic_and_inventory(
            &files,
            &table_metadata,
            &inventory,
        );
        match abapgit_inspect::model::render_json(&model) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON-Ausgabe fehlgeschlagen: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.external {
        let files = read_abap_files(&mut archive);
        let reqs = abapgit_inspect::abap::requirements::derive(&files);
        match serde_json::to_string_pretty(&reqs) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON-Ausgabe fehlgeschlagen: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.deps {
        let files = read_abap_files(&mut archive);
        let graph = abapgit_inspect::abap::graph::build(&files);
        match serde_json::to_string_pretty(&graph) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON-Ausgabe fehlgeschlagen: {e}");
                process::exit(1);
            }
        }
        return;
    }

    // Interaktive Oberflaeche
    if opts.tui {
        let files = read_abap_files(&mut archive);
        let mut objs = abapgit_inspect::objects::collect(&files);
        if let Some(t) = &opts.type_filter {
            objs.retain(|o| &o.obj_type == t);
        }
        // Quelltext je Objekt: eigene Dateien plus Dateien, die eingebettete Includes liefern
        let mut sources: abapgit_inspect::tui::app::Sources = Default::default();
        let by_name: std::collections::BTreeMap<&str, &str> = files
            .iter()
            .map(|(p, s)| (p.rsplit('/').next().unwrap_or(p), s.as_str()))
            .collect();
        let mut objects = Vec::new();
        for o in &objs {
            let key = (o.obj_type.clone(), o.name.clone());
            let mut names: Vec<String> = files
                .iter()
                .filter(|(p, _)| abapgit_inspect::objects::object_key(p).as_ref() == Some(&key))
                .map(|(p, _)| p.rsplit('/').next().unwrap_or(p).to_string())
                .collect();
            for f in abapgit_inspect::objects::source_files(o) {
                if !names.contains(&f) {
                    names.push(f);
                }
            }
            for f in names {
                if let Some(src) = by_name.get(f.as_str()) {
                    sources.entry(key.clone()).or_default().push((f, src.to_string()));
                }
            }
            objects.push(abapgit_inspect::tui::app::Object {
                key,
                entries: abapgit_inspect::objects::entries(o),
            });
        }

        if let Err(e) = abapgit_inspect::tui::run(objects, sources) {
            eprintln!("TUI-Fehler: {e}");
            process::exit(1);
        }
        return;
    }

    // Alle .abap-Dateien parsen und Fehler zusammenfassen
    if opts.check {
        let names = abap_entries(&mut archive);
        let mut total_errors = 0;
        for name in &names {
            let src = read_entry(&mut archive, name);
            let prog = abapgit_inspect::abap::parse_l1(&src);
            if !prog.errors.is_empty() {
                total_errors += prog.errors.len();
                println!("{name}: {} Fehler", prog.errors.len());
                for e in prog.errors.iter().take(5) {
                    println!("    L{}: {}", e.line, e.message);
                }
            }
        }
        println!("\n{} ABAP-Dateien geprueft, {} Fehler gesamt", names.len(), total_errors);
        return;
    }

    // Alle Dateien (Rohsicht)
    let mut entries: Vec<(String, u64)> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let e = archive.by_index(i).expect("ZIP-Eintrag");
        if !e.is_dir() {
            entries.push((e.name().to_string(), e.size()));
        }
    }

    if opts.files {
        println!("{:>10}  PFAD", "GROESSE");
        for (name, size) in &entries {
            println!("{size:>10}  {name}");
        }
        println!("\n{} Dateien", entries.len());
        return;
    }

    // Objekte gruppieren
    let mut objects: BTreeMap<(String, String), AbapObject> = BTreeMap::new();
    let mut other: Vec<String> = Vec::new();
    let mut meta: Vec<String> = Vec::new();

    for (path, size) in &entries {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        if file_name == ".abapgit.xml" {
            meta.push(path.clone());
            continue;
        }
        match classify(file_name) {
            Some((name, obj_type)) => {
                let obj = objects
                    .entry((obj_type.clone(), name.clone()))
                    .or_insert_with(|| AbapObject {
                        obj_type,
                        name,
                        files: Vec::new(),
                    });
                obj.files.push((path.clone(), *size));
            }
            None => other.push(path.clone()),
        }
    }

    let filtered: Vec<&AbapObject> = objects
        .values()
        .filter(|o| opts.type_filter.as_ref().map_or(true, |t| &o.obj_type == t))
        .collect();

    println!("ZIP: {}", opts.zip_path);
    if !meta.is_empty() {
        println!("Meta: {}", meta.join(", "));
    }
    println!();
    println!("{:<8} {:<40} {:>6}  DATEIEN", "TYP", "OBJEKT", "ANZ");
    println!("{}", "-".repeat(78));
    for o in &filtered {
        let names: Vec<&str> = o
            .files
            .iter()
            .map(|(p, _)| p.rsplit('/').next().unwrap_or(p))
            .collect();
        println!(
            "{:<8} {:<40} {:>6}  {}",
            o.obj_type,
            o.name,
            o.files.len(),
            names.join(" ")
        );
    }
    println!();

    // Zusammenfassung pro Typ
    let mut per_type: BTreeMap<&str, usize> = BTreeMap::new();
    for o in objects.values() {
        *per_type.entry(o.obj_type.as_str()).or_insert(0) += 1;
    }
    let summary: Vec<String> = per_type.iter().map(|(t, n)| format!("{t}={n}")).collect();
    println!("Objekte gesamt: {} ({})", objects.len(), summary.join(", "));
    println!("Gefilterte Ausgabe: {}", filtered.len());

    if !other.is_empty() {
        println!("\nNicht zuordenbare Dateien ({}):", other.len());
        for p in &other {
            println!("  {p}");
        }
    }
}
