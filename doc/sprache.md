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

## 5b. Interne Tabellen als Parameter und Attribute (FR-002)

Umgesetzt in drei Stufen. Tabellenparameter (`IMPORTING`, `EXPORTING`, `CHANGING`, `RETURNING`), `FORM`-Parameter,
Tabellenzuweisung `tab = expr`, `SELECT ... INTO TABLE` in einen Tabellenparameter und Tabellenattribute
in Klassen. Die Belege sind Doku-Ausschnitte aus Suchtreffern (ABAP-Schlüsselwortdokumentation, Stichworte
„Internal tables“, „Method parameters“, „INTO TABLE“), nicht vollständig gelesene Seiten. Ein SAP-System
stand für den Abgleich nicht zur Verfügung. Die Quelle je Fall steht in der Spalte „Quelle“.

| Akzeptanzfall | Ausgabe bzw. Fehler | Quelle |
|---|---|---|
| `m18_table_param_importing` | `alpha`, `beta` (je Zeile) | ABAP-Standard: IMPORTING-Tabelle, Schleife über den Parameter |
| `m18_table_param_exporting` | `gamma`, `delta` | ABAP-Standard: EXPORTING-Tabelle. Die Methode leert mit `CLEAR`, daher hängt der Fall nicht an Festlegung 8 |
| `m18_table_param_changing` | `alt`, `neu` | ABAP-Standard: CHANGING-Tabelle, Änderung wirkt im Aufrufer |
| `m18_table_return` | `eins`, `zwei` | ABAP-Standard: RETURNING-Tabelle wird per Wert übergeben, Zuweisung `lt = o->make( )` |
| `m18_table_assign` | `a`, `b`, `c`, `original`, `a`, `b` | ABAP-Standard: `lt2 = lt` kopiert alle Zeilen. Der Inhalt des Ziels wird ersetzt. Eine spätere Änderung des Ziels wirkt nicht auf die Quelle |
| `m18_table_form_param` | `form` | Festlegung 5 (FORM-Tabellenparameter aktiv). Typisierung `TYPE TABLE OF` ist ABAP-Standard |
| `m18_table_param_select` | `0400`, `0401` | ABAP-Standard: `SELECT ... INTO TABLE` leert das Ziel, vorher gefüllte Zeile `9999` verschwindet. Ausgabe `n(4)` ist die Zeichenform der Spalte |
| `m18_table_attribute_loop` | `eins`, `zwei`, `o2` | ABAP-Standard: Tabellenattribut je Objekt. Das Wort `o2` ist eine Überschrift des Programms, danach ist das Objekt `o2` leer |
| `m18_table_attribute_select` | `0400`, `0401` | ABAP-Standard: `SELECT ... INTO TABLE` in ein Tabellenattribut, derselbe Ablauf wie `m18_table_param_select` |
| `m18_error_table_param_type` | `Type does not match formal parameter: 'lt'` | Festlegung 7 (Typprüfung). Der Fehlertext ist eine Projektfestlegung |
| `m18_error_importing_table_write` | `Importing parameter 'it' is read-only and cannot be changed` | ABAP-Standard: Schreiben auf IMPORTING ist ein Syntaxfehler (Doku-Ausschnitt). Der Fehlertext ist Projektfestlegung |
| `m18_error_importing_scalar_write` | `Importing parameter 'iv' is read-only and cannot be changed` | wie oben, für einen Skalar `TYPE i` |

## 5c. Verkettete Aufrufe und Komponentenzugriff auf Attribute (FR-003)

Umgesetzt: Aufrufketten `a->m( )->n( )`, `a->attr->m( )`, `a->m( )->attr` (auch als Anweisung), Lesen und
Schreiben einer Strukturkomponente über ein Objekt `obj->struktur-komponente` (auch verschachtelt, nur
Lesen über Ketten) sowie die Sichtbarkeitsprüfung für Attribute beim Zugriff von außen.
Die Belege sind Doku-Ausschnitte aus der ABAP-Schlüsselwortdokumentation (Stichworte „Method calls“,
„Object component selector“, „Structure component selector“, „PRIVATE“, „PROTECTED“). Gelesen wurden
Spiegelseiten (eduardocopat.github.io/abap-docs, Release 7.54), nicht die offizielle Seite. Ein SAP-System
stand für den Abgleich nicht zur Verfügung. Die Quelle je Fall steht in der Spalte „Quelle“.

