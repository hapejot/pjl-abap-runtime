# Dynpro-Simulation: Spezifikation (Entwurf 2)

Stand: 2026-11-27. Status: **Vorschlag**, noch nicht umgesetzt. Was heute gilt, steht in
[sprache.md](sprache.md) (Abschnitt 7, Punkt 4: Dynpros sind nicht implementiert).

Kennzeichnung wie in [README.md](README.md): **Entscheidung** ist eine Projektentscheidung, die nicht aus
dem ABAP-Standard stammt. **zu prüfen** ist Verhalten, das gegen den ABAP-Standard belegt werden muss (Abschnitt 12).

Die Doku bleibt vorerst deutsch. Alle Meldungen von Compiler und Laufzeit sind englisch (`CLAUDE.md`).

## 0. Entscheidungen aus Entwurf 1

| Thema | Entscheidung |
|---|---|
| Layoutformat | Eigenes Format `.dynp`, zeilenorientiert, mit dem Editor zu schreiben. Kein XML. |
| abapGit-DYNP-Objekte | Vorerst ignoriert. Später, eigenes Thema. |
| Crate | Kein eigener Crate. `crossterm` ist Teil von `abap-rt`. |
| Übersetzung | `.dynp` wird vom Compiler in eine Folge von `abap_rt_screen_*`-Aufrufen übersetzt, wie ABAP-Anweisungen in Laufzeitaufrufe. Die Laufzeit parst keine Quelle. |
| Sprache | Alle Meldungen englisch. Keine Sprachabhängigkeiten. |
| Esc | löst `CANCEL` aus (wenn im GUI-Status definiert). |
| Dezimaltrenner | fest `.` |

## 1. Ziel und Abgrenzung

Ein ABAP-Programm soll Dynpros aufrufen können (`CALL SCREEN`, `SET SCREEN`, `LEAVE SCREEN`, PBO/PAI-Module),
ohne SAP-System. Die Bildschirme laufen im Terminal mit `crossterm`.

- **ABAP-kompatibel:** Anweisungen und Flow-Logic-Syntax sind die des SAP-Standards. Der ABAP-Teil eines
  Programms soll auf SAP ohne Änderung laufen.
- **Nicht am Standard:** Das Layout (`.dynp`) ist eine eigene Sprache.
- **Nicht im Umfang:** pixelgenaue SAP-GUI-Darstellung, GUI-Controls (Tree, Container, ALV), Screen Painter,
  Berechtigungen, RFC, Druck, DYNP-Objekte aus abapGit.

## 2. Begriffe

| Begriff | Bedeutung |
|---|---|
| Dynpro | Bildschirm mit Nummer (z. B. `0100`), Layout, Feldern, GUI-Status, Ablauflogik |
| Feld | Anzeige- oder Eingabeposition, gebunden an eine globale ABAP-Variable |
| PBO | Process Before Output: Werte vom Programm auf den Bildschirm |
| PAI | Process After Input: Werte vom Bildschirm ins Programm |
| Funktionscode | Wert in `sy-ucomm`, ausgelöst durch Enter, F-Taste oder Drucktaste |
| GUI-Status | Funktionstasten einer Anzeige, im `.dynp` definiert |
| Screen-Aufruf | Eine `abap_rt_screen_*`-Funktion aus Abschnitt 8.2 |

## 3. Eingaben

- Zu `prog.abap` gehört optional `prog.dynp` im selben Verzeichnis. Mit `--dynp <datei>` wird sie explizit gewählt.
- Eine Datei mit Dynpros, aber ohne `CALL SCREEN` im Programm, ist eine Warnung.
- `CALL SCREEN n` ohne `DYNPRO n` in der `.dynp` ist ein Übersetzungsfehler.
- Bei einem ZIP im abapGit-Format werden Dynpros nicht unterstützt (Abschnitt 0). Ein `CALL SCREEN` dort ist ein Übersetzungsfehler.

## 4. Dynpro-Datei (`.dynp`)

### 4.1 Grundsätze

- Zeilenorientiert, eine Anweisung pro Zeile. Schlüsselwörter groß oder klein.
- Kommentar: Zeile, die mit `*` beginnt, oder `"` außerhalb einer Zeichenkette bis Zeilenende. Wie in ABAP.
- Zeichenketten in einfachen Hochkommas. Ein Hochkomma darin wird verdoppelt (`'It''s'`).
- UTF-8. Einrückung frei.

### 4.2 Grammatik (vereinfacht)

```
datei        = { dynpro | status } .

dynpro       = "DYNPRO" nummer [ "KIND" ( "NORMAL" | "MODAL" ) ] NL
               { groesse | titel | element }
               [ flow ]
               "ENDDYNPRO" NL .

groesse      = "SIZE" zeilen spalten NL .            "Standard 20 80, siehe 4.4
titel        = "TITLE" zeichenkette NL .

element      = text | feld | taste | rahmen .
text         = "TEXT" zeile spalte zeichenkette { "COLOR" farbe | "INTENSIFIED" } NL .
feld         = "FIELD" name zeile spalte { feldattribut } NL .
taste        = "BUTTON" zeile spalte zeichenkette "CODE" funktionscode NL .
rahmen       = "FRAME" zeile spalte zeile spalte [ zeichenkette ] NL .

feldattribut = "INPUT" | "OUTPUT" | "REQUIRED" | "INVISIBLE" | "PASSWORD"
             | "COLOR" farbe | "INTENSIFIED" .
farbe        = "1" .. "7" .

flow         = "FLOW" NL
               { "PROCESS" ( "BEFORE OUTPUT" | "AFTER INPUT" ) NL
                 { "MODULE" name [ "AT" "EXIT-COMMAND" ] NL } }
               "ENDFLOW" NL .

status       = "GUISTATUS" name NL
               { fkey | exclude } "ENDGUISTATUS" NL .
fkey         = "FKEY" taste_name zeichenkette "CODE" funktionscode [ "TYPE" ( "B" | "C" | "E" ) ] NL .
exclude      = "EXCLUDE" funktionscode NL .
```

