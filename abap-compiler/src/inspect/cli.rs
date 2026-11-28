//! Untersucht ZIP-Exporte aus abapGit (Aufruf über `abapc inspect`).
//!
//! Aufruf:
//!   abapc inspect <datei.zip>                 Objektliste (Typ, Name, Dateien)
//!   abapc inspect <datei.zip> --files         alle Dateien im ZIP mit Groesse
//!   abapc inspect <datei.zip> --type CLAS     nur Objekte eines Typs
//!   abapc inspect <datei.zip> --show <pfad>   Inhalt einer Datei ausgeben
//!   abapc inspect <datei.zip> --model         Modell und Abhängigkeiten als JSON ausgeben

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

use clap::Args;
use zip::ZipArchive;

fn read_entry(archive: &mut ZipArchive<File>, path: &str) -> String {
    let mut entry = archive.by_name(path).unwrap_or_else(|e| {
        eprintln!("File '{path}' not found in the ZIP: {e}");
        process::exit(1);
    });
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).expect("read error");
    let text = String::from_utf8_lossy(&buf).into_owned();
    text.strip_prefix('\u{feff}')
        .map(str::to_string)
        .unwrap_or(text)
}

/// Inspects ZIP exports from abapGit.
#[derive(Args, Debug)]
pub struct Options {
    /// ZIP file from abapGit
    pub zip_path: String,

    /// List all files in the ZIP with size
    #[arg(long)]
    pub files: bool,

    /// Only objects of this type (e.g. CLAS, DTEL, DOMA, DEVC)
    #[arg(long = "type", value_name = "TYPE")]
    pub type_filter: Option<String>,

    /// Print the content of a file in the ZIP (path as in --files)
    #[arg(long, value_name = "PATH")]
    pub show: Option<String>,

    /// Parse an ABAP file from the ZIP and print its outline
    #[arg(long, value_name = "PATH")]
    pub ast: Option<String>,

    /// Parse all .abap files and report errors
    #[arg(long)]
    pub check: bool,

    /// Print top-level declarations per development object
    #[arg(long)]
    pub objects: bool,

    /// Print the ABAP, SQL/DDIC model and dependencies from the ZIP as JSON
    #[arg(long)]
    pub model: bool,

    /// Print usages as edges (calls, inheritance, types per member with file and line) as JSON
    #[arg(long)]
    pub deps: bool,

    /// Print the externally required classes and interfaces, aggregated per type, as JSON
    #[arg(long)]
    pub external: bool,

    /// Interactive UI: object list with filter and source text
    #[arg(long)]
    pub tui: bool,
}