| Akzeptanzfall | Ausgabe bzw. Fehler | Quelle |
|---|---|---|
| `m19_chain_two_calls` | `42` (Kette `o->get_inner( )->value( )`, auch als Anweisung ohne Ergebnis) | ABAP-Standard: Aufruf auf dem Rückgabewert (Stichwort „Method calls“). Ausgabeformat siehe Abschnitt 6 |
| `m19_chain_attribute` | `7` (Kette `o->mo_inner->value( )`) | ABAP-Standard: Attribut als Basis eines Aufrufs. Zuweisung `o->mo_inner = lo_in` ist Schreiben auf ein öffentliches Attribut |
| `m19_struct_component_read` | `40`, `3` | ABAP-Standard: Komponente über Objekt (Stichwort „Object component selector“), lesend |
| `m19_struct_component_write` | `5`, `50` | Schreiben `obj->struktur-komponente = wert` über dieselbe Regel. Belegt nur für Lesen; das Schreiben ist Vermutung, siehe Festlegung 3 |
| `m19_protected_in_subclass` | `5` | ABAP-Standard: PROTECTED-Attribut in Unterklasse über `me->` sichtbar (Stichwort „PROTECTED“) |
| `m19_error_protected_attribute_from_outside` | `Attribute 'mv_secret' is PROTECTED in class 'base' and not visible here` | ABAP-Standard: Zugriff von außen auf PROTECTED ist ein Syntaxfehler (Stichwort „PROTECTED“). Der Meldungstext ist Festlegung 4 |
| `m19_error_private_write` | `Attribute 'mv_secret' is PRIVATE in class 'base' and not visible here` | ABAP-Standard: PRIVATE nur in der Klasse selbst (Stichwort „PRIVATE“). Der Meldungstext ist Festlegung 4 |

**Festlegungen:**

1. **Sichtbarkeit vor FR-003**: Der Compiler prüfte die Sichtbarkeit weder bei `obj->attr` noch bei
   Methodenaufrufen. Mit FR-003 wird sie für Attribute beim Lesen und Schreiben geprüft, auch für
   Komponenten. Die Regel gilt für alle Zugriffe gleich. Methodenaufrufe werden weiterhin nicht geprüft (offen).
2. **PRIVATE** ist nur in der deklarierenden Klasse sichtbar. Friends sind nicht umgesetzt (belegt: Stichwort „Friends“).
3. **PROTECTED** ist in der deklarierenden Klasse und ihren Unterklassen sichtbar, und nur über eine Referenz,
   deren statischer Typ die zugreifende Klasse oder eine Unterklasse davon ist. Der Fall „Unterklasse greift über
   eine Referenz des Oberklassentyps auf PROTECTED zu“ ist in der Doku nicht eindeutig belegt. Die Festlegung ist
   bewusst streng: der Zugriff wird abgewiesen. Vor einer Änderung der Regel prüfen. Die Schreibrichtung der
   Komponente (`obj->struktur-komponente = wert`) ist nicht vollständig belegt, sie folgt der Regel für
   `obj->attr = wert`.
4. **Meldungen**: `Attribute '<name>' is <PROTECTED|PRIVATE> in class '<klasse>' and not visible here`. Der Text
   ist eine Projektfestlegung. Eine Anweisung ohne Klassenkontext (Programm, FORM) gilt als Zugriff von außen.
   Statische Methoden gelten als Klassenkontext ihrer Klasse.
5. **Initiale Referenz in der Kette**: Ein Zwischenwert ohne Objekt bricht mit dem Laufzeitfehler
   `object reference is not set (CX_SY_REF_IS_INITIAL)` ab (Exit-Code 1, kein Dump). Der Dump-Name ist belegt
   (Stichwort „Method calls“, Praxisbeleg). Ob der Fehler in ABAP behandelbar ist, ist widersprüchlich belegt
   (Doku 7.40 und 7.54). Der Compiler kennt keine Ausnahmebehandlung für diesen Fall.
6. **Attribut ohne Sichtbarkeitseintrag** (z. B. Interface-Attribute `intf~attr`, Attribute aus einer externen
   Oberklasse) gilt als öffentlich.

**Lücken (nicht umgesetzt):**

- Zuweisung an eine verkettete Stelle `a->b->c = wert` bleibt ein Übersetzungsfehler
  (`Assignment to chained attribute not supported yet`). Lesen über Ketten ist umgesetzt.
