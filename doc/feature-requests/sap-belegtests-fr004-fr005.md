# Belegtests für FR-004 und FR-005 im SAP-System

Zweck: Verhalten von `i` → `c`, `WRITE` und Konvertierungsroutinen (ALPHA) in einem echten SAP-System prüfen. Die
Ausgabe kommt danach zurück und wird in FR-004 und FR-005 als Beleg eingetragen.

## Vorgehen

1. In einem Entwicklungs- oder Testsystem (nicht produktiv) mit `SE38` ein Report `ZFR005_BELEGTEST` anlegen,
   Typ „Executable Program“.
2. Den Quelltext unten einfügen, prüfen, aktivieren und mit `F8` ausführen.
3. Den Listen-Output vollständig kopieren (Liste → Speichern oder Markieren und Kopieren).
4. Dazu angeben: SAP-Release (z. B. über `System → Status`, Feld „Release“ bzw. „SAP_BASIS“). Der Report legt keine
   Datenbankdaten an und liest nur die Domäne `MATNR`.
5. Den Report nicht transportieren. Nach dem Test löschen oder im Entwicklungssystem liegen lassen.

Die Ausgabe ist in `|…|` eingerahmt. Leerzeichen erscheinen als `_`, damit die Breite sichtbar ist.

## Quelltext

```abap
REPORT zfr005_belegtest.

DATA: lv_i   TYPE i,
      lv_c5  TYPE c LENGTH 5,
      lv_c9  TYPE c LENGTH 9,
      lv_c10 TYPE c LENGTH 10,
      lv_c15 TYPE c LENGTH 15,
      lv_c18 TYPE c LENGTH 18,
      lv_n   TYPE n LENGTH 6,
      lv_mat TYPE matnr,
      lx     TYPE REF TO cx_root.

DEFINE show_c.
  TRANSLATE &1 USING ' _'.
  WRITE: / &2, '|', &1, '|'.
END-OF-DEFINITION.

START-OF-SELECTION.

  " T1: i -> c, Feldlänge 15
  lv_i = 42.
  lv_c15 = lv_i.
  show_c lv_c15 'T1 i=42   -> c15'.
  lv_i = -42.
  lv_c15 = lv_i.
  show_c lv_c15 'T1 i=-42  -> c15'.

  " T2: i -> c, Feldlänge 10
  lv_i = 42.
  lv_c10 = lv_i.
  show_c lv_c10 'T2 i=42   -> c10'.
  lv_i = -42.
  TRY.
      lv_c10 = lv_i.
      show_c lv_c10 'T2 i=-42  -> c10'.
    CATCH cx_root INTO lx.
      WRITE: / 'T2 i=-42  -> c10 EXCEPTION:', lx->get_text( ).
  ENDTRY.

  " T3: i -> c, Feldlänge 9 und 5 (kürzer als die Ausgabe)
  lv_i = 42.
  TRY.
      lv_c9 = lv_i.
      show_c lv_c9 'T3 i=42   -> c9'.
    CATCH cx_root INTO lx.
      WRITE: / 'T3 i=42   -> c9 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  TRY.
      lv_c5 = lv_i.
      show_c lv_c5 'T3 i=42   -> c5'.
    CATCH cx_root INTO lx.
      WRITE: / 'T3 i=42   -> c5 EXCEPTION:', lx->get_text( ).
  ENDTRY.

  " T4: WRITE einer ganzen Zahl (Vergleich)
  lv_i = 42.
  WRITE: / 'T4 WRITE 42  :', lv_i.
  lv_i = -42.
  WRITE: / 'T4 WRITE -42 :', lv_i.

  " T5: Typ n ohne Domäne, Ausgabe und Umwandlung nach c
  lv_n = 42.
  WRITE: / 'T5 WRITE n6=42   :', lv_n.
  lv_c15 = lv_n.
  show_c lv_c15 'T5 n6=42 -> c15'.

  " T6: MATNR (Domäne mit ALPHA-Konvertierungsroutine)
  lv_mat = '42'.
  lv_c18 = lv_mat.
  show_c lv_c18 'T6 matnr=42 -> c18 (Rohwert, ohne WRITE)'.
  WRITE: / 'T6 WRITE matnr=42:', lv_mat.

  " T7: c-Feld mit gleicher Zahl, ohne Domäne
  lv_c18 = '42'.
  WRITE: / 'T7 WRITE c18=42  :', lv_c18.

  " T8: Funktionsbaustein ALPHA_INPUT und ALPHA_OUTPUT direkt
  CALL FUNCTION 'CONVERSION_EXIT_ALPHA_INPUT'
    EXPORTING
      input  = '42'
    IMPORTING
      output = lv_c18.
  show_c lv_c18 'T8 ALPHA_INPUT(42) -> c18'.
  CALL FUNCTION 'CONVERSION_EXIT_ALPHA_OUTPUT'
    EXPORTING
      input  = lv_c18
    IMPORTING
      output = lv_c18.
  show_c lv_c18 'T8 ALPHA_OUTPUT(...) -> c18'.

  " T9: Überlauf bei i -> c (mehr Ziffern als Platz)
  lv_i = 12345.
  TRY.
      lv_c5 = lv_i.
      show_c lv_c5 'T9 i=12345  -> c5'.
    CATCH cx_root INTO lx.
      WRITE: / 'T9 i=12345  -> c5 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = -12345.
  TRY.
      lv_c5 = lv_i.
      show_c lv_c5 'T9 i=-12345 -> c5'.
    CATCH cx_root INTO lx.
      WRITE: / 'T9 i=-12345 -> c5 EXCEPTION:', lx->get_text( ).
  ENDTRY.

  " T10: Grenze der Stellen bei i -> c9 und c10
  lv_i = 12345678.
  TRY.
      lv_c9 = lv_i.
      show_c lv_c9 'T10 i=12345678 -> c9'.
    CATCH cx_root INTO lx.
      WRITE: / 'T10 i=12345678 -> c9 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = 123456789.
  TRY.
      lv_c9 = lv_i.
      show_c lv_c9 'T10 i=123456789 -> c9'.
    CATCH cx_root INTO lx.
      WRITE: / 'T10 i=123456789 -> c9 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = 123456789.
  lv_c10 = lv_i.
  show_c lv_c10 'T10 i=123456789 -> c10'.

  " T11: gespeicherte führende Nullen in MATNR und c-Feld, WRITE
  lv_mat = '0000000000000042'.
  WRITE: / 'T11 WRITE matnr mit Nullen:', lv_mat.
  lv_c18 = '000000000000000042'.
  WRITE: / 'T11 WRITE c18 mit Nullen  :', lv_c18.

  " T12: Positiver Überlauf und Randwerte bei i -> c
  lv_i = 123456.
  TRY.
      lv_c5 = lv_i.
      show_c lv_c5 'T12 i=123456  -> c5'.
    CATCH cx_root INTO lx.
      WRITE: / 'T12 i=123456  -> c5 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = -123456.
  TRY.
      lv_c5 = lv_i.
      show_c lv_c5 'T12 i=-123456 -> c5'.
    CATCH cx_root INTO lx.
      WRITE: / 'T12 i=-123456 -> c5 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = 1234.
  lv_c5 = lv_i.
  show_c lv_c5 'T12 i=1234 -> c5'.
  lv_i = 0.
  TRY.
      DATA lv_c1 TYPE c LENGTH 1.
      lv_c1 = lv_i.
      show_c lv_c1 'T12 i=0 -> c1'.
    CATCH cx_root INTO lx.
      WRITE: / 'T12 i=0 -> c1 EXCEPTION:', lx->get_text( ).
  ENDTRY.
  lv_i = -5.
  TRY.
      lv_c1 = lv_i.
      show_c lv_c1 'T12 i=-5 -> c1'.
    CATCH cx_root INTO lx.
      WRITE: / 'T12 i=-5 -> c1 EXCEPTION:', lx->get_text( ).
  ENDTRY.
```

