# Sprache und Semantik

Stand: 2026-11-27. Quellen dieser Beschreibung: der Code in `abap-compiler/src/lower/`,
`abap-compiler/src/codegen/`, `abap-compiler/src/ddic.rs`, die Laufzeit `abap-rt/src/` und die
Akzeptanzfälle unter `tests/acceptance/` (157 Fälle).

## 1. Eingaben

- **Einzelne Datei**: `abapc prog.abap -o prog.exe`. Der Quelltext ist ein Report mit `REPORT`
  und `START-OF-SELECTION`.
- **abapGit-Export als ZIP**: `abapc export.zip -o prog.exe`. Hauptprogramm ist das einzige Programm
  mit `REPORT`, oder es wird mit `--program <name>` gewählt. Includes werden aufgelöst.
- **DDIC aus dem Export**: Strukturen (TABL) und Datenelemente (DTEL, DOMA) werden übernommen, soweit
  sie abbildbar sind. Typen: `CHAR`, `NUMC`, `CLNT`, `LANG`, `ACCP`, `CUKY`, `UNIT`, `DATS` (8 Zeichen),
  `TIMS`, `INT4`, `STRG`, `DEC`. Andere Typen führen dazu, dass das Objekt übersprungen wird; der Grund
  erscheint als Hinweis (`ddic.rs`).

## 2. Typen

| ABAP | Im Compiler | Bemerkung |
|---|---|---|
| `i` | 4-Byte-Ganzzahl (`Ty::Int`) | Überlauf ist Laufzeitfehler (Entscheidung, siehe 7) |
| `c` (`LENGTH n`) | Puffer fester Länge aus UTF-16-Einheiten | Länge in Zeichen, nicht Bytes (belegt: `m3_unicode_char_len`) |
| `string` | Handle auf `RtStr` | wird nie freigegeben, es gibt keinen GC (`abap-rt/src/lib.rs`) |
| `p` / `DEC` | gepackte Dezimalzahl, Ziffernarithmetik beliebiger Länge | Commit `2c190dc`; Speicherform BCD (`abap-rt/src/decimal.rs`) |
| `TABLE OF <typ>` | interne Tabelle (`RtTable`) mit Elementen `i`, `c`, `string` oder Struktur | Zeilen werden als Bytes kopiert |
| Struktur (`BEGIN OF ... END OF`, TABL) | feste Feldfolge im Speicher | Layout in `codegen/layout.rs` |
| `REF TO <klasse>` | Zeiger auf Objekt | `CREATE OBJECT` legt es an |
| Feldsymbole | `FIELD-SYMBOLS`, `ASSIGN`, auch dynamisch | belegt: `m9*`, `m4c_is_assigned` |

`n`, `d` und `t` werden über DDIC als Zeichenfelder fester Länge abgebildet (`NUMC`, `DATS`, `TIMS`).
Eine eigenständige Numerik für `n` gibt es nicht.

## 3. Zeichen, Längen und Vergleich

- Intern sind Zeichen UTF-16-Einheiten. Die Ausgabe erfolgt als UTF-8. Längen sind immer Zeichenzahlen.
- Zeichenfelder werden mit Leerzeichen aufgefüllt. Zuweisung kürzt auf die Feldlänge (`m3_char_truncate`).
- Beim Vergleich von `c`-Feldern werden nachgestellte Leerzeichen ignoriert (belegt:
  `m3_char_compare_padding`). Für `string` gilt das nicht.
- Teilfelder `name+off(len)` sind 0-basiert, wie in ABAP (`m3_offset_read`, `m3_offset_write`).
  Ein Offset außerhalb des Feldes ist ein Übersetzungsfehler (`m3_error_offset_out_of_range`).
- `CONCATENATE` mit `COMPRESSED` entfernt nachgestellte Leerzeichen der Operanden. Ohne
  `RESPECTING BLANKS` sind die Leerzeichen erhalten (`m3_concatenate_keeps_blanks`).