- Sichtbarkeit von Methodenaufrufen (`obj->privat( )`) wird nicht geprüft.
- Sichtbarkeit von Klassenattributen (`cls=>attr`) wird nicht geprüft.
- Unqualifizierter Zugriff einer Unterklasse auf ein PRIVATE-Attribut der Oberklasse (`attr` ohne `me->`) wird
  nicht abgewiesen. Der Zugriff über `me->` wird geprüft.

## 5d. Interface-Referenzen und Methodentabelle (FR-001, Stufe 1)

Umgesetzt: `TYPE REF TO <interface>` bei Attributen, Parametern, Rückgabewerten und lokalen Variablen;
`CREATE OBJECT <ref> TYPE <klasse>` mit Prüfung der Implementierung; Aufruf von Interface-Methoden über eine
Interface-Referenz über die Methodentabelle des Objekts; Zuweisung und Übergabe einer Klassenreferenz an eine
Interface-Referenz (Upcast). Nicht umgesetzt: CAST, Vererbung zwischen Interfaces als Zuweisung (Stufe 1),
statische Interface-Methoden über eine Referenz, Interface-Attribute über eine Referenz (`ref->attr`).
Die Factory (Stufe 2) ist ABAP-Code im Beispiel und steht nicht in diesem Abschnitt. Der Akzeptanzfall
`m20_iface_factory_return` zeigt sie als Compilerfall.

| Akzeptanzfall | Ausgabe bzw. Fehler | Quelle |
|---|---|---|
| `m20_iface_two_impls` | `add`, `x` | Zwei Klassen implementieren dasselbe Interface. Der Aufruf über die Referenz löst zur Laufzeit auf die Implementierung des Objekts auf. Der Parameter `iv` wird an die Implementierung übergeben. Belegt durch Doku-Auszug („Conversion Rules for References“, Upcast) |
| `m20_iface_mock_replaces_impl` | `mock`, `database` | Mock-Klasse ersetzt die echte Implementierung über den Konstruktor-Parameter `TYPE REF TO <interface>` (Upcast beim Aufruf). Zwei Service-Objekte mit unterschiedlichen Implementierungen |
| `m20_iface_attr_and_param` | `param ok`, `attr ok` | Interface-Referenz als Parameter (`IMPORTING`) und als Attribut, Rückgabe per `RETURNING ... TYPE REF TO`. Der Aufruf über das Attribut ist Aufruf über die Tabelle |
| `m20_iface_factory_return` | `mul`, `add` | Factory-Methode gibt `TYPE REF TO <interface>` zurück, die Implementierung wird im Rumpf per `CREATE OBJECT ro TYPE` gewählt |
| `m20_iface_inherited_table` | `base` | Unterklasse ohne eigene Implementierung erbt den Interface-Eintrag der Oberklasse (Festlegung 2). Zuweisung `lo = lc` ist Upcast über die geerbte Implementierung (Festlegung 3) |
| `m20_error_create_type_not_implemented` | `Class 'lcl_plain' does not implement interface 'lif_x'` | `CREATE OBJECT ... TYPE` mit einer Klasse, die das Interface nicht implementiert. Die Regel (Klasse muss implementieren) ist belegt (Auszug, „CREATE OBJECT“). Die Fehlerklasse (Syntax oder Laufzeit) ist nicht belegt; hier Übersetzungsfehler (Festlegung 4) |
| `m20_error_create_iface_without_type` | `an interface reference needs TYPE` | `CREATE OBJECT` ohne `TYPE` bei einer Interface-Referenz. Die Pflicht zu `TYPE` ist belegt (Auszug, „CREATE OBJECT“). Die Fehlerklasse ist nicht belegt (Festlegung 4) |

Unit-Tests: `lower::tests::interface_methods_are_qualified_and_sorted_with_sub_interfaces`,
`lower::tests::lowers_create_with_type_clause_and_arguments`, `codegen::iface::tests::implemented_closure_adds_sub_interfaces_once`.

**Festlegungen:**

1. **Methodentabelle**: Jedes Interface des Programms bekommt einen festen Offset: die Summe der Methodenzahlen
   der vorher definierten Interfaces. Die Methoden eines Interface (einschließlich der aus Unter-Interfaces
   übernommenen, qualifiziert `iface~methode`) sind nach dem qualifizierten Namen sortiert. Der Slot ist
   `Offset(Interface) + Index(Methode)`. Der Offset ist programmweit gleich, weil der Aufruf über eine
   Interface-Referenz den Offset zur Übersetzungszeit kennt, nicht den Klassentyp des Objekts. Eine Tabelle
   je Klasse hat Slots für alle Interfaces bis zum höchsten belegten Slot. Nicht belegte Slots sind null.
