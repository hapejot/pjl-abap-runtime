# SQL-Schnittstelle: Spezifikation (Entwurf 2)

Stand: 2026-11-28. Status: **Phase 1 und Phase 2 umgesetzt** (S1 und S2 ohne Ausnahmeklasse, SQLite und externer
Adapter). Phase 3 bis 5 stehen aus (Abschnitt 15). Was heute gilt, steht in [sprache.md](sprache.md),
Abschnitt 5a. Die Abweichungen und Annahmen aus Phase 2 stehen in Abschnitt 18.

Kennzeichnung wie in [README.md](README.md): **Entscheidung** ist eine Projektentscheidung, die nicht aus
dem ABAP-Standard stammt. **zu prüfen** ist Verhalten, das gegen den ABAP-Standard belegt werden muss (Abschnitt 17).

Die Doku bleibt vorerst deutsch. Alle Meldungen von Compiler und Laufzeit sind englisch (`CLAUDE.md`).

## 0. Ausgangslage im Code

| Stelle | Heute | Folge für diese Spezifikation |
|---|---|---|
| `abap-compiler/src/lower/stmt.rs` (`"SELECT" \| "UPDATE" \| "MODIFY"`) | `Stmt::ExternalOperation { name }` | Wird durch `Stmt::Sql(SqlRequest)` ersetzt (Abschnitt 5). |
| `abap-compiler/src/lower/block.rs` (`"SELECT"` als Block) | `select_loop` als externe Operation, Körper wird lowered | Wird zu einem Cursor mit `open`/`fetch`/`close` (Abschnitt 6.2). |
| `abap-compiler/src/inspect/abap/parser.rs` | `ENDSELECT`-Blöcke werden bereits erkannt | Parser-Ansatz wiederverwenden. |
| `abap-compiler/src/inspect/model.rs` | `SELECT`-Syntax ist für die abapGit-Inspektion modelliert (inkl. `@`-Hostvariablen, `JOIN`, `INTO TABLE @DATA(...)`) | Nur Vorlage für Tests. Die Inspektion bleibt getrennt vom Compiler. |
| `abap-compiler/src/lower/decl.rs` | `sy-subrc`, `sy-dbcnt` sind als Systemfelder bekannt | Werden von der Laufzeit beschrieben. |
| `abap-compiler/src/ddic.rs` | DDIC-Typen (TABL, DTEL, DOMA) werden gelesen | Quelle für Spaltentypen. Die Datenbank liefert keine Typen. |

## 1. Ziel und Abgrenzung

Ein ABAP-Programm soll Daten aus einer Datenbank oder einem Web-Service lesen und ändern können, mit der
bekannten Open-SQL-Syntax. Der Compiler kennt kein Datenbankprodukt. Welcher Adapter genutzt wird, entscheidet
die Konfiguration.

- **ABAP-kompatibel:** Die unterstützten Anweisungen haben die Syntax und die Semantik von Open SQL. Ein
  Programm aus diesem Umfang soll auf einem SAP-System ohne Änderung laufen. Abweichungen stehen in Abschnitt 4.4.
- **Adapterunabhängig:** Der Compiler erzeugt eine Zwischenstruktur (SQL-IR). Nur die Adapter in der Laufzeit
  erzeugen SQL-Text, HTTP-Aufrufe oder sonstige Zugriffe.
- **Fehler statt Stillschweigen:** Eine Anfrage, die ein Adapter nicht ausführen kann, ergibt einen Fehler an
  der Aufrufstelle. Sie wird nicht stillschweigend verkürzt oder umgeschrieben.
- **Nicht im Umfang:** Native SQL (`EXEC SQL`, `CONNECT`), CDS-Views, ABAP-SQL-Views, Pufferung von
  Tabellen, Berechtigungsprüfung (`AUTHORITY-CHECK`), Cursor-Verwaltung über mehrere Programmläufe,
  Datenbank-spezifische Funktionen, Schema-Änderungen (DDL).

## 2. Begriffe

| Begriff | Bedeutung |
|---|---|
| Open SQL | ABAP-Datenbankzugriff über `SELECT`, `INSERT`, `UPDATE`, `MODIFY`, `DELETE` |
| Adapter | Laufzeitmodul, das eine SQL-IR-Anfrage gegen ein Ziel ausführt (SQLite, PostgreSQL, OData) |
| SQL-IR | Zwischenstruktur des Compilers für jede Datenbankanfrage. Unabhängig von Dialekt und Protokoll. |
| Hostvariable | ABAP-Variable in einer Anfrage (`@lv_x`, `:lv_x`). Wird als Parameter gebunden, nie als Text eingesetzt. |
| Verbindung | Benannte Adapterinstanz, im Programm über den Namen gewählt (Abschnitt 9) |
| Fähigkeit (Capability) | Was ein Adapter ausführen kann, z. B. `join`, `aggregate`, `set_update` |
| LUW | Logische Arbeitseinheit: Änderungen bis `COMMIT WORK` oder `ROLLBACK WORK` |

## 3. Architektur

```
  prog.abap
     │  Parser (inspect/abap, lower/)
     ▼
  AST / Lowering ──► Analyse: Tabellen (DDIC), Spalten, Typen, Hostvariablen
     │
     ▼
  SQL-IR (SqlRequest)  ── Dump: abapc --dump-sql
     │  Codegen: IR als Konstante + Aufruf abap_rt_sql_*
     ▼
  Laufzeit abap-rt
     ├── Kern: Hostvariablen binden, Ergebnis in Zielbereich schreiben, SY-SUBRC/SY-DBCNT, Fehler
     └── Adapter (Trait / C-ABI-Tabelle, Auswahl über Verbindungsname)
           ├── sqlite      (eingebettete Datei, Dialekt SQLite)
           ├── postgres    (Netzwerk, Dialekt PostgreSQL)
           ├── odata       (HTTP, OData V4, Entitätsmenge je Tabelle)
           └── extern      (eigener Prozess, JSON über stdin/stdout, Abschnitt 6.4)
```

Grundsatz: Die gesamte ABAP-Semantik (Feldlängen, Padding, `SY-SUBRC`, Ergebniszuordnung, Typfehler)
liegt im **Kern** der Laufzeit. Adapter führen nur Anfragen aus und liefern typisierte Werte. So muss jeder
neue Adapter die ABAP-Regeln nicht neu umsetzen.

## 4. Sprachumfang

### 4.1 Stufen

