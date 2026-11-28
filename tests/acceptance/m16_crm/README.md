# CRM-Demo (ABAP, abapc)

Kleine CRM-Anwendung für den ABAP-Compiler: Kunden, Kontakte und Verkaufschancen in SQLite,
Bedienung über Dynpros (Terminal, crossterm). Am Ende schreibt das Programm einen Bericht als WRITE-Liste.

Dateien in diesem Ordner:

- `prog.abap`: Programm (Logik, Validierung, Bericht)
- `prog.dynp`: Dynpros 0100 (Kunde), 0200 (Kontakt und Verkaufschance), 0300 (Bestätigung, Modalfenster)
- `db.sql`: Tabellen mit Primärschlüssel und Startdaten (frei erfundene Beispieldaten)
- `input.keys`, `expected.txt`, `expected_screen.txt`: Akzeptanzlauf
- `zu-pruefen.txt`: Erwartungen, die nicht aus dem ABAP-Standard belegt sind

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

Hauptbild Kunde (0100):

| Taste | Funktion |
|---|---|
| Eingabe (Enter) | Kundennummer geändert: diesen Kunden laden. Unbekannte Nummer: Neuanlage mit dieser Nummer |
| F5 | Kontakte und Verkaufschancen des Kunden (Dynpro 0200) |
| F7 / F8 | vorheriger / nächster Kunde |
| F9 | neuer Kunde (Nummer vergibt das Programm) |
| F11 | Sichern (Anlegen oder Ändern) |
| F14 | Kunde löschen, mit Bestätigung. Kontakte und Verkaufschancen werden mit gelöscht |
| F12 / Esc | Eingaben verwerfen |
| F3 | Zurück, Bericht |

Kontakte und Verkaufschancen (0200): Der obere Block ist der Kontakt, der untere die Verkaufschance.

| Taste | Funktion |
|---|---|
| F7 / F8 | vorheriger / nächster Kontakt |
| F9 | neuer Kontakt |
| F14 | Kontakt löschen, mit Bestätigung |
| F5 / F6 | vorherige / nächste Verkaufschance |
| F10 | neue Verkaufschance |
| F15 | Verkaufschance löschen, mit Bestätigung |
| F11 | Sichern: speichert die angezeigten Blöcke |
| F4 auf Phase | Wertehilfe: wechselt zwischen offen, gewonnen, verloren |
| F3 / F12 | zurück zum Kunden. Ungesicherte Eingaben gehen verloren |

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

Der Fall `m16_crm` führt den Ablauf aus `input.keys` aus und vergleicht Bericht und Gitterabzüge.
Der Runner liest nur `prog.abap`, `db.sql`, `input.keys`, die `expected*.txt`-Dateien und `zu-pruefen.txt`.
Diese README wird vom Runner nicht gelesen.