2. **Oberklasse (Regel)**: Die Tabelle einer Klasse beginnt als Kopie der Tabelle ihrer Oberklasse. Eigene
   Implementierungen von Interface-Methoden (`iface~methode`) überschreiben den Slot. Eine Unterklasse ohne
   eigene Implementierung erbt also den Eintrag. Diese Regel ist eine Projektfestlegung. Die Vererbung
   von Interface-Implementierungen ist in der Doku nicht vollständig belegt. Das Überschreiben über
   `METHODS iface~m REDEFINITION` in einer Unterklasse ist im Compiler verdrahtet, aber nicht durch einen
   Akzeptanzfall geprüft. Die Tabellenlogik überschreibt den Slot mit der eigenen Implementierung. Ob der Parser
   `METHODS iface~m REDEFINITION` in einer Unterklasse für Interface-Methoden annimmt, ist nicht geprüft.
   Die Doku-Auskunft (Spiegelseiten, nicht SAP-original) stützt das Überschreiben, die dynamische Auflösung
   über die Referenz ist Vermutung.
3. **Upcast**: Eine Klassenreferenz darf einer Interface-Referenz zugewiesen oder als Interface-Parameter
   übergeben werden, wenn die Klasse das Interface implementiert. Das gilt auch für geerbte Interfaces und
   für Interfaces, die das Interface über `INTERFACES` einbinden. Bei CHANGING muss der Typ gleich sein.
   Die Regel ist durch einen Doku-Auszug belegt („Conversion Rules for References“).
4. **Fehlerklasse**: Alle Verstöße gegen die Implementierungspflicht (Zuweisung, `CREATE OBJECT ... TYPE`,
   fehlendes `TYPE`) sind Übersetzungsfehler, weil die Klasse zur Übersetzungszeit bekannt ist. Ob ABAP
   dafür einen Syntaxfehler oder eine Laufzeitausnahme ausgibt, ist nicht belegt. Die Meldungstexte sind
   Projektfestlegung.
5. **Abstrakte Klasse**: `CREATE OBJECT` einer abstrakten Klasse ist ein Übersetzungsfehler
   (`... is abstract and cannot be instantiated`). Der Grund ist, dass Slots abstrakter Klassen nicht gefüllt
   sind. Die ABAP-Fehlerklasse ist nicht belegt (Festlegung 4).
6. **Objektkopf**: Ein Objekt wird mit 8 Byte Kopf vor dem Objektzeiger angelegt. Der Kopf enthält den Zeiger
   auf die Methodentabelle der Klasse. Der Objektzeiger zeigt auf die Attribute wie vorher. Eine Interface-
   Referenz ist derselbe Zeiger. Dies ist eine Umsetzungsfestlegung, nicht ABAP.
7. **Signatur**: Die Implementierung einer Interface-Methode muss Parameterzahl, Typ, CHANGING-Kennzeichen und
   Rückgabetyp der Interface-Methode haben. Sonst ist es ein Übersetzungsfehler (`does not match the signature
   of the interface method`). Parameternamen werden nicht geprüft. Der Aufruf über die Tabelle passt die
   Argumente nur über die Position an.
8. **Nicht getroffen**: Ein Aufruf über eine initiale Interface-Referenz verwendet die bestehende Prüfung
   (`object reference is not set`). Interface-zu-Interface-Zuweisung ist nur für das gleiche Interface
   zulässig. Zuweisung eines Oberinterface aus einem Unter-Interface ist ein Übersetzungsfehler.

**Lücken (nicht umgesetzt):**

- CAST (`?=`, `CAST`) und Zuweisung von Interface-Referenz an Klassenreferenz.
- Statische Interface-Methoden über eine Referenz (`ref->m( )` mit `CLASS-METHODS` im Interface): Fehler.
- Interface-Attribute über eine Referenz (`ref->attr`, Kettenschritt ohne Aufruf): Fehler.
- Überschreiben von Interface-Methoden in Unterklassen (`REDEFINITION`) ist ohne Akzeptanzfall.
- Keine Vererbung zwischen Interfaces als Zuweisung (Stufe 1).