| Stufe | Inhalt | Ziel |
|---|---|---|
| S1 | `SELECT` aus einer Tabelle: Spalten, `WHERE` mit `=`, `<>`, `<`, `<=`, `>`, `>=`, `BETWEEN`, `IN (…)`, `LIKE`, `AND`, `OR`, `NOT`, `ORDER BY`, `UP TO n ROWS`, `SINGLE`, `INTO` (Feld, `CORRESPONDING FIELDS OF`, `TABLE`), `APPENDING TABLE`, `SELECT … ENDSELECT` | erste Lieferung |
| S2 | `INSERT` (Zeile, `FROM TABLE`), `UPDATE` und `DELETE` mit `WHERE`, `MODIFY`, `SY-DBCNT`, `COMMIT WORK`, `ROLLBACK WORK` | Schreibzugriff |
| S3 | `INNER JOIN`, `LEFT OUTER JOIN`, Aliase (`AS`, `~`), `DISTINCT`, `OFFSET`, Aggregate (`COUNT`, `SUM`, `MIN`, `MAX`, `AVG`), `GROUP BY`, `HAVING` | Joins und Auswertung |
| S4 | Unterabfragen (`IN (SELECT …)`), `EXISTS`, `CONNECTION`-Zusatz | optional |

Die Stufe ist eine Obergrenze. Ein Adapter darf weniger können (Abschnitt 8).

### 4.2 Syntax

- Hostvariablen mit `@` (Syntax ab ABAP 7.40 SP05) oder klassisch mit `:`. Beide Formen werden geparst. Im
  Ergebnis ist kein Unterschied.
- `@DATA(lv_x)` und `INTO TABLE @DATA(lt_x)` sind in S1 zulässig, die Variable wird inline deklariert.
- Spaltenzugriff `alias~spalte`. Ohne Alias gilt der Tabellenname.
- Die Anweisungen werden zeilenweise im Quelltext geprüft: fehlende Tabelle, unbekannte Spalte oder
  Typkonflikt sind Übersetzungsfehler, wenn die DDIC-Information vorliegt.

### 4.3 Beispiele (S1 und S2)

```abap
DATA: lt_flug TYPE STANDARD TABLE OF zflug WITH EMPTY KEY,
      ls_flug TYPE zflug,
      lv_von  TYPE zflug-carrid.

lv_von = 'LH'.
SELECT * FROM zflug INTO TABLE @lt_flug
  WHERE carrid = @lv_von
  ORDER BY connid.

IF sy-subrc = 0.
  WRITE: / 'Treffer:', sy-dbcnt.
ENDIF.

SELECT SINGLE * FROM zflug INTO @ls_flug
  WHERE carrid = @lv_von AND connid = '0400'.
IF sy-subrc <> 0.
  WRITE: / 'Nicht gefunden'.
ENDIF.

SELECT * FROM zflug INTO @ls_flug WHERE carrid = @lv_von.
  WRITE: / ls_flug-connid.
ENDSELECT.

UPDATE zflug SET seatsmax = @ls_flug-seatsmax
  WHERE carrid = @lv_von AND connid = '0400'.
COMMIT WORK.
```

### 4.4 Abweichungen vom SAP-Standard (Entscheidungen)

| Thema | Entscheidung |
|---|---|
| Mandant (`MANDT`) | Entscheidung: Kein Mandantenfilter, auch nicht später im Kern. `MANDT` ist eine normale Spalte. Die Mandantenlogik ist eine Sache des SAP-Systems. |
| Pufferung | Keine. Jeder Zugriff geht an den Adapter. |
| `NULL` | Die Datenbank kennt `NULL`, ABAP nicht. `NULL` wird beim Lesen zum Initialwert des Zielfeldes. Beim Schreiben wird der Initialwert geschrieben. |
| Schemaauflösung | DDIC-Name (`zflug`) wird über die Verbindungskonfiguration auf den Namen im Ziel abgebildet (Abschnitt 9). |
| Zeichenketten | Wie in ABAP: `CHAR`-Felder werden rechts mit Leerzeichen aufgefüllt, `NUMC` bleibt Ziffernfolge. |

## 5. SQL-IR (Zwischenstruktur)

Jede Datenbankanweisung ergibt genau eine `SqlRequest`. Der Compiler erzeugt sie, der Kern und die Adapter
lesen sie. Die IR enthält keinen SQL-Text und keine Hostwerte.

### 5.1 Inhalt (Skizze in Rust-Notation)

```rust
pub struct SqlRequest {
    pub line: u32,                    // Quelltextzeile, für Fehlermeldungen
    pub connection: String,           // "DEFAULT" oder CONNECTION-Name
    pub op: SqlOp,
}

pub enum SqlOp {
    Select(SelectOp),
    Insert(InsertOp),                 // Zeile oder FROM TABLE
    Update(UpdateOp),
    Delete(DeleteOp),
    Modify(ModifyOp),
    Commit,
    Rollback,
}

pub struct SelectOp {
    pub sources: Vec<TableRef>,       // mehrere nur mit Join (S3)
    pub joins: Vec<Join>,
    pub projection: Projection,       // All | Columns(Vec<ColumnRef>) | Aggregates(...)
    pub distinct: bool,
    pub filter: Option<Expr>,
    pub group_by: Vec<ColumnRef>,
    pub having: Option<Expr>,
    pub order_by: Vec<OrderItem>,
    pub limit: Limit,                 // None | Single | UpTo(n) | Offset(n)
    pub result: ResultMode,           // Single | Table | Append | Loop (ENDSELECT)
    pub target: Layout,               // Spalten -> Offset, Länge, ABAP-Typ im Zielbereich
    pub columns: Vec<ColumnType>,     // Typ jeder Ergebnisspalte, aus DDIC
}

pub enum Expr {
    Column(ColumnRef),                // alias.spalte
    Param(ParamSlot),                 // Index in params
    Literal(TypedValue),              // nur Konstanten aus dem Quelltext
    Compare { op: CmpOp, left: Box<Expr>, right: Box<Expr> },
    Between { value: Box<Expr>, low: Box<Expr>, high: Box<Expr> },
    In { value: Box<Expr>, list: Vec<Expr> },
    Like { value: Box<Expr>, pattern: Box<Expr> },
    IsNull(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

pub struct ParamSlot {
    pub index: u32,
    pub name: String,                 // nur für Dump und Fehlermeldungen
    pub abap_type: AbapType,          // Typ der Variable (Compiler kennt ihn)
    pub offset: u32,                  // Position im Variablenbereich des Programms
    pub length: u32,
}
```