## Was zurückgeschickt werden soll

- Der vollständige Listen-Output.
- Das SAP-Release.
- Falls ein Test einen Fehler oder einen Dump auslöst: die Meldung oder den Kurztext des Dumps (ST22), mit dem
  Testnamen.

## Was die Tests klären

- T1 und T2: Wie `i` → `c` bei Feldlänge 15 und 10 aussieht, und was bei negativer Zahl und Länge 10 passiert.
  Das entscheidet die Längenregel aus FR-004.
- T3: Verhalten bei zu kurzem Feld (Abschneiden mit `*` oder Fehler). FR-004 verlangt einen Übersetzungsfehler.
- T4: Bestätigt die Ausgabebreite (10 Zeichen, Minus danach), auf die FR-004 sich stützt.
- T5: Ob ein `n`-Feld führende Nullen behält.
- T6 und T7: Ob `WRITE` bei `MATNR` die ALPHA-Routine anwendet. Das ist die Aussage, die in FR-005 steht.
- T8: Wie die ALPHA-Routinen direkt arbeiten, unabhängig von `WRITE`.

## SAP-Ausgabe (S/4HANA 2025, Kernel 916, Angabe des Nutzers)

Listenausgabe des Reports, vom Nutzer per Listen-Export übermittelt. Die `|`-Markierungen stammen aus `show_c`.

```
T1 i=42   -> c15 | ____________42_ |
T1 i=-42  -> c15 | ____________42- |
T2 i=42   -> c10 | _______42_ |
T2 i=-42  -> c10 | _______42- |
T3 i=42   -> c9 | ______42_ |
T3 i=42   -> c5 | __42_ |
T4 WRITE 42  :         42
T4 WRITE -42 :         42-
T5 WRITE n6=42   : 000042
T5 n6=42 -> c15 | 000042_________ |
T6 matnr=42 -> c18 (Rohwert, ohne WRITE) | 42________________ |
T6 WRITE matnr=42: 42
T7 WRITE c18=42  : 42
T8 ALPHA_INPUT(42) -> c18 | 000000000000000042 |
T8 ALPHA_OUTPUT(...) -> c18 | 42________________ |
T9 i=12345  -> c5 | 12345 |
T9 i=-12345 -> c5 | *345- |
T10 i=12345678 -> c9 | 12345678_ |
T10 i=123456789 -> c9 | 123456789 |
T10 i=123456789 -> c10 | 123456789_ |
T11 WRITE matnr mit Nullen: 42
T11 WRITE c18 mit Nullen  : 000000000000000042
T12 i=123456  -> c5 | *3456 |
T12 i=-123456 -> c5 | *456- |
T12 i=1234 -> c5 | 1234_ |
T12 i=0 -> c1 | 0 |
T12 i=-5 -> c1 | * |
```