## 4. Ausdrücke und Operatoren

- Arithmetik `+`, `-`, `*`, `/`, `MOD`, unäres Minus, Klammern. Präzedenz ist getestet (`m1_precedence`).
- Division `/` bei `i` rundet kaufmännisch, Hälfte von Null weg (`m1_div_trunc`: `7 / 2` ergibt 4).
- `DIV` bei `i` schneidet die Nachkommastellen ab (`m1_div_keyword`: `7 DIV 2` ergibt 3). Negative Werte
  werden zur Null hin abgeschnitten; ob stattdessen immer nach unten gerundet werden soll, ist offen.
  Bei DEC-Feldern nutzt `DIV` bisher dieselbe Routine wie `/` und ist daher noch nicht korrekt.
- Verkettung `&&` und String-Templates `|...|` (`m3_string_concat_op`, `m3d_string_template`).
- Vergleiche `=`, `<>`, `<`, `>`, `<=`, `>=`; Logik `AND`, `OR`, `NOT`; `IS INITIAL` (`m2_cond_and_or_not`, `m2b_is_initial`).
- `LINES OF`, `STRLEN` und andere Ausdrucksfunktionen über den AST in `ast.rs`.

## 5. Anweisungen

Der Parser erkennt die folgenden Schlüsselwörter. Die Liste steht in `abap-compiler/src/lower/stmt.rs`.
Ob ein Schlüsselwort ausführbar ist, hängt vom Fall ab. Unbekannte Anweisungen führen zu einem
Übersetzungsfehler `Nicht unterstütztes Statement: <Name>` (belegt: `m1_unknown_stmt`, dort `SORT`).

**Ausführbar, mit Akzeptanzfällen:**

- Daten: `DATA`, `CONSTANTS ... VALUE`, `CLEAR`, `MOVE`, `MOVE EXACT`, `SET`, `FIELD-SYMBOLS`, `ASSIGN`, `UNASSIGN`
- Zeichenketten: `CONCATENATE`, `WRITE`
- Tabellen: `APPEND`, `READ TABLE` (mit `INDEX`, `ASSIGNING`), `DESCRIBE TABLE ... LINES`, `LOOP AT` (`m4_*`)
- Kontrollfluss: `IF`/`ELSEIF`/`ELSE`, `CASE`/`WHEN`/`OTHERS`, `DO ... TIMES`, `WHILE`, `EXIT`, `CONTINUE`, `CHECK`
- Unterprogramme: `PERFORM` mit `USING`/`CHANGING` (`m5_*`)
- Klassen und Methoden: `CLASS`, `INTERFACE`, `CREATE OBJECT`, `CALL METHOD`, Methodenaufrufe in Ausdrücken, `RETURNING` (`m5*`, `m6*`)
- Ausnahmen: `RAISE EXCEPTION`, `TRY`/`CATCH` (`m8_exceptions`)

**Erkannt, Verhalten nur teilweise belegt oder als Platzhalter:**

- `CALL FUNCTION`-Aufrufe: Ein Aufruf eines externen Funktionsbausteins endet mit Exit-Code 78 und der Meldung
  `externen Funktionsbaustein ... nicht implementiert` (`abap-rt/src/lib.rs`, `abap_rt_external_call`).
- `CALL FUNCTION`, `SUBMIT`, `EXPORT`/`IMPORT`, `RECEIVE`, `WAIT`,
  `AUTHORITY-CHECK`, `MESSAGE`, `CONVERT`, `FIND`/`SEARCH`/`SHIFT`/`SPLIT`/`CONDENSE`/`TRANSLATE`/`OVERLAY`,
  `REPLACE`, `COLLECT`, `DESCRIBE` (weitere Formen), `GET`: im Code erkannt, aber ohne
  Akzeptanzfall. Ihre Semantik ist daher nicht festgelegt.

**Ignoriert (kein Laufzeitverhalten):**

