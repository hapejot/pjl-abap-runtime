# FR-002: Interne Tabellen als Methodenparameter und Klassenattribute

Status: umgesetzt (Stufen 1 bis 3, siehe unten)

Stand der Umsetzung:
- Stufe 1 umgesetzt: Tabellenparameter IMPORTING, EXPORTING, CHANGING (auch FORM USING/CHANGING) und RETURNING, Typprüfung über den Zeilentyp, Tabellenzuweisung `tab = expr`. Akzeptanzfälle `m18_table_param_importing`, `m18_table_param_exporting`, `m18_table_param_changing`, `m18_table_return`, `m18_table_assign`, `m18_table_form_param`, `m18_error_table_param_type`.
- Stufe 2 umgesetzt: `SELECT ... INTO TABLE` in einen Tabellenparameter. Akzeptanzfall `m18_table_param_select`.
- Stufe 3 umgesetzt: Tabellenattribute in Klassen. Akzeptanzfälle `m18_table_attribute_loop`, `m18_table_attribute_select`.
- Schreibschutz für IMPORTING (Skalar und Tabelle): Zuweisung im Methodenrumpf ist ein Übersetzungsfehler
  (`m18_error_importing_table_write`, `m18_error_importing_scalar_write`). Ein Schreiben über ein Feldsymbol
  (`LOOP ... ASSIGNING`) wird nicht erkannt.
- Offen: Tabellenargument nur als Variable (kein Ausdruck wie `obj->get( )`), Tabellenart (STANDARD, SORTED, HASHED) wird nicht geprüft.
Beispiel: poc-crm (Akzeptanzfall `m16_crm`)
Stelle im Beispiel: alle DAO-Klassen und der Service. Kommentar `WORKAROUND FR-002` am DAO Kunde (`poc-crm/prog.abap:147`).

## Problem
Die Schichten sollen Listen über Schnittstellen zurückgeben (`EXPORTING et_kunden TYPE TABLE OF zkunde`).
Das geht in `abapc` nicht:

- `IMPORTING it TYPE TABLE OF zkunde` wird mit `Table parameter 'it' is not supported yet (see FR-002)`
  abgewiesen (`lower/decl.rs`).
- `EXPORTING et TYPE TABLE OF zkunde` wird ebenso abgewiesen. `RETURNING`-Tabellen werden mit
  `Table return value '...' is not supported yet (see FR-002)` abgewiesen. Der frühere stille Absturz
  (`EXPORTING` + `SELECT ... INTO TABLE`, Exit `0xC0000005`) ist behoben, siehe `README.md`, B-1.
- Ein Tabellenattribut in der Klassendefinition (`DATA mt_kontakte TYPE TABLE OF zkontakt.`) wird mit
  `Internal table attribute '...' is not supported yet (see FR-002)` abgewiesen (`codegen/data.rs`).

Der Workaround arbeitet mit Einzelsätzen: `SELECT SINGLE ... WHERE schluessel > @x ORDER BY schluessel`.
Listen entstehen im Service durch Schleifen. Das ist ABAP-konform, aber aufwendiger und langsamer.

## Echtes ABAP
Interne Tabellen als Parameter (`IMPORTING`, `EXPORTING`, `CHANGING`, `RETURNING`) und als Attribute von
Klassen sind Standard. `SELECT ... INTO TABLE` in eine Tabelle, die als Parameter übergeben wurde, ist
ebenfalls Standard. Belegt durch die ABAP-Schlüsselwortdokumentation (Stichworte „Internal tables“,
„Method parameters“). Die Fundstelle ist noch nicht im Repo festgehalten.

## Vorschlag für abapc
- Stufen: 1 Tabellenparameter (`IMPORTING`, `EXPORTING`, `CHANGING`) und Tabellenrückgabe (`RETURNING`), mit
  Typprüfung gegen den Zeilentyp. 2 `SELECT ... INTO TABLE` in Parameter. 3 Tabellenattribute in Klassen.
- Bis zur jeweiligen Stufe bleibt der Übersetzungsfehler bestehen, kein stiller Fehler.
- Abgrenzung: Typen für Tabellen (`TYPES ... TYPE STANDARD TABLE OF ... WITH DEFAULT KEY`) sind ein eigener Punkt.

## Auswirkung
- `lower/decl.rs` (Parameter- und Attributtypen), `codegen/layout.rs` (Speicher für Tabellenreferenzen).
- Die Testfälle `m17_error_table_*` prüfen die Abweisung. Jede Stufe ersetzt die passende Abweisung durch
  echte Unterstützung und passt den Testfall an (Akzeptanzfall mit Ausgabe statt `expected_error.txt`).
- Nach Umsetzung: Einzelsatz-Navigation im Beispiel durch Listen ersetzen (Zähler und Position werden einfacher).