Regeln für die IR:

- Der Typ jeder Hostvariable und jeder Spalte ist vollständig in der IR. Ein Adapter muss den ABAP-Typ
  nicht selbst bestimmen.
- Literale aus dem Quelltext sind bereits typisiert. Gleichheitsvergleiche zwischen Feldtyp und Literal
  prüft der Compiler.
- Die IR ist versioniert (`ir_version`), damit Compiler und Laufzeit gegeneinander geprüft werden können.

### 5.2 Kodierung

**Entscheidung:** Die IR wird als JSON-Text in eine Konstante geschrieben. Die Laufzeit dekodiert
sie beim ersten Ausführen und hält das Ergebnis im Cache. Begründung: lesbar, testbar, `abapc --dump-sql` zeigt
genau das, was ausgeführt wird. Codegen für verschachtelte Strukturen mit `inkwell` wäre aufwendiger.
Ein kompaktes Binärformat ist später möglich, wenn die Dekodierung messbar stört. Dasselbe JSON dient
als Protokoll zu externen Adapterprozessen (Abschnitt 6.4).

Die Laufzeit parst also keinen ABAP-Quelltext, nur die vom Compiler erzeugte IR (wie bei Dynpros).

## 6. Laufzeit-Schnittstelle

### 6.1 Adapter-Vertrag

Jeder Adapter implementiert dieselbe Schnittstelle (intern eine Rust-Trait, nach außen über die C-ABI-Funktionen
in 6.3 erreichbar):

```rust
pub trait SqlAdapter {
    fn capabilities(&self) -> &Capabilities;
    fn check(&self, req: &SqlRequest) -> Result<(), SqlError>;      // vor der Ausführung
    fn execute(&mut self, req: &SqlRequest, params: &[Value]) -> Result<Outcome, SqlError>;
    fn open_cursor(&mut self, req: &SqlRequest, params: &[Value]) -> Result<CursorId, SqlError>;
    fn fetch(&mut self, cur: CursorId) -> Result<Option<Row>, SqlError>;
    fn close_cursor(&mut self, cur: CursorId);
    fn begin(&mut self) -> Result<(), SqlError>;                     // LUW beginnt
    fn commit(&mut self) -> Result<(), SqlError>;
    fn rollback(&mut self) -> Result<(), SqlError>;
}
```

- `check` läuft vor jedem Aufruf gegen die Fähigkeiten (Abschnitt 8). Ein Fehler dort bricht ab, bevor
  etwas ausgeführt wird.
- `Value` ist ein typisierter Wert in ABAP-Darstellung (UTF-16 für Zeichen, gepackte und Dezimalzahlen wie
  in `decimal.rs`/`packed.rs`). Der Adapter wandelt in den Zieltyp um, nicht in ABAP-Semantik.
- `Row` enthält Werte in der Reihenfolge von `SelectOp.columns`. Der Kern schreibt sie in den Zielbereich.

### 6.2 Cursor und `SELECT … ENDSELECT`

- `SELECT … ENDSELECT` ist ein Cursor: `open_cursor`, dann je Zeile `fetch` mit Ausführung des Schleifenkörpers,
  dann `close_cursor`. Auch bei `EXIT`, `CHECK`, `RETURN` und Ausnahmen wird `close_cursor` aufgerufen.
- Ein Adapter darf während eines offenen Cursors keine anderen Anweisungen auf derselben Verbindung ausführen,
  wenn er das nicht kann. Die Fähigkeit `concurrent_cursors` ist dafür vorgesehen.
- `SY-DBCNT` wird während der Schleife bei jeder Zeile um 1 erhöht. Nach dem Ende steht die Anzahl gelesener Zeilen.

### 6.3 C-ABI (Symbolnamen, Präfix wie in `CLAUDE.md`)

Die Namen sind Vorschläge, die mit dem Codegen abgestimmt werden müssen.

| Funktion | Zweck |
|---|---|
| `abap_rt_sql_execute(ir_id, ir_ptr, ir_len, params, n, target)` | Single-Select, Insert, Update, Delete, Modify |
| `abap_rt_sql_cursor_open(...)` | Beginn von `SELECT … ENDSELECT` |
| `abap_rt_sql_cursor_fetch(cur, target) -> i32` | 1 = Zeile geschrieben, 0 = Ende |
| `abap_rt_sql_cursor_close(cur)` | Schließen (auch bei Abbruch) |
| `abap_rt_sql_commit()` / `abap_rt_sql_rollback()` | `COMMIT WORK` / `ROLLBACK WORK` |

Rückgabe und Fehler: Die Funktionen setzen `SY-SUBRC` und `SY-DBCNT` über den Kern. Ein technischer Fehler
wird als Laufzeit-Exception an den Aufrufer gemeldet (Abschnitt 10). Die Funktionen unwinden nicht über die
C-Grenze.

### 6.4 Externer Adapter (Prozess)

Ein Adapter kann als eigenes Programm laufen. So lassen sich Datenbanken und Dienste anbinden, ohne die
Laufzeit neu zu bauen. Das Programm kann in jeder Sprache geschrieben sein.

- **Start:** Der Kern startet das Programm beim ersten Zugriff auf die Verbindung, einmal je Verbindung.
  Befehl und Argumente stehen in der Konfiguration (Abschnitt 9). Keine Zugangsdaten in Argumenten.
  Sie kommen über Umgebungsvariablen.
- **Protokoll:** Eine Anfrage pro Zeile als JSON auf stdin, eine Antwort pro Zeile als JSON auf stdout.
  stderr wird vom Kern gesammelt und nur im Fehlerfall gemeldet.
- **Anfrage:** `{"id": 1, "method": "execute", "ir": { … }, "params": [ … ]}`. Methoden entsprechen dem
  Adapter-Vertrag aus 6.1: `capabilities`, `check`, `execute`, `open_cursor`, `fetch`, `close_cursor`,
  `begin`, `commit`, `rollback`.
- **Antwort:** `{"id": 1, "ok": true, "rows": [ … ], "dbcnt": 3}` oder
  `{"id": 1, "ok": false, "error": {"kind": "db", "message": "…"}}`. `kind` ist `db`, `capability` oder
  `protocol`.
- **Werte:** Zahlen mit fester Genauigkeit (`DEC`, `PACK`, `NUMC`) gehen als Zeichenkette, damit keine
  Rundung entsteht. Datum und Zeit gehen als `YYYYMMDD` bzw. `HHMMSS`.