Phase 1 nutzt die Grammatik ohne `EXCLUDE` und `PASSWORD` (Phase 2). `AT EXIT-COMMAND` ist in Phase 1 enthalten. Seine genaue Semantik ist offen (Abschnitt 12, Punkt 8).

### 4.3 Beispiel

**`zkunde_demo.dynp`**

```
* Dynpro 0100: Kunde anlegen
DYNPRO 0100
  SIZE 20 80
  TITLE 'Kunde anlegen'

  TEXT 2 2 'Kundennummer'
  FIELD ZKUNNR 2 20 INPUT REQUIRED
  TEXT 4 2 'Name'
  FIELD ZNAME1 4 20 INPUT
  TEXT 6 2 'Ort'
  FIELD ZORT 6 20 INPUT
  BUTTON 9 2 'Sichern' CODE SAVE

  FLOW
    PROCESS BEFORE OUTPUT
      MODULE STATUS_0100
    PROCESS AFTER INPUT
      MODULE USER_COMMAND_0100
      MODULE EXIT_0100 AT EXIT-COMMAND
  ENDFLOW
ENDDYNPRO

GUISTATUS STATUS_0100
  FKEY F3  'Back'    CODE BACK   TYPE B
  FKEY F12 'Cancel'  CODE CANCEL TYPE C
  FKEY F15 'Exit'    CODE EXIT   TYPE E
ENDGUISTATUS
```

**`zkunde_demo.abap`**

```abap
REPORT zkunde_demo.

DATA: zkunnr  TYPE c LENGTH 10,
      zname1  TYPE c LENGTH 30,
      zort    TYPE c LENGTH 30,
      ok_code TYPE sy-ucomm.

START-OF-SELECTION.
  CALL SCREEN 0100.

MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'SAVE'.
      MESSAGE 'Customer saved' TYPE 'S'.
    WHEN 'BACK' OR 'CANCEL'.
      LEAVE TO SCREEN 0.
  ENDCASE.
ENDMODULE.

MODULE exit_0100 INPUT.
  LEAVE PROGRAM.
ENDMODULE.
```

Hinweis: Die Literale im Beispiel sind englisch, weil die Laufzeit englisch ist. Der Text im Layout bleibt so,
wie der Autor ihn schreibt.

### 4.4 Koordinaten und Größe

- Zeilen und Spalten sind 1-basiert und beziehen sich auf den Dynpro-Bereich.
- Standard `SIZE 20 80`. Höchstens 20 × 80.
- Das Terminal muss mindestens 80 × 24 groß sein. Kleiner: Laufzeitfehler beim ersten `CALL SCREEN`.
- Ein Element außerhalb der Größe ist ein Übersetzungsfehler.
- **Entscheidung:** feste Größe, kein Scrollen.

### 4.5 Bildschirmaufteilung (Terminal 80 × 24)

| Terminalzeilen | Inhalt |
|---|---|
| 1 | Titelzeile (`TITLE`) |
| 2 – 21 | Dynpro-Bereich |
| 22 – 23 | Funktionstastenleiste (aus dem GUI-Status) |
| 24 | Meldungszeile (`MESSAGE`) |

**Entscheidung:** feste Aufteilung. Ein `KIND MODAL`-Dynpro wird als Rahmen über den Dynpro-Bereich gelegt
(Phase 2, mit `CALL SCREEN ... STARTING AT`).

### 4.6 Prüfungen beim Übersetzen

Jede Verletzung ist ein Übersetzungsfehler mit englischer Meldung, mit Datei, Zeile und Spalte.

- Feldname muss eine globale Variable des Programms sein, mit erlaubtem Typ (4.7).
- Feldname höchstens einmal je Dynpro.
- Feld, Text und Taste dürfen sich nicht überlappen und müssen in den Dynpro-Bereich passen.
- `MODULE` in der Ablauflogik muss existieren. `PROCESS BEFORE OUTPUT` braucht `MODULE ... OUTPUT`,
  `PROCESS AFTER INPUT` braucht `MODULE ... INPUT`.
- Jedes `CALL SCREEN n` braucht ein `DYNPRO n`. Ein nicht aufgerufenes `DYNPRO` ist eine Warnung.
- `SET PF-STATUS` und `STATUS` verweisen auf einen vorhandenen `GUISTATUS`.
- `CODE` in `FKEY` und `BUTTON` höchstens 70 Zeichen (Länge von `sy-ucomm`).

### 4.7 Dynpro-Feldtypen

Die Länge kommt immer aus dem ABAP-Datentyp, nicht aus der `.dynp`.

| ABAP-Typ | Anzeige | Eingabe |
|---|---|---|
| `c` (`LENGTH n`) | links, Leerzeichen am Ende | beliebige Zeichen |
| `n` (`NUMC`, DDIC) | rechts, mit Ziffern | nur Ziffern |
| `d` (`DATS`, DDIC) | `JJJJMMTT`, Länge 8 | Ziffern, Prüfung als Datum beim PAI |
| `t` (`TIMS`, DDIC) | `HHMMSS`, Länge 6 | Ziffern, Prüfung als Uhrzeit beim PAI |
| `i` | Feldbreite 11: 10 Ziffern rechtsbündig, danach Stelle 11 für `-` (nur bei negativen Werten, sonst Leerzeichen) | Ziffern mit optionalem `-` vorne oder hinten |
| `p` / `DEC` | rechtsbündig, Nachkommastellen | Ziffern, `.` als Dezimaltrenner |
| `string` | **nicht erlaubt** | `Type string is not allowed for dynpro field 'X'` |

