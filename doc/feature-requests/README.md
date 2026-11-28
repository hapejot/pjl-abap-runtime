# Feature-Requests an den Compiler

Stand: 2026-11-28. Hier stehen Funktionen, die echtes ABAP hat und `abapc` noch fehlen. Ein Request
entsteht, wenn ein Beispiel (poc-crm, poc-finance) an einer Lücke hängt. Die Umsetzung entscheidet der
Nutzer. Der Compiler wird nicht still erweitert.

## Index

| Nr | Titel | Beispiel | Status |
|---|---|---|---|
| [FR-001](FR-001-interfaces-und-factory.md) | Interfaces als Referenztyp, `CREATE OBJECT ... TYPE`, Factory für DAOs | poc-crm | umgesetzt (Stufe 1, Methodentabelle pro Klasse; Stufe 2 Factory offen, Beispiel nicht geändert) |
| [FR-002](FR-002-tabellen-als-parameter.md) | Interne Tabellen als Methodenparameter und Klassenattribute | poc-crm | umgesetzt (Stufen 1 bis 3; Einschränkungen siehe FR-002) |
| [FR-003](FR-003-verkettete-aufrufe-und-komponentenzugriff.md) | Verkettete Methodenaufrufe, Komponentenzugriff auf Objektattribute | poc-crm | umgesetzt (Einschränkungen siehe FR-003) |
| [FR-004](FR-004-zahl-in-zeichenfeld.md) | Zuweisung einer Zahl an ein Zeichenfeld (eigene Regel nach SAP-Testlauf, kein `WRITE`) | poc-crm | umgesetzt (Offenes siehe FR-004; Beispiel poc-crm nicht geändert) |
| [FR-005](FR-005-konvertierungsroutinen.md) | Konvertierungsroutinen für Ausgabe und Eingabe (Domäne) | offen, Beispiel noch nicht untersucht | umgesetzt (Stufe 1: `ALPHA` bei `WRITE`, Eingabe und Dynpro offen; siehe FR-005) |

Reihenfolge der Umsetzung (festgelegt vom Nutzer, 2026-11-28): FR-002, FR-003, FR-001, FR-004, FR-005.

## Beobachtete Fehler

Keine Funktionen, sondern Fehlverhalten des Compilers. Sie gehören nicht in die Feature-Liste, werden aber
hier festgehalten, bis sie behoben sind.

- **B-1 (stiller Absturz)**: `SELECT ... INTO TABLE <param>` mit einem `EXPORTING`-Tabellenparameter wird ohne
  Fehler übersetzt. Das Programm bricht zur Laufzeit mit `0xC0000005` ab. Reproduktion: Methode mit
  `EXPORTING et TYPE TABLE OF zkunde` und `SELECT * FROM zkunde INTO TABLE et ...`. Gehört zu FR-002.
  Status: behoben. Der Absturz tritt nicht mehr auf, weil FR-002 Stufe 2 `SELECT ... INTO TABLE` in
  Tabellenparameter unterstützt (Akzeptanzfall `m18_table_param_select`). Zuerst war der Fehler eine
  Abweisung (siehe FR-002). Die früheren Fehlerfälle `m17_error_table_*` sind durch die `m18_*`-Fälle ersetzt
  (Details in FR-002 und `doc/sprache.md`, Abschnitt 5b). FR-002 ist umgesetzt.
- **B-2 (LLVM-Fehler)**: `Basic Block in function 'abap_module_pai_...' does not have terminator` bei einer
  `IF`-Bedingung mit Methodenaufruf (`IF obj->methode( ) = 'X'.`) im Zweig eines `CASE` in einem Modul.
  Beobachtet in `m16_crm` beim Umbau, nicht isoliert. Der Workaround ist, den Rückgabewert vorher in eine
  Variable zu schreiben.
  Status: behoben (Fix-Art: Codegen-Fehlerbehandlung in `codegen/stmt.rs` und `codegen/mod.rs`. Ein Fehler in
  einer Bedingung lässt die Blöcke von `IF`/`CASE` nicht mehr offen, und der LLVM-Prüfer läuft nur ohne
  gemeldete Fehler). Testfälle: `m17_if_method_in_case` (Mechanismus, Ausgabe), `m17_error_if_condition_in_case`,
  Unit-Test `codegen::stmt::tests::failed_call_in_if_condition_inside_case_reports_only_the_source_error`.
  Die Ursache im ursprünglichen poc-crm-Stand ist nicht eindeutig belegt; der Auslöser ist im Repo nicht erhalten.
- **B-3 (unklare Meldung)**: Ein Parameter mit dem Typ `sy-ucomm` (Systemfeld) führt zu
  `Type does not match formal parameter: 'ok_code'`. Die Meldung nennt den Variablennamen, nicht den
  Parameter. Der Workaround ist `TYPE c LENGTH 70`.
  Status: behoben (Fix-Art: `TYPE sy-...` und `LIKE sy-...` ergeben denselben festen Typ wie die Systemfelder,
  `sy-ucomm` ist `CHAR 70`; `lower/decl.rs`). Testfälle: `m17_sy_ucomm_parameter`, Unit-Test
  `lower::tests::system_field_type_is_the_same_for_parameter_and_declaration`.

## Vorlage

Datei: `FR-NNN-kurzer-slug.md`, NNN fortlaufend.

```markdown
# FR-NNN: <Titel in einem Satz>

Status: offen | in Arbeit | umgesetzt | verworfen
Beispiel: poc-crm | poc-finance
Stelle im Beispiel: <Datei:Zeile>, Workaround-Marker: `* WORKAROUND FR-NNN`

## Problem
Was das Beispiel braucht und warum der Workaround nicht reicht.

## Echtes ABAP
Wie das Verhalten im SAP-Standard aussieht, mit Beleg (Doku-URL oder Quelltextstelle).
Nicht belegte Angaben als Vermutung kennzeichnen.

## Vorschlag für abapc
Welche Sprachfunktion, welche Syntax, welche Semantik. Abgrenzung: was bewusst nicht gehört.

## Auswirkung
Betroffene Dateien im Compiler (grob), Akzeptanzfälle, die dazukommen würden.
```