- **Fehler des Prozesses:** Beendet sich der Prozess während einer Anfrage, ist das ein Laufzeitfehler mit
  dem gesammelten stderr. Eine Antwort, die nicht dem Protokoll entspricht, ist ein Protokollfehler.
- **Zeitlimit:** Konfigurierbar je Verbindung. Ein Zeitüberschreiten beendet den Prozess und meldet einen
  Fehler.
- **Gleichzeitigkeit:** Eine Anfrage zur Zeit je Prozess. Cursor laufen über `fetch` derselben Verbindung.

## 7. Adapter

### 7.1 SQLite

- Eingebettete Datei (`path`), kein Server. Gut für Tests, Entwicklung und Einzelplatzbetrieb.
- Parameter mit `?` gebunden, nie Text eingesetzt. Bezeichner werden in doppelte Anführungszeichen gesetzt
  und vorher gegen die DDIC-Namen geprüft.
- Umsetzung: `rusqlite` (mit eingebauter SQLite-Quelle, MSVC-tauglich zu prüfen). Lizenz vor Aufnahme prüfen.
- Datumstyp: DDIC `DATS` ist als `TEXT` `YYYYMMDD` abgebildet, `TIMS` als `HHMMSS`. Die Abbildung steht
  in der Konfiguration, nicht im Adapter fest verdrahtet.
- In-Memory-Datenbank für die Akzeptanzfälle.

### 7.2 PostgreSQL

- Netzwerkverbindung über Konfiguration (Host, Port, Datenbank, Benutzer). Passwort nur über Umgebungsvariable
  (Abschnitt 13).
- Parameter als `$1`, `$2` gebunden. Dialektunterschiede (Limit: `LIMIT`/`OFFSET`, Groß-/Kleinschreibung bei
  `LIKE` mit `ILIKE`, Bezeichnerquotierung) liegen ausschließlich im Adapter.
- Umsetzung: synchroner Client (z. B. `postgres`-Crate). Entscheidung zwischen synchron und asynchron offen
  (Abschnitt 16). Lizenz prüfen.
- Verbindungen werden wiederverwendet. Ein Verbindungsabbruch innerhalb einer LUW ist ein Fehler, kein
  stiller Wiederaufbau.

### 7.3 OData (Web-Services)

Zweck: eingeschränkte Abfragen und Änderungen gegen moderne Web-Anwendungen, ohne dass im ABAP auf die
bequeme Open-SQL-Syntax verzichtet werden muss.

**Vorschlag: OData V4 zuerst.** V2 (klassisches SAP Gateway) kann später als zweiter Dialekt folgen. Die Wahl
ist offen (Abschnitt 16).

Zuordnung IR → OData (V4):

| Open SQL | OData V4 |
|---|---|
| `SELECT … FROM tab` | `GET /<EntitySet>` |
| `SELECT SINGLE … WHERE <voller Schlüssel>` | `GET /<EntitySet>(<key>)` |
| Spaltenliste | `$select` |
| `WHERE` mit `=`, `<>`, `<`, `<=`, `>`, `>=`, `AND`, `OR`, `NOT` | `$filter` mit `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `and`, `or`, `not` |
| `IN (…)` | `$filter` mit `in` (V4.01, je nach Service) |
| `LIKE 'A%'` | `startswith(...)` (nur einfache Muster, sonst Fehler) |
| `ORDER BY` | `$orderby` |
| `UP TO n ROWS` | `$top` |
| `OFFSET` | `$skip` |
| `INSERT` | `POST /<EntitySet>` |
| `UPDATE … WHERE <voller Schlüssel>` | `PATCH /<EntitySet>(<key>)` mit `If-Match` (ETag), wenn der Service ETags liefert |
| `DELETE … WHERE <voller Schlüssel>` | `DELETE /<EntitySet>(<key>)` |
| `MODIFY` mit vollem Schlüssel | `PATCH` mit Upsert, nur wenn der Service ihn meldet, sonst Fehler |

Regeln:

- Die Entitätsmenge je Tabelle (und die Schlüsselfelder) kommen aus der Konfiguration. Ein `$metadata`-Abruf
  ist optional und später möglich.
- Werte in URLs werden nach OData-Regeln kodiert (Zeichenketten in einfachen Hochkommas, verdoppelt). Es
  wird nie Text aus Hostvariablen in den Filter eingesetzt, ohne diese Kodierung.
- **Keine Mengenänderung:** `UPDATE` oder `DELETE` ohne vollständigen Schlüssel im `WHERE` ist ein Fehler.
  Ausnahme nur, wenn die Konfiguration eine Operation ausdrücklich erlaubt (Capability `set_update`).
- **Keine Joins, keine Aggregate, kein `GROUP BY`** in S1 bis S2. In S3 nur, wenn der Service sie anbietet
  (`$expand` bzw. `$apply`). Sonst Fehler.
- **Transaktionen:** `COMMIT WORK` ist erfolgreich, weil jede Änderung bereits wirksam ist. `ROLLBACK WORK`
  ist ein Fehler (Abschnitt 12).
- **Komplizierte Operationen** (Joins, Aggregate, Mengenänderungen, Upsert ohne Service-Unterstützung) führen
  zum Fehler. Es gibt keine Ersatzlösung über mehrere Anfragen (Entscheidung).
- Paging: Ergebnisse werden seitenweise geholt (Seitengröße konfigurierbar). `SELECT … ENDSELECT` holt
  Seiten nach, ohne den ganzen Bestand zu laden.
- Authentifizierung über Konfiguration (Abschnitt 13). Zeitlimit und Wiederholung bei Fehlern 429/503 sind
  konfigurierbar, Wiederholung nur bei Lesezugriffen (idempotent).

## 8. Fähigkeiten (Capabilities)

Jeder Adapter meldet seine Fähigkeiten. Der Kern prüft jede IR dagegen, bevor ausgeführt wird.

| Fähigkeit | SQLite | PostgreSQL | OData V4 |
|---|---|---|---|
| `select_simple` (S1) | ja | ja | ja |
| `where_compare`, `between`, `in_list`, `and_or_not` | ja | ja | ja |
| `like` | ja | ja | eingeschränkt |
| `order_by` | ja | ja | ja |
| `limit`, `single` | ja | ja | ja |
| `insert`, `delete`, `update_by_key` | ja | ja | ja |
| `update_by_set`, `delete_by_set` (Mengenänderung) | ja | ja | nein |
| `modify` | ja | ja | eingeschränkt |
| `join_inner`, `join_left` | ja | ja | nein (S3) |
| `aggregate`, `group_by`, `having` | ja | ja | nein (S3) |
| `subquery` (S4) | ja | ja | nein |
| `commit` | ja | ja | ja (ohne Rückgängig-Wirkung) |
| `rollback` | ja | ja | nein |
| `concurrent_cursors` | nein | ja | nein |

Die Matrix ist eine Vorgabe für die Umsetzung. Die genaue Ausprägung je Service steht in dessen Dokumentation
und wird vom Adapter gemeldet, nicht vom Compiler angenommen.

Verhalten bei fehlender Fähigkeit:

- Ist die Verbindung zur Übersetzungszeit bekannt (Konfiguration mit `--db-config`), wird die Anweisung als
  **Übersetzungsfehler** gemeldet. Das entspricht einem Syntaxfehler in SAP.
- Ist sie nicht bekannt, wird sie beim Start des Programms in `check` gegen den Adapter geprüft. Ein
  Verstoß ist ein **Laufzeitfehler vor der ersten Ausführung**, nicht erst mitten im Programm.

## 9. Konfiguration

Die Verbindung wird beim **Start des Programms** gewählt, nicht im Quelltext. So bleibt das ABAP-Programm
unabhängig vom Ziel. Eine Konfigurationsdatei beschreibt die Verbindungen.

Beispiel (ohne Geheimnisse), Datei `flug.toml` neben `flug.exe`:

```toml
# flug.toml - Verbindungen für das Programm flug.exe
[connections.DEFAULT]
adapter = "sqlite"
path = "data/flug.sqlite"

