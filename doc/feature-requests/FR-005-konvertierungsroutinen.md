# FR-005: Konvertierungsroutinen für Ausgabe und Eingabe

Status: umgesetzt (Stufe 1, eingeschränkt auf `ALPHA` bei `WRITE`; Eingabe und Dynpro offen, siehe unten)
Beispiel: poc-crm (noch nicht untersucht, welche Felder eine Routine brauchen)
Stelle im Beispiel: offen. Kandidaten sind Dynpro-Felder und Ausgaben mit Domänen, die eine Konvertierungsroutine haben.

## Problem
Die Umwandlung von Werten für Ausgabe und Eingabe ist heute im Laufzeitcode fest verdrahtet:

- Ganze Zahlen: `abap_rt_write_int` (`abap-rt/src/lib.rs`), rechtsbündig in 10 Stellen mit Minuszeichen danach.
- Zeichenketten: `abap_rt_write_chars`, volle Länge mit Auffüllung.

Ein Feld kann keine eigene Umwandlung für Ausgabe (`WRITE`, Dynpro) oder Eingabe (Dynpro, Parameter) haben.
Das betrifft zum Beispiel Nummern mit führenden Nullen. Ohne diese Möglichkeit muss jede Umwandlung im Programm
von Hand geschrieben werden.

## Echtes ABAP
Angabe des Nutzers (2026-11-28, als Projektwissen übernommen): ABAP arbeitet intern mit führenden Nullen. Die
Nullen werden erst bei der `WRITE`-Ausgabe durch Konvertierungsroutinen (Conversion Exits) umgewandelt. `WRITE`
wendet Konvertierungsroutinen an.

Einordnung beim Prüfen dieses FR:
- Die Konvertierungsroutine hängt an der Domäne im Data Dictionary. Die bekannteste ist `ALPHA` mit
  `CONVERSION_EXIT_ALPHA_INPUT` und `CONVERSION_EXIT_ALPHA_OUTPUT`.
- Für ganze Zahlen (`i`) gilt die Aussage „intern mit führenden Nullen“ nicht. `i` liegt intern binär. Die
  Ziffern-Darstellung entsteht bei der Umwandlung. Die Projektregel für `i` → `c` (FR-004) hängt daher nicht an
  einer Konvertierungsroutine.
- Ob `WRITE` jede Konvertierungsroutine anwendet oder nur bei DDIC-typisierten Feldern, ist im Dokument nicht
  weiter geprüft. Die Keyword-Dokumentation zu „Conversion exits“ ist der Belegort, falls das Dokument zur Umsetzung
  gebraucht wird.

## Vorschlag für abapc
- Stufe 1: Eine Umwandlung am Feld oder an der Domäne angeben, mit einer Ausgabe- und einer Eingabefunktion. Der
  Compiler übersetzt den Aufruf in einen Laufzeitaufruf.
- Stufe 2: Eingebaute Routinen für `ALPHA` (führende Nullen) und die Ganzzahl-Ausgabe aus FR-004 als Standardfall.
- Abgrenzung: Keine Routinen aus dem ABAP-Quelltext in der ersten Stufe. Der Compiler liest nur die Domänenangabe
  aus dem Data Dictionary (`ddic.rs`), keine Routinen selbst.

## Auswirkung
- `ddic.rs` (Domäne mit Konvertierungsroutine einlesen), `lower/` (Feldattribut), `codegen/stmt.rs` (`WRITE`),
  `codegen/screen.rs` (Dynpro-Feldübertragung), `abap-rt` (Laufzeitfunktionen für die Routinen).
- FR-004 Stufe 1 ist ohne FR-005 umsetzbar. FR-005 Stufe 2 würde die feste Regel für `i` → `c` durch eine
  Konvertierungsroutine ersetzen, wo eine Domäne sie angibt.
- Akzeptanzfälle: `WRITE` mit `ALPHA`-Domäne, Eingabe im Dynpro mit führenden Nullen, Ausgabe ohne Routine bleibt
  unverändert.

## Stand (2026-11-28)

Umgesetzt in Stufe 1:
- `abapc --ddic <Verzeichnis>` liest abapGit-XML (`*.doma.xml`, `*.dtel.xml`, `*.tabl.xml`) für eine einzelne ABAP-Datei.
  Bei ZIP-Exporten liest der Compiler das DDIC aus dem Export (unverändert).
- `ddic.rs`: Die Routine steht im Element `CONVEXIT` der Domäne (`DD01V`). Ein Datenelement erbt sie über seine Domäne.
  Festlegung: Feldname `CONVEXIT`. Belegt durch den Serializer-Quelltext des abapGit-Reports (`DD01V`, Feld `convexit`)
  und durch einen Export, der das Element enthält. Der Wert ALPHA für die Standarddomäne `MATNR` ist nicht im Repo
  belegt. Die Tests verwenden daher eine synthetische Domäne.
- `lower/`: `DATA ... TYPE <Datenelement>` mit Routine merkt sich die Routine. `WRITE <Variable>` übergibt sie an die
  IR. Eine unbekannte Routine ist ein Übersetzungsfehler bei der Deklaration (kein Fallback).
- `codegen/`: `WRITE` ruft für `ALPHA` die Laufzeitfunktion `abap_rt_write_convexit_alpha` auf.
- `abap-rt/src/convexit.rs`: ALPHA-Ausgabe (`abap_rt_write_convexit_alpha`) und ALPHA-Eingabe
  (`abap_rt_convexit_alpha_input`, bisher ohne Aufruf aus dem Codegen).

Akzeptanzfälle: `m22_alpha_write_domain`, `m22_write_without_domain`, `m22_error_unknown_conversion_routine`.
Erwartungswerte und Belege: `doc/sprache.md`, Abschnitt 5f.

## Festlegungen (Stufe 1, nicht belegt)

- Ausgabe bei Nicht-Ziffern: Der Wert bleibt unverändert. Vorzeichen, Buchstaben und führende Leerzeichen machen
  den Wert nicht numerisch. Nachfolgende Leerzeichen zählen nicht (belegt durch T11).
- Nur-Nullen ergibt `0`.
- Eingabe eines nicht-numerischen Werts: linksbündig mit Leerzeichen, bei Überlauf abgeschnitten.
- Unbekannte Routine: Übersetzungsfehler `Conversion routine '<NAME>' of data element '<element>' is not supported`.
- Die Ausgabebreite ist die Feldlänge, nicht `OUTPUTLEN`.

## Offen

- Eingabe (Dynpro-Feldübertragung, Selektionsbild, Parameterübergabe): nicht in Stufe 1. `abap_rt_convexit_alpha_input`
  ist vorhanden, der Codegen ruft sie nicht auf.
- Klassenattribute und Strukturkomponenten mit Domäne: nicht erfasst (nur `DATA`-Anweisungen).
- Weitere Routinen als ALPHA: nicht umgesetzt. Ihre Übersetzung ist ein Fehler.
- Genaues Verhalten bei Nicht-Ziffern, Blanks im Wert und Vorzeichen: nicht im Volltext der SAP-Doku geprüft
  (siehe Festlegungen). Der Quelltext der Funktionsbausteine `CONVERSION_EXIT_ALPHA_*` ist nicht gelesen.
- Die Ausgabebreite aus `OUTPUTLEN` ist offen.
- Der Zusammenhang zwischen Flat-Namen (`variable_conversions` ist nach Variablenname geführt, wie `data_types`) und
  gleichnamigen Parametern in Unterprogrammen ist nicht gesondert geprüft.
