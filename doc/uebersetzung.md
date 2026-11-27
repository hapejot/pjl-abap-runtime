# Übersetzung von ABAP-Programmen

Stand: 2026-11-27. Voraussetzung ist eine funktionierende LLVM-21-Installation und ein Build des
Workspaces. Details dazu stehen in [llvm-installation.md](llvm-installation.md).

## 1. Build

```
cargo build --workspace
```

Das erzeugt `abapc` (Binary) und die Laufzeitbibliothek `abap_rt.lib` (Windows MSVC) bzw.
`libabap_rt.a` (sonst) im selben Verzeichnis, z. B. `target/debug/`. Der Compiler sucht die
Laufzeit standardmäßig dort, neben seiner eigenen Binary. Ein anderer Pfad lässt sich mit
`--runtime` angeben.

Das `cargo build --workspace` ist wichtig: `cargo test -p abap-compiler` baut die Laufzeit-`staticlib`
nicht zwingend mit.

## 2. Aufruf

```
abapc <prog.abap> -o <ausgabe.exe> [--emit-ir <datei.ll>] [--runtime <laufzeit>]
abapc <export.zip> -o <ausgabe.exe> [--program <name>]
abapc inspect <export.zip> [...]        # abapGit-Export untersuchen
abapc analyze ...                       # Übersetzungsfehler auswerten
abapc library build ...                 # externe Bibliothek als JSON erzeugen
```

- `-o` ist Pflicht für die Übersetzung.
- `--emit-ir` schreibt die LLVM-IR-Datei. Das hilft beim Nachvollziehen der Codeerzeugung.
- Bei ZIP-Eingabe wird das Hauptprogramm über `--program` gewählt. Fehlt die Angabe, muss genau ein
  Programm mit `REPORT` im Export liegen.

## 3. Ablauf

Die Übersetzung in `abap-compiler/src/main.rs` läuft in dieser Reihenfolge:

1. **Einlesen**: Bei einer Datei `parse_source`, bei einem ZIP `project`. Der Scanner und der Parser auf
   zwei Ebenen (`inspect/abap/`) liefern Anweisungen mit Token und Zeilennummer.
2. **Includes**: bei ZIP werden Includes aufgelöst (`project::expand_includes`). Fehlende Includes
   werden nach den Regeln in `sprache.md` behandelt.
3. **DDIC**: Strukturen und Datenelemente aus dem Export werden geladen. Übersprungene Objekte werden
   als Hinweis auf stderr ausgegeben.
4. **Lowering**: Anweisungen werden in die Zwischensprache `IR` überführt (`lower/`, `ir.rs`). Dabei
   werden Symbole aufgelöst und Typen geprüft.
5. **Codegen**: Die IR wird über `inkwell` in ein LLVM-Modul übersetzt (`codegen/`).
6. **Objektdatei**: Das Modul wird für die Zielplattform des Hosts als `.o` geschrieben.
7. **Linken**: Unter Windows (MSVC) über `link.exe`, das über die Visual-Studio-Installation gefunden
   wird. Sonst über `cc` mit `-lpthread -ldl -lm -lgcc_s`. Die Objektdatei wird danach gelöscht.

Fehler aus den Schritten 1 bis 4 werden gesammelt, nach Zeile sortiert und gemeinsam ausgegeben. Der
Compiler bricht dann mit einem Fehler ab, ohne eine EXE zu erzeugen.

## 4. Fehlerausgabe

- Übersetzungsfehler tragen eine Zeilennummer und eine Meldung. Die Meldungen des Compilers sind im
  Code aktuell auf Deutsch (z. B. `Nicht unterstütztes Statement: SORT`, belegt durch `m1_unknown_stmt`).
  Laufzeitmeldungen der EXE sind auf Englisch. Das widerspricht der Konvention in
  `.claude/agents/rust-implementer.md` („Fehlermeldungen auf Englisch“). Welche Sprache gelten soll, ist
  offen und muss entschieden werden.
- Laufzeitfehler des übersetzten Programms beenden das Programm mit Exit-Code 1 und einer Meldung auf
  stderr. Die bisherige Ausgabe auf stdout wird vorher geleert.

## 5. Akzeptanzfall anlegen

Jeder Fall ist ein Verzeichnis unter `tests/acceptance/`:

```
tests/acceptance/<präfix>_<name>/
  prog.abap           ABAP-Quelltext
  expected.txt        exakte stdout-Ausgabe
  expected_error.txt  Teil der Fehlermeldung, wenn die Übersetzung scheitern muss
```

Schritte:

1. Das Präfix aus dem bestehenden Schema wählen (`m1_` Grundlagen, `m2_` Kontrollfluss, `m3_` Zeichen,
   `m4_` Tabellen, `m5_` Unterprogramme und Strukturen, `m6_` Methoden, `m7_` Ausgabe, `m8_`
   Ausnahmen, `m9_` Feldsymbole, `m10_` Laufzeitfehler).
2. Den Erwartungswert aus dem ABAP-Standard bestimmen. Nicht aus der aktuellen Ausgabe des Compilers.
   Die Quelle (Doku-URL, Systemtext) im Testnamen oder als Kommentar im `prog.abap` festhalten.
3. Ausführen: `cargo test -p abap-compiler --test acceptance`.

## 6. Neue Anweisung hinzufügen

Reihenfolge, die sich an den bestehenden Anweisungen orientiert:

1. Akzeptanzfall mit Erwartung aus dem ABAP-Standard anlegen (siehe Abschnitt 5). Der Fall muss zunächst
   fehlschlagen.
2. Die Anweisung in `abap-compiler/src/lower/stmt.rs` als Zweig im `match kw.as_str()` ergänzen. Dort
   werden Token geprüft und Fehlermeldungen mit `err(line, ...)` gebaut.
3. Falls nötig, eine neue Variante in `ast.rs` (Ausdrücke) bzw. `lower/ir.rs` (`Stmt`) ergänzen.
4. In `abap-compiler/src/codegen/stmt.rs` (bzw. `expr.rs`) die LLVM-Erzeugung ergänzen. Für Laufzeitaufrufe
   den Namen und die Signatur im Codegen eintragen.
5. Die Laufzeitfunktion in `abap-rt/src/lib.rs` (oder einem Untermodul) als `#[no_mangle] pub extern "C"`
   ergänzen. Laufzeitfehler über die bestehenden Muster melden (`abap_rt_arith_error` oder das
   Ausnahmemodell), nicht über `panic!`.
6. Unit-Tests, falls reine Logik dabei ist (`#[cfg(test)]`).
7. `cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo test` ausführen.
8. Die Beschreibung in [sprache.md](sprache.md) nachziehen: Anweisung und Status eintragen.

Die Regeln für Code-Stil stehen in `.claude/agents/rust-richtlinien.md`.
