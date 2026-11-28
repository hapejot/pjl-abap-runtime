# CRM-Demo (ABAP, abapc)

Kleine CRM-Anwendung für den ABAP-Compiler: Kunden, Kontakte und Verkaufschancen in SQLite,
Bedienung über Dynpros (Terminal, crossterm). Am Ende schreibt das Programm einen Bericht als WRITE-Liste.

Dateien in diesem Ordner:

- `prog.abap`: Programm (Schichten, siehe unten)
- `prog.dynp`: Dynpros 0100 (Kunde), 0200 (Kontakt und Verkaufschance), 0300 (Bestätigung, Modalfenster)
- `db.sql`: Tabellen mit Primärschlüssel und Startdaten (frei erfundene Beispieldaten)
- `zu-pruefen.txt`: Erwartungen, die nicht aus dem ABAP-Standard belegt sind

Der Akzeptanzlauf liegt unter `tests/acceptance/m16_crm/`. Dort liegen Kopien von `prog.abap` und `prog.dynp`,
dazu `input.keys` (Eingabeskript) und die erwarteten Ausgaben.

## Aufbau des Programms

- **Dynpro** (Module in `prog.abap`): nur PBO- und PAI-Module. Sie kopieren über `to_dynpro` und `from_dynpro`
  zwischen den Dynpro-Feldern und `ms_maske` im View-Model und rufen das View-Model auf.
- **View-Model** (`lcl_crm_view`): Zustand der Bildschirme, Blättern und Entscheidungen zu den Tasten. Kein SQL.
- **Service** (`lcl_crm_service`): Anwendungsfälle und Transaktionsgrenzen (`COMMIT WORK`, `ROLLBACK WORK`).
  Kennt nur die DAO-Interfaces, die er per Konstruktor erhält.
- **Validierung** (`lcl_validator`): Prüfungen der Eingaben, Ausnahme `lcx_crm`.
- **DAOs**: je Tabelle ein Interface (`lif_dao_kunde`, `lif_dao_kontakt`, `lif_dao_chance`) und eine Implementierung
  mit Open SQL (`lcl_dao_kunde_sql`, `lcl_dao_kontakt_sql`, `lcl_dao_chance_sql`). Das einzige SQL. Listen
  kommen als Tabellenparameter, sortiert nach dem Schlüssel.
- **Factory** (`lcl_dao_factory`): statische Methoden liefern die DAOs als Interface-Referenz. Das Hauptprogramm
  holt sie hierüber.
- **Bericht** (`lcl_report`): WRITE-Liste.

Der Mock `lcl_dao_kunde_mock` liegt in `test/lcl_dao_kunde_mock.abap`. Er wird von `prog.abap` nicht verwendet
und wartet auf ABAP Unit. Die Feature-Requests FR-001 bis FR-004 sind im Beispiel umgesetzt; es gibt keine
`WORKAROUND`-Markierungen mehr.

## Übersetzen

Im Wurzelverzeichnis des Repos (LLVM 21 wie üblich):

    cargo build -p abap-rt
    cargo run -p abap-compiler --bin abapc -- tests/acceptance/m16_crm/prog.abap -o crm.exe

Die Dynpro-Datei `prog.dynp` liegt neben `prog.abap` und wird automatisch gefunden.

## Datenbank anlegen

Die Datenbank ist eine SQLite-Datei. Sie wird einmal aus `db.sql` erzeugt, zum Beispiel mit Python:

    python -c "import sqlite3; c = sqlite3.connect('crm.sqlite'); c.executescript(open('db.sql', encoding='utf-8').read()); c.commit()"

Die Datei `crm.sqlite` liegt neben `crm.exe`.

## Konfiguration