Dezimaltrenner ist fest `.` (Entscheidung, keine Sprachabhängigkeit).

### 4.8 Feldattribute

| Attribut | Wirkung |
|---|---|
| (keines) / `INPUT` | editierbar. **Entscheidung:** Standard ist `INPUT`, siehe Abschnitt 12, Punkt 1. |
| `OUTPUT` | nur Anzeige, PAI überträgt nicht zurück. |
| `REQUIRED` | Pflichtfeld. Leere Eingabe wird vor dem PAI abgewiesen. |
| `INVISIBLE` | nicht angezeigt, wird übertragen. |
| `PASSWORD` | Anzeige als `*`. Phase 2. |
| `COLOR n` | Farbe 1 bis 7, Zuordnung in 8.3. |
| `INTENSIFIED` | hervorgehoben. |

## 5. ABAP-Anweisungen und ihre Laufzeitaufrufe

Die Zuordnung zeigt, wie der Compiler jede Anweisung übersetzt. Die Aufrufe stehen in Abschnitt 8.2.

| ABAP | Laufzeitaufruf | Phase |
|---|---|---|
| `CALL SCREEN n.` | `abap_rt_call_screen(n)` | 1 |
| `CALL SCREEN n STARTING AT x y ENDING AT x2 y2.` | `abap_rt_call_screen_modal(n, x, y, x2, y2)` | 2 |
| `SET SCREEN n.` | `abap_rt_set_screen(n)` | 1 |
| `LEAVE SCREEN.` | `abap_rt_leave_screen()` | 1 |
| `LEAVE TO SCREEN n.` | `abap_rt_leave_to_screen(n)` | 1 |
| `LEAVE PROGRAM.` | `abap_rt_leave_program()` | 1 |
| `SET PF-STATUS 'x'.` | `abap_rt_set_pf_status('x')` | 1 |
| `MESSAGE 'x' TYPE 'T'.` | `abap_rt_message('T', 'x')` | 1 |

Der Messagetyp muss ein Großbuchstabe sein (`S`, `I`, `W`, `E`, `A`, `X`). Kleinbuchstaben sind ein Übersetzungsfehler.
| `SET CURSOR FIELD f.` | `abap_rt_set_cursor(f)` | 2 |
| `GET CURSOR FIELD f.` | `abap_rt_get_cursor(f)` | 2 |
| `sy-ucomm`, `sy-dynnr` | Felder der SY-Struktur, siehe 8.2 (`abap_rt_screen_sy`) | 1 |

`LEAVE TO SCREEN 0` ist die Form, die der Beispielcode nutzt. Ihr Verhalten ist zu prüfen (Abschnitt 12).

## 6. Ablauf zur Laufzeit

### 6.1 Schleife je Dynpro

```
call_screen(n):
  push Stapelrahmen(n)
  loop:
    PBO:  rufe PBO-Module von n in Reihenfolge
          (Module setzen Aktionen: SET SCREEN, LEAVE, MESSAGE, SET PF-STATUS)
          Wenn LEAVE: Schleife verlassen
    Anzeige:
          Felder aus den ABAP-Variablen lesen und formatieren
          Titel, Funktionstasten, Meldung zeichnen
          Cursor: zuletzt gesetzter Cursor, sonst erstes Eingabefeld
    Eingabe:
          Ereignisse lesen, bis ein Funktionscode ausgelöst wird (6.2, 6.3)
    Übertragung Bildschirm -> Programm:
          Prüfen (REQUIRED, Typ). Fehler: Meldung E, zurück zur Anzeige ohne PAI (6.4).
          Sonst: INPUT-Felder in die ABAP-Variablen schreiben.
    PAI:  sy-ucomm = Funktionscode
          rufe PAI-Module von n in Reihenfolge
          Wenn LEAVE: Stapelrahmen entfernen, zurück zum Aufrufer
          Wenn SET SCREEN m: n durch m ersetzen
          sonst: gleiches Dynpro, nächster PBO
  pop Stapelrahmen
```

Aktionen aus Modulen (`SET SCREEN`, `LEAVE`) werden als Zustand gespeichert und nach dem Modul ausgewertet.
Mehrere Aktionen im selben Modul: die letzte gewinnt (zu prüfen, Abschnitt 12).

### 6.2 Funktionscodes

| Auslöser | `sy-ucomm` |
|---|---|
| Enter | `''` |
| F1 bis F12 | `CODE` der `FKEY`-Zeile mit dieser Taste, sonst `''` |
| Esc | `CANCEL`, wenn im GUI-Status definiert, sonst `''` (**Entscheidung**, bestätigt) |
| Drucktaste | `CODE` der `BUTTON`, wenn sie fokussiert ist und Enter gedrückt wird |
| Tab / Shift+Tab | kein Funktionscode, Cursor zum nächsten bzw. vorherigen Eingabefeld |
| Shift+F1 bis F12 | Funktionscode für F13 bis F24 (Phase 2) |

Die F-Taste-Zuordnung hängt am GUI-Status, nicht am Dynpro.

### 6.3 Eingabe im Terminal

- Druckbare Zeichen überschreiben das Feld ab dem Cursor.
- Backspace links, Delete unter dem Cursor, Pfeiltasten innerhalb des Feldes und zwischen Feldern.
- Ein Zeichen, das das Feld überlaufen würde, wird abgewiesen.
- Ein Zeichen, das nicht zum Typ passt (Buchstabe in `n`), wird sofort abgewiesen.

### 6.4 Fehler bei der Übertragung

- `REQUIRED` leer oder Typfehler (z. B. ungültiges Datum): Meldung E in Zeile 24, Cursor auf dem Feld.
  Kein PAI-Lauf, die Eingabe bleibt.
- Bei `B`, `C`, `E` (Zurück, Abbrechen, Beenden) entfallen die Prüfungen. Ob Werte dann übertragen werden, ist zu prüfen.

