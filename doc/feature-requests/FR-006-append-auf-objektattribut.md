# FR-006: APPEND auf ein Tabellenattribut eines Objekts

Status: offen
Beispiel: keines direkt. Gefunden beim Umbau von poc-crm (Mock-Prüfung in `poc-crm/test/`), nicht von poc-crm benötigt.
Stelle im Compiler: `abap-compiler/src/lower/stmt.rs:482` (ExternalOperation `append_object_attribute`)

## Problem
`APPEND ls_k TO lo_mock->mt_kunden.` (Tabellenattribut `mt_kunden` eines Objekts, öffentlich deklariert als
`DATA mt_kunden TYPE TABLE OF zkunde.`) wird übersetzt, ohne Fehler. Das Programm bricht zur Laufzeit mit
`abapc runtime: external function module 'append_object_attribute' not implemented` und Exit 78 ab.
Das ist ein stiller Fehler im Übersetzer: Die Anweisung wird nicht umgesetzt, aber auch nicht abgewiesen.

## Echtes ABAP
`APPEND` auf ein Tabellenattribut eines Objekts ist Standard, genau wie auf eine lokale Tabelle.
Belegt durch die ABAP-Schlüsselwortdokumentation (Stichwort „APPEND“, interne Tabellen als Attribut).
Die Fundstelle ist noch nicht im Repo festgehalten.

## Vorschlag für abapc
- Entweder `APPEND` auf Tabellenattribute umsetzen (bevorzugt, gleiche Logik wie bei lokalen Tabellen).
- Oder bis dahin einen Übersetzungsfehler mit Zeilenangabe ausgeben, statt einer Laufzeitabbruch-Anweisung.
  Dieselbe Regel gilt für die anderen ExternalOperation-Varianten auf Tabellenattributen (`INSERT`, `MODIFY`,
  `DELETE`, siehe FR-002 Stufe 3 und den Bericht zu B-1).

## Auswirkung
- `lower/stmt.rs` (Zweig `APPEND`, Zeile 482), `codegen/` (Tabellenattribut als Ziel).
- Akzeptanzfall: `APPEND` auf ein Tabellenattribut mit Ausgabe, und ein Fall für die Abweisung, falls die
  Umsetzung später kommt.