Neben `crm.exe` liegt die Datei `crm.toml`. Sie enthält keine Geheimnisse, nur den Adapter, den Pfad
(relativ zur Konfigurationsdatei) und die Tabellen:

    [connections.DEFAULT]
    adapter = "sqlite"
    path = 'crm.sqlite'

    [tables.ZKUNDE]
    connection = "DEFAULT"
    target = "zkunde"
    key = ["KUNNR"]

    [tables.ZKONTAKT]
    connection = "DEFAULT"
    target = "zkontakt"
    key = ["KNR"]

    [tables.ZCHANCE]
    connection = "DEFAULT"
    target = "zchance"
    key = ["CNR"]

Alternativ setzt man die Umgebungsvariable `ABAPC_DB_CONFIG` auf den Pfad der Datei.

## Starten

    crm.exe

Die Anwendung läuft im Terminal (80 × 24). Mit F3 wird das Hauptbild verlassen; danach erscheint der Bericht.

## Bedienung

Es werden nur die Funktionstasten F1 bis F12 verwendet. F4 ist die Wertehilfe (SAP-Standard) und hat nur
auf dem Feld Phase eine Wirkung.

Hauptbild Kunde (0100):

| Taste | Funktion |
|---|---|
| Eingabe (Enter) | Kundennummer geändert: diesen Kunden laden. Unbekannte Nummer: Neuanlage mit dieser Nummer |
| F2 | Sichern (Anlegen oder Ändern) |
| F3 | Zurück, Bericht |
| F4 | Wertehilfe (keine Wirkung auf diesem Bild) |
| F5 / F6 | vorheriger / nächster Kunde |
| F7 | neuer Kunde (Nummer vergibt das Programm) |
| F8 | Eingaben verwerfen |
| F9 | Kunde löschen, mit Bestätigung. Kontakte und Verkaufschancen werden mit gelöscht |
| F10 | Kontakte und Verkaufschancen des Kunden (Dynpro 0200) |

Kontakte und Verkaufschancen (0200): Der obere Block ist der Kontakt, der untere die Verkaufschance.

| Taste | Funktion |
|---|---|
| F2 | Kontakt löschen, mit Bestätigung |
| F3 | zurück zum Kunden. Ungesicherte Eingaben gehen verloren |
| F4 auf Phase | Wertehilfe: wechselt zwischen offen, gewonnen, verloren |
| F5 / F6 | vorherige / nächste Verkaufschance |
| F7 / F8 | vorheriger / nächster Kontakt |
| F9 | neuer Kontakt |
| F10 | neue Verkaufschance |
| F11 | Sichern: speichert die angezeigten Blöcke |
| F12 | Eingaben verwerfen, zurück zum Kunden |
| F1 | Verkaufschance löschen, mit Bestätigung |

Bestätigung (0300, Modalfenster): F5 Ja, F12 Nein.

Meldungen im Dynpro (Zeile 17 oder 19) und in der Meldungszeile (Zeile 24) zeigen Prüffehler, zum Beispiel
fehlender Name, E-Mail ohne @, Wert 0, Phase ohne Abschlussdatum.

## Funktionen

- Kunden: anzeigen, blättern, per Nummer wählen, anlegen, ändern, löschen (mit Kontakten und Verkaufschancen in einer LUW)
- Kontakte je Kunde: anlegen, ändern, löschen
- Verkaufschancen je Kunde: anlegen, ändern, löschen. Phase nur offen, gewonnen oder verloren. Wert größer 0.
  Eine abgeschlossene Chance wird nicht wieder offen.
- Validierung in der Klasse `lcl_validator`, Ausnahme `lcx_crm`, behandelt mit TRY/CATCH
- Wertehilfe für die Phase (F4), Modalfenster für die Bestätigung
- Bericht: offene Chancen mit Summe, gewonnene Chancen mit Summe, Umsatzanteil je Kunde

## Akzeptanzlauf

    cargo test -p abap-compiler --test acceptance

Der Fall `m16_crm` führt den Ablauf aus `input.keys` aus und vergleicht Bericht und Bildschirmabzüge.
Der Runner liest nur `prog.abap`, `prog.dynp`, `db.sql`, `input.keys`, die `expected*.txt`-Dateien und `zu-pruefen.txt`.
Diese README wird vom Runner nicht gelesen.