[connections.ARCHIV]
adapter = "postgres"
host = "db.example.internal"
port = 5432
database = "archiv"
user = "abapc"
password_env = "ABAPC_ARCHIV_PASSWORD"     # nur der Name der Variable

[connections.WEB]
adapter = "odata"
service_url = "https://erp.example.internal/odata/v4/flug/"
auth = "bearer"
token_env = "ABAPC_WEB_TOKEN"
page_size = 500

[connections.EXTERN]
adapter = "extern"
command = "adapter/meinadapter.exe"        # relativ zur Konfigurationsdatei
args = ["--modus", "lesen"]
timeout_ms = 30000

[tables.ZFLUG]                             # DDIC-Name -> Ziel
connection = "DEFAULT"
target = "flug"                            # Tabelle im Ziel
columns = { CARRID = "carrier_id", CONNID = "connection_id" }
key = ["CARRID", "CONNID"]                 # für OData und MODIFY
```

- Auswahl: Standardverbindung `DEFAULT`. Mit `CONNECTION <name>` im Quelltext (S4) gezielt eine andere.
- **Dateiname (Entscheidung):** Konvention `<NAME>.exe` -> `<NAME>.toml`. Windows-Namen werden ohne
  Unterschied der Groß- und Kleinschreibung verglichen.
- **Suchreihenfolge (Entscheidung):**
  1. Umgebungsvariable `ABAPC_DB_CONFIG` oder Option des Programms, falls gesetzt (überschreibt alles)
  2. `<NAME>.toml` im Verzeichnis der EXE
  3. `<NAME>.toml` im aktuellen Arbeitsverzeichnis
  Wird keine Datei gefunden, ist der Start ein Fehler, sobald ein Programm auf eine Verbindung zugreift.
- Relative Pfade in der Konfiguration (SQLite-Datei, Adapterprogramm) beziehen sich auf das Verzeichnis
  der Konfigurationsdatei.
- Der Compiler liest die Konfiguration nur, wenn mit `--db-config <datei>` angegeben. Ohne diese Angabe prüft
  er keine Fähigkeiten, und die Prüfung findet beim Start statt (Abschnitt 8).
- Die Konfigurationsdatei enthält keine Passwörter oder Token, nur die Namen von Umgebungsvariablen.

## 10. Fehler und Systemfelder

ABAP meldet Ergebnisse über `SY-SUBRC` und `SY-DBCNT`. Technische Fehler sind Ausnahmen. Die folgende
Tabelle ist die Vorgabe, wobei die Einzelheiten gegen den ABAP-Standard zu prüfen sind (Abschnitt 17).

| Fall | `SY-SUBRC` | `SY-DBCNT` | Ergebnis |
|---|---|---|---|
| Treffer (Single, Liste, Loop) | 0 | Anzahl Zeilen | normal |
| Kein Treffer | 4 | 0 | normal, kein Fehler |
| `INSERT` (Einzelsatz) mit doppeltem Schlüssel | 4 | 0 | normal |
| `INSERT FROM TABLE` mit doppeltem Schlüssel, ohne `ACCEPTING DUPLICATE KEYS` | unverändert | unverändert | Ausnahme `CX_SY_OPEN_SQL_DB`-Entsprechung. Ohne Behandlung Laufzeitfehler mit Rollback der LUW |
| `INSERT FROM TABLE` mit `ACCEPTING DUPLICATE KEYS` | 4 bei Duplikaten | Anzahl tatsächlich eingefügter Zeilen | normal |
| `UPDATE`/`DELETE` ohne betroffene Zeile | 4 | 0 | normal |
| Nicht unterstützte Fähigkeit (zur Übersetzungszeit) | - | - | Übersetzungsfehler |
| Nicht unterstützte Fähigkeit (zur Laufzeit, in `check`) | - | - | Laufzeitfehler vor der Ausführung |
| Verbindung nicht erreichbar, Authentifizierung, Timeout | unverändert | unverändert | Ausnahme (`CX_SY_OPEN_SQL_DB`-Entsprechung) |
| Datenbankfehler (Constraint, Syntax im Ziel, HTTP 4xx/5xx) | unverändert | unverändert | Ausnahme |
| Typkonflikt Ziel <-> Spalte, den der Kern nicht umsetzen kann | unverändert | unverändert | Ausnahme |

Regeln:

- Eine Ausnahme ist mit `TRY … CATCH` abfangbar. Ist sie nicht gefangen, endet das Programm mit einem
  Laufzeitfehler. Die Ausnahmeklasse wird im Sprachdokument ergänzt, sobald die Ausnahmebehandlung
  feststeht.
- Vor einer Ausnahme werden `SY-SUBRC` und `SY-DBCNT` nicht verändert.
- Die Ausnahmeklasse dient für spezielle Fälle. Kein Treffer bleibt bei `SY-SUBRC = 4` ohne Ausnahme
  (Standard, Entscheidung).
- Laufzeitfehler sind Fehler, kein `panic!` (`CLAUDE.md`). Meldungen sind englisch, feste Texte im Code.
- Der Fehlertext nennt die Verbindung, die Tabelle und die Zeile, nicht die Zugangsdaten.

Beispiel der Meldungen (englisch, Vorschlag):

```
db error at line 42 (connection ARCHIV, table ZFLUG): connection refused
sql capability missing at line 17 (connection WEB): JOIN not supported by odata adapter
```

## 11. Typen und Konvertierung

- Die ABAP-Typen aller Spalten kommen aus der DDIC (`ddic.rs`). Die Datenbank liefert nur Rohwerte.
- Umwandlung im Adapter: Der Adapter liefert den Rohwert in der Form, die er zurückbekommt (Text, Zahl,
  Zeitstempel). Die Umwandlung in den ABAP-Typ macht der Kern mit den Regeln aus `decimal.rs` und
  `packed.rs`.
- Passt ein Rohwert nicht zum ABAP-Typ (z. B. `DATS` mit `2026-13-01`), ist das ein Laufzeitfehler mit
  Zeile und Feld, nicht ein stiller Wert.
- Zeichenketten: UTF-8 im Ziel, UTF-16 in der Laufzeit. Die Umwandlung liegt im Kern.
- Dezimaltrennzeichen ist fest `.` (`CLAUDE.md`).
- Zahlen in OData: JSON-Zahlen, die die Genauigkeit des ABAP-Typs überschreiten, sind als Zeichenkette zu
  liefern. Der Kern prüft die Länge.

## 12. Transaktionen (LUW)

- **SQLite, PostgreSQL:** Eine LUW beginnt beim ersten schreibenden Zugriff im Programm. `COMMIT WORK`
  bestätigt, `ROLLBACK WORK` verwirft. Nicht bestätigte Änderungen werden beim Programmende zurückgerollt
  (**Entscheidung**, konservativ).
- **OData:** Jede Änderung ist sofort wirksam. `COMMIT WORK` ist erfolgreich, `ROLLBACK WORK` schlägt fehl:
  eine Ausnahme zur Laufzeit, bzw. ein Übersetzungsfehler, wenn die Verbindung zur Übersetzungszeit bekannt
  ist (Entscheidung). Eine spätere Erweiterung mit `$batch` ist möglich, wenn der Service es anbietet.
- Schreibt das Programm über mehrere Verbindungen, ist jede Verbindung eine eigene LUW. Ein verteiltes
  Commit ist nicht Teil des Umfangs.
- Ein Adapter muss bei Abbruch des Programms seine offenen Änderungen zurückrollen oder den Fehler melden.

## 13. Sicherheit

- **Keine Textkonkatenation mit Werten.** Alle Hostwerte gehen als gebundene Parameter oder kodierte
  Werte in die Anfrage. SQL-Injection ist damit nicht möglich, solange Bezeichner aus der DDIC kommen und
  geprüft werden.
- Bezeichner (Tabellen-, Spaltennamen) werden nie aus Laufzeitwerten gebaut. Sie stammen aus der
  Übersetzung und der Konfiguration.
- **Geheimnisse nicht in Dateien.** Passwörter, Token und Zertifikate kommen aus Umgebungsvariablen oder
  einem Schlüsselspeicher des Betriebssystems. Die Konfiguration nennt nur den Namen. Keine Zugangsdaten
  in Quelltext, Akzeptanzfällen, Doku oder Protokoll.
- Protokollierung (`--trace-sql`, optional) zeigt Dialekttext und Parameternamen, aber keine Werte von
  Feldern, die als vertraulich markiert sind. Eine Markierung vertraulicher Felder ist vorerst nicht
  vorgesehen (Abschnitt 16).
- Die Berechtigungsprüfung der Zielsysteme (Datenbankrechte, OData-Scopes) liegt beim Betreiber. Der Compiler
  prüft keine Rechte.
- TLS ist für OData Pflicht. `http://` ist nur mit ausdrücklicher Option für lokale Tests zulässig.

