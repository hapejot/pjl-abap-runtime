//! abapgit-inspect: untersucht ZIP-Exporte aus abapGit.
//!
//! Aufruf:
//!   abapgit-inspect <datei.zip>                 Objektliste (Typ, Name, Dateien)
//!   abapgit-inspect <datei.zip> --files         alle Dateien im ZIP mit Groesse
//!   abapgit-inspect <datei.zip> --type CLAS     nur Objekte eines Typs
//!   abapgit-inspect <datei.zip> --show <pfad>   Inhalt einer Datei ausgeben

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

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

struct Options {
    zip_path: String,
    list_files: bool,
    type_filter: Option<String>,
    show: Option<String>,
    ast: Option<String>,
    check: bool,
}

#[derive(Default)]
struct AbapObject {
    obj_type: String,
    name: String,
    files: Vec<(String, u64)>,
}

fn usage() -> ! {
    eprintln!(
        "Verwendung: abapgit-inspect <datei.zip> [--files] [--type TYP] [--show PFAD]\n\
         \n\
           --files        alle Dateien im ZIP mit Groesse auflisten\n\
           --type TYP     nur Objekte dieses Typs (z.B. CLAS, DTEL, DOMA, DEVC)\n\
           --show PFAD    Inhalt der Datei im ZIP ausgeben (Pfad wie in --files)"
    );
    process::exit(2);
}

fn parse_args() -> Options {
    let mut args = env::args().skip(1);
    let mut opts = Options {
        zip_path: String::new(),
        list_files: false,
        type_filter: None,
        show: None,
        ast: None,
        check: false,
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => usage(),
            "--files" => opts.list_files = true,
            "--check" => opts.check = true,
            "--ast" => opts.ast = Some(args.next().unwrap_or_else(|| usage())),
            "--type" => opts.type_filter = Some(args.next().unwrap_or_else(|| usage()).to_uppercase()),
            "--show" => opts.show = Some(args.next().unwrap_or_else(|| usage())),
            s if s.starts_with("--") => {
                eprintln!("Unbekannte Option: {s}");
                usage();
            }
            _ => {
                if opts.zip_path.is_empty() {
                    opts.zip_path = a;
                } else {
                    usage();
                }
            }
        }
    }
    if opts.zip_path.is_empty() {
        usage();
    }
    opts
}

/// abapGit kodiert Namespace-Slashes als '#' im Dateinamen (#ns#obj -> /ns/obj).
fn decode_name(raw: &str) -> String {
    raw.replace('#', "/").to_uppercase()
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
    let opts = parse_args();

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
        let prog = abapgit_inspect::abap::parse(&src);
        print!("{}", abapgit_inspect::abap::outline(&prog));
        return;
    }

    // Alle .abap-Dateien parsen und Fehler zusammenfassen
    if opts.check {
        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
            .filter(|n| n.ends_with(".abap"))
            .collect();
        let mut total_errors = 0;
        for name in &names {
            let src = read_entry(&mut archive, name);
            let prog = abapgit_inspect::abap::parse(&src);
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

    if opts.list_files {
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