### 6.5 Meldungen

| Typ | Wirkung |
|---|---|
| S, I, W | Meldung in Zeile 24, Ablauf geht weiter. |
| Typ klein geschrieben (`'s'`) | Übersetzungsfehler, kein Laufzeitverhalten. |
| E | im PAI: Dynpro bleibt, PBO läuft erneut, Meldung bleibt. Im PBO: Meldung in Zeile 24. |
| A, X | Programmabbruch mit englischer Laufzeitmeldung. |

## 7. Anzeige (Terminal)

- `crossterm`: Alternativer Bildschirm und Rohmodus beim Start. Wiederherstellung beim Beenden **und** beim Panic
  (Panic-Hook). Ohne das bleibt das Terminal unbrauchbar.
- Gitter 80 × 24 als Zwischenpuffer. Nur geänderte Zellen werden geschrieben.
- Eingabefelder unterstrichen oder hinterlegt. Ausgabe und Texte normal.
- Darstellung nicht SAP-genau. Position, Länge und Eingabefähigkeit müssen stimmen.

## 8. Architektur

### 8.1 Verteilung

| Ort | Inhalt |
|---|---|
| `abap-compiler` (`dynpro/`, neu) | Parser für `.dynp`, Prüfung gegen das Programm (4.6), Übersetzung in IR |
| `abap-compiler` `codegen/` | erzeugt die Aufrufe aus 8.2 und die Feldtabelle mit Adressen der Variablen |
| `abap-rt` `screen/` (neu) | Schleife (6.1), Stapel, Zustand, Konvertierung ABAP-Wert ↔ Anzeigetext, Trait für das Terminal |
| `abap-rt` `screen/term.rs` (neu) | `crossterm`-Backend |
| `abap-rt` `screen/headless.rs` (neu) | Headless-Backend für Tests (9.1) |

Die Konvertierung (`i`, `p`, `n`, `d`, `t`) nutzt `decimal.rs` und `packed.rs` aus `abap-rt`, ohne Zyklus.

**Risiko vor Phase 1:** `crossterm` als Abhängigkeit der `staticlib` muss beim Linken der erzeugten
`.exe` auf Windows/MSVC funktionieren (Systembibliotheken, die `crossterm` nachzieht). Das ist der erste
Schritt in Phase 1, bevor weitere Arbeit darauf aufbaut.

### 8.2 Übersetzung in Aufrufe

Der Compiler übersetzt `.dynp` in eine Folge von Aufrufen. Sie stehen im Programmstart (Prolog), in der
Reihenfolge der Datei, **vor** dem ersten Anweisungsblock. Die Aufrufe bilden die Dynpros ab, wie der Compiler
ABAP-Anweisungen in Aufrufe übersetzt.

Konventionen wie in `abap-rt` bisher: Zeichen als UTF-16 mit `(ptr: *const u16, len: u64)`, Namen mit Präfix `abap_rt_`.

**Prolog-Aufrufe (aus der `.dynp`):**

```
abap_rt_screen_sy(ucomm: *mut u16, ucomm_len: u64, dynnr: *mut u16, dynnr_len: u64)
    -- registriert SY-UCOMM und SY-DYNNR (vom Codegen angelegt)

abap_rt_screen_begin(no: u32, kind: u32, rows: u32, cols: u32, title: *const u16, title_len: u64)
    -- kind: 0 = NORMAL, 1 = MODAL

abap_rt_screen_text(row: u32, col: u32, text: *const u16, len: u64, attrs: u32)

abap_rt_screen_field(row: u32, col: u32, name: *const u16, name_len: u64,
                     data: *mut u8, abap_type: u32, length: u32, decimals: u32, attrs: u32)
    -- abap_type: 0 c, 1 n, 2 d, 3 t, 4 i, 5 p
    -- attrs: Bitmaske INPUT, OUTPUT, REQUIRED, INVISIBLE, PASSWORD, INTENSIFIED, COLOR (Bits 8..10)

abap_rt_screen_button(row: u32, col: u32, text: *const u16, text_len: u64,
                      fcode: *const u16, fcode_len: u64, attrs: u32)

abap_rt_screen_frame(row1: u32, col1: u32, row2: u32, col2: u32,
                     title: *const u16, title_len: u64)

abap_rt_screen_module(no: u32, phase: u32, func: extern "C" fn(), at_exit: u32)
    -- phase: 0 = PBO, 1 = PAI

abap_rt_screen_end(no: u32)

abap_rt_status_begin(name: *const u16, name_len: u64)
abap_rt_status_fkey(key: u32, text: *const u16, text_len: u64,
                    fcode: *const u16, fcode_len: u64, kind: u32)
    -- key: 1..24, kind: 0 = keine, 1 = B, 2 = C, 3 = E
abap_rt_status_exclude(fcode: *const u16, fcode_len: u64)   -- Phase 2
abap_rt_status_end()
abap_rt_status_exclude(fcode: *const u16, fcode_len: u64)      -- Phase 2, gilt für den zuletzt begonnenen Status
abap_rt_set_cursor(name: *const u16, name_len: u64)             -- Phase 2, SET CURSOR FIELD f (nur Feldname)
abap_rt_get_cursor(dst: *mut u16, dst_len: u64)                 -- Phase 2, GET CURSOR FIELD f
abap_rt_set_titlebar(text: *const u16, text_len: u64)           -- Phase 2, SET TITLEBAR 'text' (nur Literal)
```

`abap_rt_screen_module` übergibt einen Funktionszeiger auf die Modul-Funktion, die der Codegen erzeugt. Die
Modulfunktion hat die Signatur `extern "C" fn()`.

Für `abap_rt_screen_field` ist `data` die Adresse der globalen Variable im Datenbereich des Programms
(Layout: `codegen/layout.rs`). Ein Feld ohne Variable ist ein Übersetzungsfehler.