## 14. Tests

- **Compiler:** Parser- und Lowering-Tests für jede Stufe. Dump-Tests (`--dump-sql`) vergleichen die IR mit
  Erwartungswerten. Übersetzungsfehler mit `expected_error.txt`.
- **Akzeptanzfälle** (`tests/acceptance/<fall>/`, Präfix nach Schema, Milestone-Nummer festzulegen):
  Programm mit `expected.txt`. Datenbasis wird vorab aus SQL-Skript im Fall erzeugt, nicht aus `libraries/`
  oder `sample-archives/` (`CLAUDE.md`).
- **SQLite:** In-Memory-Datenbank, läuft ohne Zusatz in `cargo test`.
- **PostgreSQL:** Test nur, wenn `ABAPC_TEST_PG` gesetzt ist. Keine Zugangsdaten im Repo. Ein Container für
  lokale Läufe ist optional.
- **OData:** Lokaler Mock-HTTP-Server im Test, der Anfragen prüft (URL, Filter, Kodierung, ETag). Kein Netz
  im Test.
- **Fehlerpfade:** Jeder Fehler aus Abschnitt 10 braucht einen Test, auch die Fälle `SY-SUBRC`-unverändert.

## 15. Phasen

| Phase | Inhalt | Ergebnis |
|---|---|---|
| 0 | Entscheidungen aus Abschnitt 16 getroffen. Spezifikation Entwurf 2 freigegeben. | erledigt |
| 1 | Parser und Lowering für S1. `Stmt::Sql`. `--dump-sql`. Laufzeitkern mit `SqlAdapter`-Trait. SQLite-Adapter. Externer Adapter (Prozess, JSON). `SY-SUBRC`, `SY-DBCNT`. `SELECT … ENDSELECT` über Cursor. | Lesen aus SQLite, erste Akzeptanzfälle |
| 2 | S2: `INSERT`, `UPDATE`, `DELETE`, `MODIFY`. `COMMIT WORK`, `ROLLBACK WORK`. Fähigkeitsprüfung zur Übersetzungszeit mit `--db-config`. | Schreiben, LUW |
| 3 | PostgreSQL-Adapter. S3 (Joins, Aggregate) für SQLite und PostgreSQL. | zweiter Datenbankadapter |
| 4 | OData-Adapter V4: Lesen, Schlüsselzugriffe, Schreiben nach Schlüssel. Mock-Server-Tests. | Web-Anbindung |
| 5 | S4 (Unterabfragen, `CONNECTION`). OData-Erweiterungen (`$batch`, `$apply`), falls der Service es bietet. V2 als Option. | optional |