## 5e. Zuweisung einer ganzen Zahl an ein Zeichenfeld (FR-004)

Die Zuweisung `TYPE i` → `TYPE c` ist keine Ausgabe mit `WRITE`. Sie folgt eigenen Regeln. Es gibt keinen
Übersetzungsfehler und keinen Laufzeitabbruch, auch bei Überlauf (Festlegung des Nutzers, 2026-11-28).

Quelle der Erwartungswerte: SAP-Testlauf S/4HANA 2025, Kernel 916, Angabe des Nutzers. Der Report steht in
`doc/feature-requests/sap-belegtests-fr004-fr005.md`, die Beobachtungen in `doc/feature-requests/FR-004-zahl-in-zeichenfeld.md`.
Eine Belegung in der Keyword-Dokumentation ist nicht gelesen und wird hier nicht behauptet.

Regel mit L = Feldlänge und d = Anzahl der Ziffern von |Wert| (0 hat eine Ziffer):
- positiv oder 0, d < L: Ziffern rechtsbündig in L−1 Stellen, letzte Stelle Leerzeichen.
- positiv, d = L: nur die Ziffern, keine Vorzeichenstelle.
- positiv, d > L: `*` vorne, danach die rechten L−1 Ziffern.
- negativ, d < L: Ziffern rechtsbündig in L−1 Stellen, letzte Stelle `-`.
- negativ, d ≥ L: `*` vorne, danach die rechten L−2 Ziffern, letzte Stelle `-`. Bei L = 1 bleibt nur `*`.

Erwartungswerte (in der Tabelle steht `_` für ein Leerzeichen, wie im SAP-Report). „Zeile“ heißt: die Zeile steht
wörtlich im Repo (FR-004, Abschnitt „Beispiele aus dem Lauf“). „Regel“ heißt: aus der Regel abgeleitet, eine
SAP-Zeile dazu liegt im Repo nicht vor.

| Fall | Wert | Feld | Erwartung | Beleg |
|---|---|---|---|---|
| T1 | 42 | c15 | `____________42_` (15 Zeichen) | Regel. Die Zeile in FR-004 hat 19 Zeichen, siehe Offenes |
| T1 | -42 | c15 | `____________42-` | Zeile |
| T2 | 42 | c10 | `_______42_` | Regel |
| T2 | -42 | c10 | `_______42-` | Regel |
| T3 | 42 | c9 | `______42_` | Regel |
| T3 | 42 | c5 | `__42_` | Regel |
| T4 | WRITE 42 | (WRITE) | `________42` (10 Zeichen) | Angabe in FR-004, Projektfestlegung Abschnitt 7, Punkt 1 |
| T4 | WRITE -42 | (WRITE) | `________42-` (11 Zeichen) | Angabe in FR-004, Projektfestlegung Abschnitt 7, Punkt 1 |
| T9 | 12345 | c5 | `12345` | Regel |
| T9 | -12345 | c5 | `*345-` | Zeile |
| T10 | 12345678 | c9 | `12345678_` | Zeile |
| T10 | 123456789 | c9 | `123456789` | Zeile |
| T10 | 123456789 | c10 | `123456789_` | Regel |
| T12 | 123456 | c5 | `*3456` | Zeile |
| T12 | -123456 | c5 | `*456-` | Zeile |
| T12 | 1234 | c5 | `1234_` | Zeile |
| T12 | 0 | c1 | `0` | Zeile |
| T12 | -5 | c1 | `*` | Zeile |

Akzeptanzfälle: `m21_t01_zahl_in_c15`, `m21_t02_zahl_in_c10`, `m21_t03_zahl_in_c9_und_c5`, `m21_t04_write_ganze_zahl`,
`m21_t09_ueberlauf_c5`, `m21_t10_grenzen_c9_c10`, `m21_t12_randwerte_c5_c1`. Die Ausgabe enthält keine `|`-Markierung
des Reports.

Abgrenzung: `WRITE` bleibt unverändert (Abschnitt 6, `m1_write_int`). Eine Dezimalzahl (`TYPE p`) an ein
Zeichenfeld ist weiterhin ein Übersetzungsfehler, weil dafür keine Regel belegt ist.

Nicht umgesetzt und ohne Akzeptanzfall: T5 (Typ `n`), T6 bis T8 und T11 (`MATNR` mit ALPHA-Konvertierung,
`CALL FUNCTION`). Dafür gibt es im Repo keine SAP-Ausgabe, und sie gehören zu FR-005.