- `REPORT`, `TYPES`, `TABLES`, `PARAMETERS`, `SELECT-OPTIONS`, `SELECTION-SCREEN`, `CLASS ... DEFERRED`,
  `INTERFACE ... DEFERRED`.
- `INCLUDE ... IF FOUND`, wenn die Datei fehlt. Ein `INCLUDE` ohne `IF FOUND` mit fehlender Datei ist
  ein Fehler (`m5_include_unresolved`).

## 5a. Datenbankzugriff (Open SQL)

Die Datenbank ist eine Laufzeit-Schnittstelle mit austauschbaren Adaptern (SQLite, externer Prozess).
Der Compiler erzeugt pro Anweisung eine Zwischenstruktur (SQL-IR, JSON). Die Laufzeit führt sie aus. Die
Spezifikation steht in [sql-spezifikation.md](sql-spezifikation.md).

**Phase 1 (umgesetzt, Akzeptanzfälle `m11_*`):**

- `SELECT [SINGLE] <* | Spalten> FROM <tab> [WHERE …] [ORDER BY …] [UP TO n ROWS] INTO <ziel>` aus einer Tabelle,
  mit Zielen Arbeitsbereich, `@DATA(…)`, `INTO TABLE`, `APPENDING TABLE`.
- `SELECT … ENDSELECT` als Cursor. `SY-DBCNT` zählt die gelesenen Zeilen, `SY-SUBRC` ist 0 bei mindestens
  einer Zeile, sonst 4.
- WHERE mit `=`, `<>`, `<`, `<=`, `>`, `>=`, `BETWEEN`, `IN (…)`, `LIKE`, `AND`, `OR`, `NOT`, Klammern.
  Hostvariablen mit `@x` oder `:x`.
- Prüfung zur Übersetzungszeit: unbekannte Tabelle oder Spalte, Typkonflikt bei Vergleichen. Meldungen englisch,
  mit Zeile. `abapc --dump-sql <datei>` zeigt die IR.

**Phase 2 (umgesetzt, Akzeptanzfälle `m12_*`):**

- `INSERT` als Einzelsatz (`VALUES`, `FROM wa`) und `INSERT … FROM TABLE` (mit `ACCEPTING DUPLICATE KEYS`).
- `UPDATE … SET … WHERE …` und `UPDATE … FROM wa` (Zeile nach Schlüssel).
- `DELETE FROM … WHERE …`, `DELETE … FROM wa`, `DELETE FROM TABLE itab`.
- `MODIFY … FROM wa` und `MODIFY … FROM TABLE itab` (Upsert nach Schlüssel).
- `COMMIT WORK [AND WAIT]`, `ROLLBACK WORK`.
- `CORRESPONDING FIELDS OF` bei `SELECT`.
- `SY-DBCNT` zählt betroffene Zeilen. `SY-SUBRC` ist 4, wenn keine Zeile betroffen ist.

Dublette bei einem Einzelsatz-`INSERT`: `SY-SUBRC` 4, keine Ausnahme. Bei `INSERT FROM TABLE` ohne
`ACCEPTING DUPLICATE KEYS` bricht das Programm mit einem Laufzeitfehler ab (`duplicate key`) und die offene
LUW wird zurückgerollt. Eine Ausnahmeklasse `CX_SY_OPEN_SQL_DB` ist noch nicht umgesetzt.

Transaktionen: Eine LUW je Verbindung beginnt beim ersten schreibenden Zugriff. Offene LUWs werden am
Programmende und bei `LEAVE PROGRAM` zurückgerollt.

`LIKE` ist case-sensitiv wie in ABAP. Ein `ESCAPE` wird abgelehnt.

Übersetzung mit `abapc --db-config <datei>`: Der Compiler prüft die Fähigkeiten der Verbindungen zur
Übersetzungszeit. Verbindungen vom Typ `postgres` und `odata` sind in dieser Version nicht verfügbar und
führen zu einem Übersetzungsfehler. Verbindungen vom Typ `extern` werden erst zur Laufzeit geprüft.