Vor jeder Phase: `cargo fmt --check`, `cargo clippy --workspace --all-targets`,
`cargo test -p abap-compiler --test acceptance`, `cargo test -p abap-rt`, `cargo test -p abap-compiler`.

## 16. Entscheidungen

Stand 2026-11-28. Die Entscheidungen des Auftraggebers sind hier festgehalten.

| # | Thema | Entscheidung |
|---|---|---|
| 1 | IR-Format | JSON-Text. Gleiches Format für externe Adapterprozesse (6.4). |
| 2 | Adapterwahl | Zur Laufzeit über die Konfigurationsdatei (Abschnitt 9). |
| 3 | Fehlerbehandlung | Ausnahmeklasse für spezielle Fälle. `SY-SUBRC = 4` bleibt der Standard für "kein Treffer". |
| 4 | Mandant | Kein Mandantenfilter. `MANDT` wird ignoriert. |
| 5 | OData-Version | V4. V2 ist kein Ziel der ersten Stufen. |
| 6 | PostgreSQL-Client | Synchron. |
| 7 | Transaktionen OData | `COMMIT WORK` erfolgreich, `ROLLBACK WORK` schlägt fehl (Abschnitt 12). |
| 8 | Komplizierte OData-Operationen | Führen zum Fehler, keine Ersatzlösung. |
| 9 | Vertrauliche Felder | Vorerst nicht nötig. Die Protokollierung zeigt keine Feldwerte, nur Dialekttext und Parameternamen. |
| 10 | Konfigurationsdatei | `<NAME>.toml` neben der EXE, sonst im Arbeitsverzeichnis (Abschnitt 9). Umgebungsvariable `ABAPC_DB_CONFIG` überschreibt. |
| 11 | Lizenzen | Kein Hindernis. Das Projekt ist Open Source. Die Lizenzen der Abhängigkeiten müssen dazu passen. |

Offen ist nichts aus Entwurf 1. Neue Punkte kommen hier hinzu.

## 17. Zu prüfen (ABAP-Verhalten)

Diese Punkte sind aus dem ABAP-Standard zu belegen, bevor die Akzeptanzfälle erwartet werden. Der
Subagent `sap-specialist` kann die Prüfung mit der SAP-Dokumentation und dem Quelltext im System
unterstützen.

- `SY-SUBRC` bei `SELECT SINGLE` ohne Treffer, bei `SELECT … ENDSELECT` ohne Zeilen, bei `UPDATE`/`DELETE`
  ohne betroffene Zeile, bei `INSERT` mit doppeltem Schlüssel.
- `SY-DBCNT` bei `SELECT … INTO TABLE` mit und ohne `UP TO n ROWS`, bei `UPDATE`/`DELETE`, bei `MODIFY`.
- Verhalten von `SY-DBCNT` während einer `SELECT … ENDSELECT`-Schleife, wenn sie per `EXIT` verlassen wird.
- Regeln für `INTO CORRESPONDING FIELDS OF` bei gleichnamigen und fehlenden Spalten.
- `CHAR`-Auffüllung und `NUMC`-Verhalten beim Lesen aus der Datenbank.
- Initialwert bei `NULL` in der Datenbank (Abschnitt 4.4).
- Bedeutung von `CONNECTION` im Open-SQL-Umfang und wie Fehler dort gemeldet werden.
- Name und Vererbung der Ausnahmeklassen für Datenbankfehler, insbesondere `CX_SY_OPEN_SQL_DB`.
- Wann ein `UPDATE` ohne `WHERE` in Open SQL zulässig ist (Mengenänderung) und ob `UPDATE … SET` den Schlüssel
  ändern darf.
- Ob `MODIFY` ohne vollen Schlüssel einen Fehler oder eine Warnung erzeugt.
- Ob `COMMIT WORK` ohne vorherige Änderung Auswirkungen hat (Sperren, Sperrfreigabe).

**Ergebnis der Prüfung (2026-11-28, Quelle: SAP-Doku für 7.50 bis 7.58; kein Systemabgleich über ADT):**

| Punkt | Befund |
|---|---|
| `SY-SUBRC`/`SY-DBCNT` bei SELECT ohne Treffer, UPDATE/DELETE ohne Zeile | belegt: 4, Anzahl 0 |
| `INSERT` Einzelsatz mit doppeltem Schlüssel | belegt: 4, 0, keine Ausnahme |
| `INSERT FROM TABLE` ohne `ACCEPTING DUPLICATE KEYS` mit Duplikat | belegt: Ausnahme `CX_SY_OPEN_SQL_DB`, Werte unverändert (Korrektur gegenüber Abschnitt 10 oben) |
| `INSERT FROM TABLE` mit `ACCEPTING DUPLICATE KEYS` | belegt: 4 bei Duplikaten, `SY-DBCNT` = eingefügte Zeilen |
| `SY-DBCNT` in der Schleife | belegt: Anzahl bisher gelesener Zeilen |
| `SY-DBCNT` nach normalem Ende der ENDSELECT-Schleife | **nicht eindeutig belegt.** Die Doku sagt für FETCH "0, wenn keine Zeile gelesen werden kann". Der Akzeptanzfall prüft 3 und bleibt unbelegt, bis die Prüfung entfernt ist |
| `SY-SUBRC` nach EXIT | belegt sinngemäß: 0 |
| Ziel bei SELECT SINGLE ohne Treffer | belegt: unverändert |
| `INTO TABLE` ohne Treffer | belegt: Zieltabelle wird geleert. Bei `APPENDING` bleibt sie unverändert |
| NULL in der Datenbank | belegt: typgerechter Initialwert. Im Akzeptanzfall nicht getestet (keine nullbare Spalte) |
| `NUMC`/`CHAR` beim Lesen | belegt für die Zuweisungsregel. Die Projektfälle lesen mit `SELECT *` ohne `CORRESPONDING`; die Doku sagt dort "unkonvertiert, nach Struktur des Ergebnisses". Der Fall muss auf `INTO CORRESPONDING FIELDS OF` umgestellt werden |
| `UP TO` zusammen mit `SINGLE` | belegt verboten (Doku 7.58) |
| `UP TO 0 ROWS` mit Literal 0 | belegt nicht zulässig im Strict Mode ab 7.51. Ob der Standard-Compiler das Literal außerhalb des Strict Mode ablehnt, ist **nicht eindeutig belegt**. Host-Variable 0 bedeutet alle Zeilen |