Die Erwartungen T1 bis T12 stimmen mit der SAP-Ausgabe überein (Datei `doc/feature-requests/sap-belegtests-fr004-fr005.md`).
Offen: Die Länge 0 (`TYPE c LENGTH 0`) liefert eine leere Ausgabe. Das ist
eine Annahme und nicht belegt.

## 5f. Konvertierungsroutine ALPHA bei der Ausgabe (FR-005, Stufe 1)

Eine Domäne im Data Dictionary kann eine Konvertierungsroutine haben (Element `CONVEXIT` in `DD01V`, abapGit-Datei
`*.doma.xml`; belegt durch den Serializer-Quelltext des abapGit-Standalone-Reports, Abschnitt `DD01V`). Der Compiler
liest den Namen aus dem Data Dictionary. Routinen aus dem ABAP-Quelltext gibt es nicht.

Umgesetzt ist nur `ALPHA`. Die Anwendung in Stufe 1: `WRITE` einer Variable, die mit `DATA ... TYPE <Datenelement>`
deklariert ist, wenn das Datenelement eine Domäne mit der Routine `ALPHA` hat. Andere Ausgaben bleiben unverändert.
Eingabe (Dynpro, Selektionsbild, Parameter) ist nicht angebunden, auch wenn `abap-rt` die Eingaberegel hat.

Regel (ALPHA-Ausgabe):
- Ist der Wert rein numerisch (nur Ziffern `0` bis `9`, nachfolgende Leerzeichen zählen nicht), werden die führenden
  Nullen entfernt. Das Ergebnis ist linksbündig und rechts mit Leerzeichen auf die Feldlänge aufgefüllt.
- Belegt: Der SAP-Testlauf T11 gibt für MATNR mit gespeicherten Nullen `42` aus (`doc/feature-requests/sap-belegtests-fr004-fr005.md`).
  Der Doku-Auszug zu ALPHA nennt die Regel „rein numerisch“ und linksbündiges Ergebnis (SAP-Keyword-Doku, Suchtreffer,
  nicht im Volltext geprüft).
- Festlegung, nicht belegt: Nicht-numerische Werte bleiben unverändert. Nur-Nullen ergibt `0`. Führende Leerzeichen
  machen den Wert nicht numerisch. Die Regel für Vorzeichen und Buchstaben ist nicht im Volltext geprüft.
- Festlegung: Ohne Routine (Domäne ohne `CONVEXIT` oder Datenelement mit direktem Typ) gibt es keine Umwandlung.

Eingabe-Regel (in `abap-rt`, noch nicht vom Codegen aufgerufen): rein numerische Werte werden links mit Nullen auf
die Feldlänge aufgefüllt, rechtsbündig (belegt durch T8 `CONVERSION_EXIT_ALPHA_INPUT('42')` in c18 mit Nullen, und
die Doku-Auszüge). Nicht-numerische Werte: Festlegung, linksbündig mit Leerzeichen.

Erwartungswerte (in der Tabelle steht `_` für ein Leerzeichen):

| Fall | Programm | Erwartung | Beleg |
|---|---|---|---|
| `m22_alpha_write_domain`: `lv_nr` (Datenelement mit Domäne ALPHA, 18 Zeichen) = `'0000000000000042'`, `WRITE / lv_nr.` | Feld wird als `42` plus 16 Leerzeichen ausgegeben | T11 (Wert `42`). Ausgabebreite = Feldlänge: Festlegung |
| `m22_write_without_domain`: `lv_plain` (Datenelement ohne Domäne, 18 Zeichen) = `'000000000000000042'`, `WRITE / lv_plain.` | `000000000000000042` | T11 (c18 mit Nullen bleibt) |
| `m22_write_without_domain`: `lv_nr` (Domäne ohne Routine) = `'0000000000000042'`, `WRITE / lv_nr.` | `0000000000000042` plus 2 Leerzeichen | Festlegung: keine Routine, keine Umwandlung |
| `m22_error_unknown_conversion_routine`: Domäne mit Routine `ZTKV_UNBEKANNT`, `DATA lv_nr TYPE ztkv_nr.` | Übersetzungsfehler `Conversion routine 'ZTKV_UNBEKANNT' of data element 'ztkv_nr' is not supported` | Festlegung: kein stiller Fallback |