/// Alle `.abap`-Dateien als `(Pfad, Quelltext)`.
pub fn read_abap_files(archive: &mut ZipArchive<File>) -> Vec<(String, String)> {
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

pub fn run(mut opts: Options) {
    opts.type_filter = opts.type_filter.map(|t| t.to_uppercase());

    let file = File::open(&opts.zip_path).unwrap_or_else(|e| {
        eprintln!("Cannot open '{}': {e}", opts.zip_path);
        process::exit(1);
    });
    let mut archive = ZipArchive::new(file).unwrap_or_else(|e| {
        eprintln!("Invalid ZIP: {e}");
        process::exit(1);
    });

    // Einzelne Datei ausgeben
    if let Some(path) = &opts.show {
        let mut entry = archive.by_name(path).unwrap_or_else(|e| {
            eprintln!("File '{path}' not found in the ZIP: {e}");
            process::exit(1);
        });
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).expect("read error");
        let text = String::from_utf8_lossy(&buf);
        let mut out = io::stdout().lock();
        let _ = out.write_all(strip_bom(&text).as_bytes());
        return;
    }

    // ABAP-Datei parsen und Gliederung ausgeben
    if let Some(path) = &opts.ast {
        let src = read_entry(&mut archive, path);
        let prog = crate::inspect::abap::parse_l1(&src);
        print!("{}", crate::inspect::abap::outline(&prog));
        return;
    }

    // Top-Level-Deklarationen je Entwicklungsobjekt
    if opts.objects {
        let files = read_abap_files(&mut archive);
        let mut objs = crate::inspect::objects::collect(&files);
        if let Some(t) = &opts.type_filter {
            objs.retain(|o| &o.obj_type == t);
        }
        print!("{}", crate::inspect::objects::render(&objs));
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
        let model = crate::inspect::model::analyze_with_ddic_and_inventory(
            &files,
            &table_metadata,
            &inventory,
        );
        match crate::inspect::model::render_json(&model) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON output failed: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.external {
        let files = read_abap_files(&mut archive);
        let reqs = crate::inspect::abap::requirements::derive(&files);
        match serde_json::to_string_pretty(&reqs) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON output failed: {e}");
                process::exit(1);
            }
        }
        return;
    }

    if opts.deps {
        let files = read_abap_files(&mut archive);
        let graph = crate::inspect::abap::graph::build(&files);
        match serde_json::to_string_pretty(&graph) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("JSON output failed: {e}");
                process::exit(1);
            }
        }
        return;
    }

    // Interaktive Oberflaeche
    if opts.tui {
        let files = read_abap_files(&mut archive);
        let mut objs = crate::inspect::objects::collect(&files);
        if let Some(t) = &opts.type_filter {
            objs.retain(|o| &o.obj_type == t);
        }
        // Quelltext je Objekt: eigene Dateien plus Dateien, die eingebettete Includes liefern
        let mut sources: crate::inspect::tui::app::Sources = Default::default();
        let by_name: std::collections::BTreeMap<&str, &str> = files
            .iter()
            .map(|(p, s)| (p.rsplit('/').next().unwrap_or(p), s.as_str()))
            .collect();
        let mut objects = Vec::new();
        for o in &objs {
            let key = (o.obj_type.clone(), o.name.clone());
            let mut names: Vec<String> = files
                .iter()
                .filter(|(p, _)| crate::inspect::objects::object_key(p).as_ref() == Some(&key))
                .map(|(p, _)| p.rsplit('/').next().unwrap_or(p).to_string())
                .collect();
            for f in crate::inspect::objects::source_files(o) {
                if !names.contains(&f) {
                    names.push(f);
                }
            }
            for f in names {
                if let Some(src) = by_name.get(f.as_str()) {
                    sources
                        .entry(key.clone())
                        .or_default()
                        .push((f, src.to_string()));
                }
            }
            objects.push(crate::inspect::tui::app::Object {
                key,
                entries: crate::inspect::objects::entries(o),
            });
        }

        if let Err(e) = crate::inspect::tui::run(objects, sources) {
            eprintln!("TUI error: {e}");
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
            let prog = crate::inspect::abap::parse_l1(&src);
            if !prog.errors.is_empty() {
                total_errors += prog.errors.len();
                println!("{name}: {} error(s)", prog.errors.len());
                for e in prog.errors.iter().take(5) {
                    println!("    L{}: {}", e.line, e.message);
                }
            }
        }
        println!(
            "\n{} ABAP files checked, {} error(s) total",
            names.len(),
            total_errors
        );
        return;
    }

    // Alle Dateien (Rohsicht)
    let mut entries: Vec<(String, u64)> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let e = archive.by_index(i).expect("ZIP entry");
        if !e.is_dir() {
            entries.push((e.name().to_string(), e.size()));
        }
    }

    if opts.files {
        println!("{:>10}  PATH", "SIZE");
        for (name, size) in &entries {
            println!("{size:>10}  {name}");
        }
        println!("\n{} files", entries.len());
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
    println!("{:<8} {:<40} {:>6}  FILES", "TYPE", "OBJECT", "COUNT");
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
    println!("Objects total: {} ({})", objects.len(), summary.join(", "));
    println!("Filtered output: {}", filtered.len());

    if !other.is_empty() {
        println!("\nUnassignable files ({}):", other.len());
        for p in &other {
            println!("  {p}");
        }
    }
}