Offen: Ein Abgleich am Quelltext des SAP-Systems über ADT fand nicht statt, weil keine Destination konfiguriert ist.

## 18. Stand der Umsetzung: Abweichungen und Annahmen

### 18.1 Abweichungen von der Spezifikation

| Thema | Umsetzung | Grund |
|---|---|---|
| Ausnahmeklasse `CX_SY_OPEN_SQL_DB` | nicht umgesetzt. Datenbankfehler sind Laufzeitfehler. Die Dublette bei `INSERT FROM TABLE` ohne `ACCEPTING` ist ein Laufzeitfehler mit Text `duplicate key` | Der Ausnahmemechanismus trägt das ohne Umbau nicht |
| Trace | Umgebungsvariable `ABAPC_TRACE_SQL` statt Option `--trace-sql` | Programme nehmen keine Argumente |
| Konfigurationsmodul | `abap-compiler` hängt von `abap-rt` ab (Konfiguration und Fähigkeiten). `abap-sql-ir` bleibt frei von TOML-Abhängigkeiten | Keine zweite Konfigurationsdefinition |
| Fähigkeitsprüfung zur Übersetzungszeit | SQLite vollständig. `extern` erst zur Laufzeit. `postgres` und `odata` sind Übersetzungsfehler ("not implemented in this version") | Adapter sind in Phase 1 und 2 nur SQLite und extern |
| Crate `abap-sql-ir` | eigenes Crate, nur Serde-Typen | Gemeinsame IR für Compiler und Laufzeit ohne Duplikat |
| `LIKE` | case-sensitiv (GLOB-Übersetzung, Metazeichen als Daten). Vorher in Phase 1 case-insensitiv | ABAP-Semantik. Beabsichtigte Änderung |
| `ESCAPE` in `LIKE` | Übersetzungsfehler | nicht im Umfang |
| `INSERT` mit Spaltenliste, `UPDATE … FROM TABLE` | Übersetzungsfehler | nicht im Umfang |

### 18.2 Annahmen, nicht gegen den ABAP-Standard belegt

- `SY-DBCNT` bei `UPDATE … FROM wa` und `DELETE … FROM wa`: 1 bei Erfolg, sonst 0.
- `UPDATE … SET` mit Dublette: `SY-SUBRC` 4, `SY-DBCNT` 0, ohne Ausnahme.
- `MODIFY … FROM TABLE`: `SY-SUBRC` 4, wenn nicht alle Zeilen verarbeitet wurden.
- Leere interne Tabelle bei `INSERT`, `MODIFY`: `SY-SUBRC` 0, `SY-DBCNT` 0. Bei `DELETE FROM TABLE`: 0 und 0.
- Schreiben von `CHAR` und `NUMC` wie beim Lesen (Auffüllung, Abschneiden).
- `CORRESPONDING FIELDS OF` bei Einzelsatz: Komponenten ohne Spalte bleiben unverändert (Akzeptanzfall `m12_sql_corresponding_fields`, `zu-pruefen.txt`).
- `CORRESPONDING FIELDS OF TABLE` und `APPENDING CORRESPONDING`: Neue Zeilen starten mit dem Initialwert jeder Komponente (CHAR blank, Zahlen 0, PACKED 0 mit Vorzeichen 0x0C). Nicht zugeordnete Komponenten bekommen also diesen Wert. Akzeptanzfall `m12_sql_corresponding_table_initial`, `zu-pruefen.txt`.
- `COMMIT WORK` und `ROLLBACK WORK` lassen `SY-SUBRC` unverändert. Nicht gegen den Standard belegt (Akzeptanzfall `m12_sql_commit_subrc`).
- `SY-DBCNT` bei `SELECT … ENDSELECT`: 0 vor der Schleife. Während der Schleife zählt es die gelesenen Zeilen. Nach dem Ende steht die Gesamtzahl, auch wenn der Körper weitere Datenbankanweisungen ausführt (Akzeptanzfälle `m12_sql_loop_dbcnt_empty`, `m12_sql_loop_dbcnt_nested`).
- Struktur-Zuordnung bei `INSERT`/`UPDATE FROM`/`DELETE FROM`/`MODIFY` mit Work Area: nach Position, gleiche Anzahl und Art der Komponenten.
- Rollback: Offene LUWs werden bei `sql::fail`, `LEAVE PROGRAM` und Programmende zurückgerollt. Andere Laufzeitfehler (z. B. Division durch 0) rollen nicht zurück. SQLite stellt das beim nächsten Öffnen über das Journal wieder her. Nicht getestet.

### 18.3 Offene Punkte

- `CORRESPONDING FIELDS OF` ohne eine einzige passende Spalte: Der SQLite-Adapter lehnt die leere Spaltenliste zur Laufzeit ab. ABAP-Verhalten ungeklärt, Entscheidung nötig. Vorschlag: Übersetzungsfehler, wenn Tabelle und Struktur statisch bekannt sind.
- Schlüssel für `UPDATE … FROM wa`, `DELETE … FROM wa` und `MODIFY` kommt aus der Konfiguration (`key`). Der externe Adapter bekommt ihn nicht über das Protokoll.
- Die Akzeptanzfälle für fehlende Tabellen sind getrennt: `m11_error_sql_table_not_configured` (Übersetzungsfehler, Tabelle nicht in der Konfiguration) und `m11_runtime_sql_table_missing_in_db` (Laufzeitfehler, Tabelle ist konfiguriert, fehlt aber in der Datenbank).
- Pro Schreibzugriff: Der Fehlerpfad bei anderen Laufzeitfehlern rollt die LUW nicht zurück (siehe 18.2).
- **Testlauf:** `cargo test -p abap-compiler` baut die Laufzeit-Bibliothek `abap_rt.lib` nicht neu. Vor dem Akzeptanzlauf muss `cargo build -p abap-rt` laufen. Sonst prüfen die Fälle gegen alten Code. Bei einem Fehlschlag zuerst diesen Schritt wiederholen.
- Offen aus dem Review, vorbestehend und nicht SQL-spezifisch: Eine ungefangene Ausnahme (`RAISE EXCEPTION` ohne `TRY`) endet mit Exit 0 ohne Meldung. Die Spezifikation (Abschnitt 10) verlangt einen Laufzeitfehler.
- Offen: DATS- und TIMS-Prüfung beim Lesen (Abschnitt 11) und Surrogatpaare, die am Feldende zerschnitten werden.
