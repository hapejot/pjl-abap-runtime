# ABAPC

Dieses Projekt beschreibt einen ABAP Compiler.
Es werden Quellen entweder direkt aus dem Filesystem oder aus einem ZIP mit der ABAPGIT Struktur genutzt.

Es sollen dabei auch Typedefinitionen aus dem Verzeichnis nachgelesen werden, die damit das Data-Dictionary des SAP nachbilden.
Aktuell besteht bereits der Parser auf zwei Ebenen und ein Scanner.

Ziel: Der ABAP-Standard so weit umsetzen, wie es sinnvoll ist. Dynpros werden als eigene Crossterm-Simulation nachgebildet. Die Screen-Beschreibung (`.dynp`, eigenes Layoutformat) wird vom Compiler in Screen-Runtime-Aufrufe übersetzt, genau wie ABAP-Anweisungen in Laufzeitaufrufe. Spezifikation: `doc/dynpro-spezifikation.md`.

Zu klären:
- Welches ist der Main-Report für den Compile-Lauf? Möglicherweise direkt auf der Kommandozeile angeben und alle weiteren Quellen über ein Verzeichnis oder ein ZIP nachlesen.

## Aufbau

- `abap-compiler` (Bibliothek `abap_compiler`, Binary `abapc`):
  - Lexer, Parser und Lowering: `inspect/abap/`, `lower/` (`tokens.rs`, `stmt.rs`, `block.rs`, `ir.rs`).
  - Analyse und Symbole: `analyze.rs`, `symbols.rs`.
  - DDIC: `ddic.rs`. Bibliotheken: `library.rs`. Projekte: `project.rs`.
  - Codegen mit `inkwell` (LLVM 21): `codegen/`.
  - abapGit-Inspect: `inspect/` (Modell, Objekte, CLI, TUI mit `crossterm`).
- `abap-rt` (Bibliothek `abap_rt`, `staticlib` und `rlib`): Laufzeit mit C-ABI. Zeichen in UTF-16, Dezimal (`decimal.rs`), gepackte Zahlen (`packed.rs`), Listausgabe, Ausnahmen, Laufzeitfehler. Symbolnamen und Signaturen müssen zum Codegen passen.
  - Dynpro-Laufzeit (`screen/`): Screen-Schleife, Feldübertragung, Funktionscodes, Terminal mit `crossterm` (Teil von `abap-rt`, kein eigener Crate), Headless-Backend für Tests.
  - Dynpro-Beschreibung (`.dynp`): Parser und Prüfung im Compiler, Übersetzung in `abap_rt_screen_*`-Aufrufe im Codegen.
- `tests/acceptance/<fall>/`: ABAP-Akzeptanzfälle. Runner: `abap-compiler/tests/acceptance.rs`.
- `abapgit-inspect/`: jq-Skripte und JSON-Auswertungen. Kein Rust-Crate.

## Befehle

- Formatierung: `cargo fmt --check`
- Lints: `cargo clippy --workspace --all-targets`
- Akzeptanzfälle: `cargo test -p abap-compiler --test acceptance`
- Weitere Tests: `cargo test -p abap-rt`, `cargo test -p abap-compiler`

Build-Voraussetzungen: LLVM 21, der Pfad steht in `.cargo/config.toml` (`LLVM_SYS_211_PREFIX`). Target ist Windows mit MSVC.

## Akzeptanzfälle

- Jeder Fall ist ein Verzeichnis unter `tests/acceptance/`, Präfix nach dem bestehenden Schema (`m1_`, `m2_`, `m3_`, `m10_` ...).
- `prog.abap`: ABAP-Quelltext.
- `expected.txt`: exakte stdout-Ausgabe des übersetzten Programms.
- `expected_error.txt`: Teil der Fehlermeldung, wenn die Übersetzung scheitern muss.
- Erwartungswerte stammen aus dem ABAP-Standard, oder aus eigener Vorgabe. In jedem Fall werden sie im Sprachdokument festgehalten.

## Nicht anfassen und nicht einchecken

- `libraries/` und `sample-archives/`: aus SAP-Systemen abgeleitete Daten. Inhalte nicht in Tests oder Code übernehmen.
- `zabapgit_standalone.prog.abap`: abapGit-Quelltext, 5 MB, nur gezielt mit `grep` lesen.
- Keine Passwörter, Zugangsdaten, API-Keys oder Zertifikate in Code, Tests oder Doku.
- Das Repo hat viele unkommittierte Änderungen aus anderer Arbeit. Keine Dateien außerhalb des Auftrags anfassen. Kein `git checkout`, `git reset` oder `git stash`, um Stände zu ändern. Nicht committen, wenn es nicht verlangt ist.

## Konventionen

- Doku-Kommentare auf Deutsch. Die Markdown-Doku in `doc/` bleibt vorerst deutsch.
- Alle Meldungen sind englisch: CLI-Ausgaben, Compiler-Fehler und -Warnungen, Laufzeitmeldungen (stderr, Laufzeitfehler, Terminaltexte der Dynpro-Laufzeit). Das gilt auch für neuen Code. Bestehende deutsche Meldungen werden noch umgestellt; die Akzeptanzfälle mit `expected_error.txt` und `expected_runtime_error.txt` werden dabei mit angepasst.
- Keine Sprachabhängigkeiten: keine Übersetzungstabellen, kein Message-Katalog, keine locale-abhängige Ausgabe. Meldungen sind feste englische Texte im Code.
- Texte aus dem ABAP-Quelltext oder aus dem Dynpro-Layout werden unverändert ausgegeben.
- Dezimaltrennzeichen ist fest `.` (bis eine Entscheidung dazu fällt).
- Dynpro-Tastenbelegung: Esc löst den Funktionscode `CANCEL` aus, wenn der GUI-Status ihn definiert.
- Commit-Nachrichten auf Deutsch (so in der Historie).
- Laufzeitfehler der ABAP-Ausführung (Überlauf, Division durch 0) sind Fehler, kein `panic!`.
- Mehrzeilige Änderungen mit dem Edit-Werkzeug, nicht mit `sed`/`perl`.