Trace: Mit der Umgebungsvariablen `ABAPC_TRACE_SQL` gibt das Programm Dialekttext und Parameternamen auf
stderr aus, keine Feldwerte.

**Nicht unterstützt (Übersetzungsfehler):** Joins, Aliase mit `~`, `DISTINCT`, `OFFSET`, Aggregate,
`GROUP BY`, `HAVING`, `FOR ALL ENTRIES`, `CONNECTION`, `IS NULL`, Unterabfragen, `ESCAPE`, `STRING`-Spalten,
`INSERT` mit Spaltenliste, `UPDATE … FROM TABLE`.

Konfiguration: `<NAME>.toml` neben der EXE oder im Arbeitsverzeichnis, oder `ABAPC_DB_CONFIG`. Die Datei
enthält keine Geheimnisse, nur Namen von Umgebungsvariablen (Spezifikation, Abschnitt 9).

## 6. Ausgabe

- `WRITE` schreibt Felder hintereinander auf dieselbe Zeile. `/` beginnt eine neue Zeile
  (`m7_write_appends_same_line`, `m7_write_slash_*`). Ein Ende ohne Zeilenumbruch ist in
  `m7_write_no_trailing_for_empty` festgelegt.
- Ganze Zahlen werden rechtsbündig in 10 Stellen ausgegeben (`abap-rt/src/lib.rs`, `abap_rt_write_int`;
  belegt: `m1_write_int`). Negative Zahlen haben das Minuszeichen danach und damit 11 Zeichen. Das
  Alignment zwischen Feldern kommt über die Tabs (Abschnitt 7, Punkt 2).
  Die Breite 10 ist eine Projektfestlegung (Abschnitt 7, Punkt 2).
- Zeichenfelder werden mit ihrer vollen Länge ausgegeben, inklusive Auffüllung. Strings ohne Auffüllung
  (`m3_write_string_no_pad`).
- `m7_write_chain_slash` erwartet bei `WRITE: 'a', 'b', / 'c'.` eine Ausgabe mit Tabulatoren zwischen
  den Feldern. Das ist eine Projektfestlegung (Abschnitt 7, Punkt 3), kein ABAP-Standard.

## 7. Eigene Festlegungen, die nicht unbedingt mit dem SAP Standard übereinstimmen
1. **Breite der Ganzzahl-Ausgabe**: `WRITE` gibt eine ganze Zahl (`TYPE i`) rechtsbündig in 10 Stellen aus.
   Negative Zahlen bekommen das Minuszeichen danach, dadurch sind sie 11 Zeichen breit. Positive Zahlen
   bleiben bei 10 Zeichen. Nach Angabe des Nutzers entspricht das der Ausgabe im SAP-System.
2. **Tabulatoren bei `WRITE`**: Zwischen zwei Feldern steht die Folge Tabulator, Leerzeichen, Tabulator.
3. **Ganzzahl-Überlauf und Division durch 0**: Der Compiler bricht mit Exit-Code 1 ab und schreibt eine
   Meldung auf stderr (`m10_int_overflow`, `m10b_int_division_zero`). ABAP würde hier einen Laufzeitfehler
   mit Ausnahmeklasse auslösen. Die Meldung ist eine Vereinfachung und keine Nachbildung des Dumps.
4. Dynpros sind umgesetzt: `.dynp`-Layout, PBO/PAI, Wertehilfe `PROCESS ON VALUE-REQUEST`, Modalfenster
  (Akzeptanzfälle `m11_dynpro_*` bis `m15_dynpro_*`). Die Terminal-Simulation läuft mit `crossterm` in
  `abap-rt`. Die Spezifikation steht in `doc/dynpro-spezifikation.md`.

**Lücken:**

- Die Semantik der Anweisungen ohne Akzeptanzfall (Abschnitt 5) ist nicht festgelegt.
- Für Ausnahmen gibt es kein Verhalten für nicht gefangene Ausnahmen, das über den Abbruch hinausgeht.
  Das ist nicht belegt.