**Aufrufe aus den ABAP-Anweisungen:** siehe Abschnitt 5 (Spalte „Laufzeitaufruf").

Die Aufrufe bekommen das Ergebnis über Rückgabewerte oder über den Zustand im Runtime-Objekt. Fehler im Aufruf
(z. B. unbekannte Dynpro-Nummer) sind Laufzeitfehler, kein `panic!` (CLAUDE.md).

### 8.3 Farben

| SAP-Farbnummer | Terminal |
|---|---|
| 1 | grau |
| 2 | blau |
| 3 | grün |
| 4 | rot |
| 5 | gelb |
| 6 | cyan |
| 7 | weiß |

Farben sind nur Hinweise. Eingabefelder sind auch ohne Farbe erkennbar.

## 9. Tests

### 9.1 Headless-Backend

Das Terminal-Trait hat zwei Implementierungen. `crossterm` für den echten Lauf, Headless für Tests: zeichnet in
ein Gitter und liest Tasten aus einer Skriptdatei. Die Auswahl erfolgt per Umgebungsvariable
(`ABAPRT_SCREEN_SCRIPT=<datei>` startet Headless). Ohne sie und mit Terminal: `crossterm`.

### 9.2 Akzeptanzfälle

Ein Fall unter `tests/acceptance/<fall>/`:

| Datei | Inhalt |
|---|---|
| `prog.abap` | ABAP-Quelltext |
| `<name>.dynp` | Dynpro-Beschreibung (nur mit `CALL SCREEN`) |
| `input.keys` | Eingabeskript (9.3) |
| `expected.txt` | exakte stdout-Ausgabe des Programms |
| `expected_screen.txt` | Gitterabzüge aus `SNAPSHOT` |

Der Runner aus `abap-compiler/tests/acceptance.rs` vergleicht die Abzüge zeichengenau.

### 9.3 Eingabeskript

```
FIELD ZKUNNR            ' Cursor auf das Feld (nur Test)
TYPE '4711'             ' Zeichen am Cursor schreiben (Überschreibmodus)
KEY TAB                 ' ENTER, TAB, BTAB, F1..F12, SHIFT-F1..SHIFT-F12 (= F13..F24), ESC, BACKSPACE, DELETE, UP, DOWN, LEFT, RIGHT
KEY ENTER
SNAPSHOT                ' Gitter in expected_screen.txt
```

`'` beginnt einen Kommentar. Die Skriptsprache gilt nur für Tests.

### 9.3a Gitterabzug (Format, Stand Phase 1)

Ein `SNAPSHOT` schreibt den aktuellen Bildschirm als Text:

- 24 Zeilen, jede genau 80 Zeichen breit (Leerzeichen aufgefüllt, Zeichen je Zelle, keine Breitzeichen).
- Danach eine Trennzeile `----`. Pro `SNAPSHOT` ein Block.
- Ziel: Datei aus `ABAPRT_SCREEN_OUT`. Ohne diese Variable gehen die Abzüge nach stderr.

Offen (Entscheidung nötig): Sollen rechte Leerzeichen in den Zeilen entfernt werden, damit die Dateien
im Editor lesbar bleiben und der Vergleich auf Zeilenebene erfolgt? Vorschlag: ja, Zeilen rechts getrimmt,
Vergleich zeichengenau über die getrimmten Zeilen.

### 9.4 Weitere Tests

- Parser: gültige und ungültige Dateien mit Position.
- Prüfungen aus 4.6.
- Prolog-Aufrufe: Reihenfolge und Argumente, Vergleich mit einer erwarteten Liste.
- Schleife: PBO → Anzeige → Eingabe → Übertragung → PAI, auch mit `LEAVE` und `SET SCREEN` aus Modulen.
- Konvertierung: jeder Typ aus 4.7, gültig und ungültig.

## 10. Umfang nach Phasen

| Phase | Inhalt |
|---|---|
| **0** | Linktest: `abap-rt` mit `crossterm` in eine `.exe` linken und ein leeres Fenster (Alternativscreen) öffnen und schließen. |
| **1 (MVP)** | `DYNPRO`, `SIZE`, `TITLE`, `TEXT`, `FIELD` mit `INPUT`/`OUTPUT`/`REQUIRED`/`INVISIBLE`/`COLOR`, `BUTTON`, `FRAME`. `FLOW` mit `PROCESS` und `MODULE` (ohne Feldmodule), inkl. `AT EXIT-COMMAND` (Semantik offen, Abschnitt 12). `GUISTATUS` mit `FKEY`. `CALL SCREEN`, `SET SCREEN`, `LEAVE SCREEN`, `LEAVE TO SCREEN`, `LEAVE PROGRAM`, `SET PF-STATUS`, `MESSAGE`, `sy-ucomm`. Typen `c`, `n`, `d`, `t`, `i`, `p`. Headless-Tests und `crossterm`-Lauf. |
| **2** | `CHAIN`/`ENDCHAIN`, `FIELD ... MODULE ... ON INPUT`, `ON REQUEST` (einfache Auswahlliste), `SET CURSOR`, `GET CURSOR`, `EXCLUDE`, `Shift+F`, `PASSWORD`, `CALL SCREEN ... STARTING AT`, `SET TITLEBAR`, Strukturkomponenten als Feld. |
| **3** | Subscreens, Tabellenkontrollen (`LOOP AT ... WITH CONTROL`), Registerleisten, Selektionsbilder. |

## 11. Hinweise zur Umsetzung

- Die Umstellung der bestehenden deutschen Meldungen auf Englisch ist ein eigener Schritt vor oder mit Phase 1.
  Betroffen sind laut Bestand: etwa 100 Zeichenketten im Compiler (`lower/`, `codegen/`, `analyze.rs`, `ddic.rs`,
  `symbols.rs`, u. a.), 10 Stellen in `abap-rt` (`lib.rs`, `decimal.rs`, `packed.rs`) und 10 Akzeptanzfälle mit
  `expected_error.txt` bzw. `expected_runtime_error.txt`. Die Fälle werden mit angepasst.
- Ohne Sprachabhängigkeit: Keine Übersetzungstabellen, keine Umgebungsabhängigkeit bei Meldungen.

## 12. Zu prüfendes SAP-Verhalten

Der `sap-specialist` klärt diese Punkte, bevor sie als Akzeptanzfall festgelegt werden:

1. Ist der Standard eines Feldes ohne Attribut `INPUT`?
2. Ab wann greift `REQUIRED`: im GUI vor dem PAI oder im PAI? Welche Meldung?
3. Welche Daten werden bei `B`, `C`, `E` übertragen, welche Prüfungen entfallen?
4. Ablauf nach `MESSAGE ... TYPE 'E'` im PAI: läuft das PBO erneut? Bleibt die Eingabe?
5. Verhalten von `LEAVE TO SCREEN 0` gegenüber `LEAVE SCREEN`.
6. Mehrere `SET SCREEN` in einem Modul: welche Aktion gilt?
7. Typeinschränkungen für Felder (`string`, `i`, `p`, `d`, `t`) und ihre Darstellung (Vorzeichen, Nachkommastellen).
8. **Semantik von `AT EXIT-COMMAND`.** Vorgabe des Auftraggebers, Quelle: vom Auftraggeber eingefügter
   Text zu SAP-Verhalten (nicht unabhängig am SAP-System geprüft). Umgesetzt in Phase 1:
   - Das Exit-Modul läuft nur bei Funktionscodes vom Typ `E`. Bei allen anderen Typen wird es ignoriert.
   - Es läuft vor allen anderen PAI-Modulen und vor den Prüfungen, unabhängig von seiner Position in der Ablauflogik.
   - Pro Dynpro ist höchstens ein Exit-Modul erlaubt (Übersetzungsfehler sonst).
   - Vor dem Exit-Modul werden keine Felder übertragen. `SY-UCOMM` ist gesetzt. Die Programmfelder behalten ihre Werte aus dem PBO.
   - Verlässt das Modul das Dynpro (`LEAVE SCREEN`, `LEAVE TO SCREEN`, `SET SCREEN`, `LEAVE PROGRAM`), endet das PAI.
   - Verlässt es das Dynpro nicht, laufen Übertragung, alle Prüfungen und die übrigen PAI-Module normal weiter.
     Das Exit-Modul wird dabei nicht noch einmal aufgerufen.
   - Die Prüfungen (Pflichtfelder, Format, Festwerte und Prüftabellen) entfallen vor dem Exit-Modul vollständig.
   - Offen: Der Text erwähnt ein OK-Code-Feld, das den Funktionscode aufnimmt. Dieses Feld gibt es in Phase 1 nicht.
     Der Funktionscode steht nur in `SY-UCOMM`.
   Die Regeln für Funktionstypen B und C ohne Exit-Modul (lockere Übertragung, Abschnitt 6.4) sind davon nicht berührt.
9. **`STATUS` im `DYNPRO`** entfällt. Der GUI-Status wird nur mit `SET PF-STATUS` in einem PBO-Modul gesetzt.
   Das PBO läuft vor der Anzeige, daher reicht das.

10. **Phase-2-Teilmenge (Stand): Annahmen, die noch nicht belegt sind.**
   - `PASSWORD`: Stern nur für Zeichen ungleich Leerzeichen. Ein eingetipptes Leerzeichen bleibt leer. Der Wert im Programm ist der Klartext.
   - `EXCLUDE`: gilt nur im gesetzten Status, Vergleich exakt. Der Compiler warnt, wenn kein FKEY denselben Code hat.
   - `SET CURSOR FIELD f`: nur Bezeichner-Form. Feld muss sichtbar sein. Der Name muss in mindestens einem DYNPRO der `.dynp` vorkommen. Wirkt auf die nächste Anzeige des obersten Dynpros.
   - `GET CURSOR FIELD f`: liest den Cursor der letzten Anzeige bzw. Übertragung. Ohne Feld unter dem Cursor: Leerzeichen.
   - `SET TITLEBAR 'text'`: nur Literal ohne `WITH`. Textsymbol-Form ist „not supported yet“. Gilt bis zum Verlassen des Dynpros.
   - Strukturkomponenten: nur `struktur-komponente` einer Struktur auf Programmebene mit Typen c, n, d, t, i, p.
   - Verhalten von `GET CURSOR` und `PASSWORD` (Übertragung beim PAI) ist noch nicht gegen SAP belegt.

## 12a. Ablauf-Zusätze: FIELD, CHAIN, ON INPUT, ON REQUEST (Phase 2)

Quelle: vom Auftraggeber eingefügter Text aus allgemeinem ABAP-Wissen, nicht gegen die Dokumentation
geprüft. Vor produktivem Einsatz gegen die Schlüsselwortdokumentation abgleichen (`FIELD ... MODULE ... ON REQUEST`).

**Syntax in der PAI-Ablauflogik (`.dynp`, Block `FLOW`):**

```
PROCESS AFTER INPUT
  FIELD ZKUNNR MODULE CHECK_KUNNR ON REQUEST
  CHAIN
    FIELD ZWERK
    FIELD ZLGORT
    MODULE CHECK_LAGER ON CHAIN-REQUEST
  ENDCHAIN
  MODULE USER_COMMAND_0100
```

- `FIELD f` ohne `MODULE`: nur Übertragung an dieser Stelle.
- `FIELD f MODULE m` ohne Zusatz: Modul läuft immer.
- `FIELD f MODULE m ON INPUT`: Modul läuft nur, wenn der Wert des Feldes nach der Übertragung ungleich dem Initialwert ist.
- `FIELD f MODULE m ON REQUEST`: Modul läuft nur, wenn der Benutzer seit dem letzten PBO in das Feld eingegeben hat.
- `CHAIN` ... `ENDCHAIN`: gruppiert Felder. Innerhalb nur `FIELD f` (ohne `MODULE`) und höchstens ein `MODULE m ON CHAIN-INPUT` oder `MODULE m ON CHAIN-REQUEST`. Verschachteln ist nicht erlaubt.
- `ON CHAIN-INPUT`: Modul läuft, wenn mindestens ein Kettenfeld ungleich Initialwert ist.
- `ON CHAIN-REQUEST`: Modul läuft, wenn mindestens ein Kettenfeld seit dem letzten PBO eine Eingabe hatte.
- `MODULE` mit `ON CHAIN-*` außerhalb einer CHAIN ist ein Übersetzungsfehler. `ON INPUT` und `ON REQUEST` gelten nur für `FIELD`.
- `ON *-INPUT` (Asterisk-Eingabe) ist nicht Teil von Phase 2.

**Eingabemarkierung (ON REQUEST):** Jede Eingabe in ein Feld (Zeichen, Löschen, Überschreiben mit gleichem Wert) setzt
die Markierung. Die Markierung wird bei jedem PBO zurückgesetzt. Änderungen durch das Programm im PBO zählen nicht.
Cursorbewegung ohne Änderung zählt nicht. Vorbelegung aus SPA/GPA-Parametern zählt nicht (Annahme, Abschnitt 12).

**Initialwert (ON INPUT):** wie bei `REQUIRED`: c nur Leerzeichen, n/d/t nur Nullen, i/p Null.

**Übertragung im PAI:**
- Prüfungen (Pflicht, Format, Typ) laufen wie bisher vor allen Modulen für alle Felder.
- Felder, die in keiner `FIELD`-Anweisung (auch nicht in einer CHAIN) stehen, werden zu Beginn des PAI übertragen.
- Felder einer `FIELD`-Anweisung werden erst bei dieser Anweisung in die ABAP-Variable übertragen. Frühere Module sehen den alten Wert.
- Felder einer CHAIN werden bei ihrer `FIELD`-Anweisung übertragen (Annahme: die Kette ist dabei noch nicht verarbeitet).
- Das PAI arbeitet die Anweisungen in Quelltextfolge ab. `MODULE` ohne Feldbezug läuft an seiner Stelle.

**Meldung E oder W aus einem Feld- oder Kettenmodul (Annahme, Abschnitt 12):**
- Das Dynpro wird neu angezeigt, mit PBO wie in Abschnitt 6.5. Eingabebereit sind nur das Feld des Moduls bzw. alle Felder der Kette.
- Nach der Korrektur und Enter läuft das PAI ab dieser Anweisung weiter (das fehlgeschlagene Modul wird erneut aufgerufen,
  Anweisungen davor laufen nicht erneut).
- Für Module ohne Feldbezug gilt weiter Abschnitt 6.5.

**Festlegungen dieses Abschnitts (Entscheidungen, siehe Bericht):**
- Pro Dynpro gibt es eine geordnete Liste von PAI-Einträgen (Feld, Kette, Modul, Exit-Modul). Die Reihenfolge ist die Quelltextfolge.
- Ein Feld in mehr als einer `FIELD`-Anweisung ist ein Übersetzungsfehler.
- `ON INPUT` feuert bei jedem PAI, solange der Wert ungleich Initialwert ist, auch ohne neue Eingabe (Akzeptanzfall `m13_dynpro_on_request`).
- Eingabemarkierung: nur tatsächlich ausgeführte Änderungen zählen. Überschreiben mit gleichem Zeichen zählt. Abgewiesene Zeichen (Typ, Feldende, gesperrtes Feld) nicht.
- Cursor (Pfeiltasten, `SET CURSOR`) darf auf gesperrte Felder. Tippen, Löschen und Tab wirken nur auf eingabebereite Felder.
- Übertragung ohne Prüfungserfolg behält Wiederaufnahme und Beschränkung.
- Akzeptanzfall `m13_dynpro_on_request` zeigt den Ablauf mit Eingabeskript und Gitterabzug.

**ABI für die PAI-Liste (Prolog-Aufrufe, in Quelltextfolge; `PAI-Module` bleiben bei `abap_rt_screen_module`):**

```
abap_rt_screen_pai_field(no: u32, name: *const u16, name_len: u64,
                         func: Option<extern "C" fn()>, cond: u32 /*0 ohne, 1 ON INPUT, 2 ON REQUEST*/,
                         chain: u32 /*0 ohne Kette, sonst Kettennummer ab 1*/)
abap_rt_screen_pai_chain(no: u32, chain: u32, func: extern "C" fn(), cond: u32 /*3 ON CHAIN-INPUT, 4 ON CHAIN-REQUEST*/)
```

Die Eingabemarkierung führt die Laufzeit selbst. Der Compiler gibt sie nicht vor.

## 12b. Wertehilfe F4: PROCESS ON VALUE-REQUEST (Phase 2)

Quelle: vom Auftraggeber eingefügter Text aus allgemeinem ABAP-Wissen, nicht gegen die Dokumentation geprüft.

- Syntax im Block `FLOW`: `PROCESS ON VALUE-REQUEST` mit Zeilen `FIELD <name> MODULE <m>`. Die Module sind INPUT-Module.
- F4 auf einem Feld mit POV-Modul läuft nur dieses Modul. Kein PAI, keine automatischen Prüfungen, kein PBO danach.
- Ein POV-Modul hat Vorrang vor Suchhilfen (es gibt in der Laufzeit keine Suchhilfen).
- Feldtransport: Nichts wird automatisch ins Programm übertragen, auch das F4-Feld nicht. Nicht bestätigte Eingaben sind im Programm nicht sichtbar.
- Felder lesen: `CALL FUNCTION 'DYNP_VALUES_READ'` (Felder aus dem aktuellen Dynpro).
- Ergebnis zurück: `CALL FUNCTION 'F4IF_INT_TABLE_VALUE_REQUEST'` (Auswahlliste, Ergebnis in das F4-Feld), oder das Feld direkt im Modul setzen. Weitere Felder mit `CALL FUNCTION 'DYNP_VALUES_UPDATE'` aktualisieren.
- **Entscheidung (Umfang):** In der Laufzeit werden nur diese drei Funktionsbausteine nachgebildet. Alle anderen `CALL FUNCTION` bleiben unbekannt wie bisher (Laufzeitfehler).
- **Entscheidung (Auswahlliste):** Einfache Liste im Vollbild des Dynpro-Bereichs, Pfeiltasten und Enter wählen, Esc bricht ab. Bei Abbruch bleibt das Feld unverändert.
- Der Rückgabewert von `F4IF_INT_TABLE_VALUE_REQUEST` wird nur in das F4-Feld geschrieben, wenn `VALUE_ORG = 'S'` übergeben wird (Annahme nach Text, zu prüfen).
- Offen: Ob das F4-Feld nach dem POV-Modul ohne Enter zurücktransportiert wird (Text: „sollten Sie testen“).

**Umsetzungsstand (Option B, Entscheidung des Auftraggebers):** Nur der POV-Teil. Die Funktionsbausteine `DYNP_VALUES_READ`, `DYNP_VALUES_UPDATE` und `F4IF_INT_TABLE_VALUE_REQUEST` sind nicht umgesetzt. Das Modul setzt das Feld selbst.
- Taste F4 auf einem Feld mit POV-Modul: Das Modul läuft. Danach kein PAI, keine Prüfungen, kein PBO-Lauf.
- Die Anzeige wird nur für das F4-Feld aus der ABAP-Variable neu gezeichnet. Andere Felder behalten, was der Benutzer getippt hat.
- F4 auf einem Feld ohne POV-Modul: keine Wirkung (keine Suchhilfe in der Laufzeit).
- Ein POV-Modul ist ein INPUT-Modul.
- Die Eingabemarkierung wird durch POV nicht verändert.

## 12c. Modalfenster: CALL SCREEN ... STARTING AT (Phase 2)

- Das Fenster liegt über dem darunterliegenden Dynpro, mit einem Rahmen aus `+`, `-`, `|` (Abschnitt 8.3, Phase 1).
- Position: `STARTING AT x y` ist Spalte x, Zeile y im Terminal. `ENDING AT x2 y2` setzt die rechte untere Ecke. Ohne `ENDING AT` ergibt sich die Größe aus dem Dynpro (`SIZE`).
- Der Inhalt des Fensters ist der Dynpro-Bereich, zusätzlich eine Titelzeile im Rahmen aus `TITLE`.
- Das Fenster überdeckt die Zellen; das darunterliegende Dynpro wird nicht neu gezeichnet und bleibt sichtbar außerhalb des Fensters.
- Beim Verlassen des Fensters wird der Bereich des Fensters mit dem darunterliegenden Dynpro neu gezeichnet.
- Funktionstastenleiste und Meldungszeile gehören zum Hauptbildschirm und bleiben unverändert, außer wenn das Fenster sie überdeckt (dann gewinnt das Fenster).
- **Umsetzung (Entscheidungen):**
  - Der GUI-Status des Fensters bestimmt die Funktionscodes, solange das Fenster oben liegt. Seine Tastenleiste wird nicht gezeichnet (Annahme, vom Auftraggeber zu bestätigen).
  - Der Titel beginnt eine Zelle nach der Ecke und wird auf die Innenbreite gekürzt. Das `FRAME`-Element setzt den Titel mit Leerzeichen, also zwei Zellen nach der Ecke. Die beiden sind noch nicht angeglichen.
  - Beim Verlassen wird das Bild vor dem Öffnen wiederhergestellt. Das Hauptdynpro zeichnet sich im nächsten PBO mit den aktuellen Werten (Annahme).
  - `SET SCREEN` im Fenster wechselt nur den Inhalt. Die Lage bleibt. Das neue Dynpro muss in die Innenfläche passen, seine Art (KIND) wird nicht geprüft (Annahme, vom Auftraggeber zu bestätigen).
  - Innenfläche ohne `ENDING AT`: `SIZE`. Mit `ENDING AT`: (y2−y−1) × (x2−x−1).
  - Ein normales `CALL SCREEN` auf ein `KIND MODAL`-Dynpro zeigt es im Vollbild (nicht spezifiziert).

## 13. Offene Punkte

1. **Modalfenster** (`CALL SCREEN ... STARTING AT`): **Entschieden:** Das Fenster hat einen sichtbaren Rahmen.
   Es überlappt das darunterliegende Dynpro, das Dynpro bleibt sichtbar und wird nicht gelöscht. Siehe Abschnitt 12c.
2. **Tastenbelegung der Tabulatortasten:** Tab und Shift+Tab sind in Terminals teils mit anderen Funktionen belegt.
   Zu testen auf Windows Terminal und der klassischen Konsole.
3. **Startpunkt:** Phase 0 (Linktest) vor allem anderen.

## 14. Nächste Schritte

1. Phase 0: `crossterm` in `abap-rt` einbinden und den Linktest in eine `.exe` machen.
2. Umstellung der Meldungen auf Englisch (Abschnitt 11) als eigener Schritt, mit Anpassung der Akzeptanzfälle.
3. Punkte aus Abschnitt 12 durch den `sap-specialist` klären.
4. Phase 1: Parser und Prüfung im Compiler, Prolog-Codegen, `screen/` mit Headless-Backend, danach `crossterm`.