Akzeptanzfälle: `m22_alpha_write_domain`, `m22_write_without_domain`, `m22_error_unknown_conversion_routine`. Die
DDIC-Daten dieser Fälle sind synthetisch (Namen `ZTKV_*`) und liegen im Unterverzeichnis `ddic/` des Falls. Sie stammen
nicht aus den SAP-Beispielexporten. Der Runner übergibt das Verzeichnis mit `abapc --ddic`.

Abgrenzung: Die Feldlänge der Ausgabe ist die Länge des Feldes, nicht die `OUTPUTLEN` aus der Domäne (Festlegung,
offen). Klassenattribute und Strukturkomponenten sind in Stufe 1 nicht erfasst (nur `DATA`-Anweisungen).

## 6. Ausgabe

- `WRITE` schreibt Felder hintereinander auf dieselbe Zeile. `/` beginnt eine neue Zeile
  (`m7_write_appends_same_line`, `m7_write_slash_*`). Ein Ende ohne Zeilenumbruch ist in
  `m7_write_no_trailing_for_empty` festgelegt.
- Ganze Zahlen werden rechtsbündig in 10 Stellen ausgegeben (`abap-rt/src/lib.rs`, `abap_rt_write_int`;
  belegt: `m1_write_int`). Negative Zahlen haben das Minuszeichen danach und damit 11 Zeichen. Das
  Alignment zwischen Feldern kommt über die Tabs (Abschnitt 7, Punkt 2).
  Die Breite 10 ist eine Projektfestlegung (Abschnitt 7, Punkt 2).
- Die Zuweisung einer ganzen Zahl (`TYPE i`) an ein Zeichenfeld ist keine Ausgabe mit `WRITE`. Sie bildet das
  Verhalten des SAP-Systems nach (S/4HANA 2025, Kernel 916): Ziffern rechtsbündig, Vorzeichen am Ende
  (Leerzeichen oder `-`), bei Überlauf `*` vorne. Kein Fehler, kein Laufzeitabbruch. Die Regel mit allen Beispielen
  steht in FR-004.
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
5. **FORM-Tabellenparameter** (`FORM f USING it TYPE TABLE OF ...`) sind aktiv. Der Parser für FORM und Methoden
   ist derselbe. Der Auftrag nannte nur Methodenparameter (`m18_table_form_param`).
6. **Tabellenargument nur als Variable**: Ein Ausdruck wie `o->get( )` als Tabellenargument wird mit
   `Internal table argument must be a variable (not supported yet)` abgewiesen, weil der Zeilentyp eines
   Ausdrucks nicht geprüft werden kann.
7. **Typprüfung der Tabellen nur über den Zeilentyp**: Der Zeilentyp muss gleich sein (sonst
   `Type does not match formal parameter`). Die Tabellenart (`STANDARD`, `SORTED`, `HASHED`) und die
   Schlüsselangaben werden nicht geprüft. Nach der Doku wäre `STANDARD` an `SORTED` ein Syntaxfehler.
   Das ist nicht umgesetzt.
8. **EXPORTING-Tabellen werden beim Aufruf nicht geleert**: Der Aktualparameter wird per Referenz übergeben
   und behält seinen Inhalt. Die Doku-Auskunft stützt das nur teilweise (Forum, Blog und ein Ausschnitt).
   Kein Akzeptanzfall hängt daran.
9. **Schreibschutz für IMPORTING**: Eine Zuweisung an einen IMPORTING-Parameter (Skalar oder Tabelle) ist ein
   Übersetzungsfehler. Erkannt werden Zuweisungen, `CLEAR`, `APPEND`, `READ TABLE ... INTO`, `LOOP ... INTO`,
   `DESCRIBE`, `SELECT`-Ziele, `CREATE OBJECT`, `RECEIVING` und `CHANGING`-Argumente. Ein Schreiben über ein
   Feldsymbol (`LOOP ... ASSIGNING`) wird nicht erkannt. Ob der Speicher beim Aliasing (globale Variable und
   IMPORTING zeigen auf dasselbe Objekt) geschützt sein müsste, ist nicht belegt und nicht geprüft.

**Lücken:**

- Die Semantik der Anweisungen ohne Akzeptanzfall (Abschnitt 5) ist nicht festgelegt.
- Für Ausnahmen gibt es kein Verhalten für nicht gefangene Ausnahmen, das über den Abbruch hinausgeht.
  Das ist nicht belegt.
