# FR-004: Zuweisung einer Zahl an ein Zeichenfeld

Status: umgesetzt (Stand 2026-11-28, siehe Abschnitt "Stand")
Beispiel: poc-crm (Akzeptanzfall `m16_crm`)
Stelle im Beispiel: Die erste Fassung hatte `lv_idx = mv_kidx.` mit `lv_idx TYPE c LENGTH 10`. Der Workaround
verwendet jetzt die Stringvorlage `|{ mv_kidx } von { mv_klen }|`, die Zahlen bereits richtig ausgibt.
Der Workaround-Marker `* WORKAROUND FR-004` fehlt im Beispiel und ist noch nachzutragen.

## Problem
`abapc` meldet `Number cannot be assigned to a character field (not supported yet)` (`codegen/stmt.rs:705`).

## Festlegung (Nutzer, 2026-11-28)
`abapc` bildet das Verhalten des SAP-Systems bei der Umwandlung `i` → `c` nach. Die Umwandlung ist keine
Ausgabe mit `WRITE`. Sie hat eigene Regeln (siehe unten), und es gibt keinen Übersetzungsfehler und keinen
Laufzeitabbruch.

## Beobachtetes Verhalten (SAP-Testlauf, 2026-11-28)
Report `ZFR005_BELEGTEST` (`doc/feature-requests/sap-belegtests-fr004-fr005.md`), Ausgabe vom Nutzer.
System: S/4HANA 2025, Kernel-Release 916 (Angabe des Nutzers). Das SAP-Release wurde nicht angegeben und ist nach
Angabe des Nutzers für die Regel nicht maßgeblich.

Die Regel gilt für Länge L und d Ziffern des Betrags. Sie passt zu allen Läufen T1 bis T12:
- Kein Fehler, kein Laufzeitabbruch. Überlauf wird mit `*` markiert.
- Positiv oder 0, d < L: Ziffern rechtsbündig in L−1 Stellen, letzte Stelle Leerzeichen.
- Positiv, d = L: Ziffern füllen das Feld, keine Vorzeichenstelle.
- Positiv, d > L: `*` vorne, danach die rechten L−1 Ziffern.
- Negativ, d < L: Ziffern rechtsbündig in L−1 Stellen, letzte Stelle `-`.
- Negativ, d ≥ L: `*` vorne, danach die rechten L−2 Ziffern, letzte Stelle `-`. Bei L = 1 bleibt nur `*`.

Beispiele aus dem Lauf:
- `42` in `c15` ergibt `____________42_`, `-42` ergibt `____________42-` (15 Zeichen, Unterstriche für Leerzeichen).
- `12345` in `c5` ergibt `12345`, `-12345` ergibt `*345-`.
- `123456` in `c5` ergibt `*3456`, `-123456` ergibt `*456-`, `1234` in `c5` ergibt `1234_`.
- `0` in `c1` ergibt `0`, `-5` in `c1` ergibt `*`.
- `123456789` in `c9` ergibt `123456789`, `12345678` in `c9` ergibt `12345678_`.

Abgrenzung zu `WRITE`: `WRITE 42` ist 10 Zeichen breit ohne Vorzeichenstelle, `WRITE -42` 11 Zeichen. Die
Umwandlung hat dagegen immer eine Vorzeichenstelle, sofern die Ziffern sie zulassen, und die `*`-Markierung.

Die Quelle ist der Testlauf, nicht die Keyword-Doku. Eine Belegung in der Keyword-Dokumentation steht noch aus.

## Vorschlag für abapc
- Zuweisung von `i` an `c` nach der beobachteten Regel oben, für positive und negative Zahlen und für Überlauf.
- Keine Prüfung der Feldlänge zur Übersetzungszeit, kein Laufzeitfehler.
- Die Ausgabe mit `WRITE` bleibt unverändert, siehe `abap_rt_write_int` in `abap-rt/src/lib.rs`.
- Die interne Darstellung des Zielfelds (Leerzeichen oder führende Nullen) ist nicht Teil dieses FR, siehe FR-005.

## Auswirkung
- `codegen/stmt.rs` (Zuweisung `i` → `c`, Stelle 705), neue Laufzeitfunktion in `abap-rt` für die Umwandlung,
  `lower/stmt.rs` (Typprüfung der Zuweisung).
- Akzeptanzfälle: die Läufe T1 bis T12 aus `doc/feature-requests/sap-belegtests-fr004-fr005.md` als Vorlage. Die
  Erwartungswerte stammen aus dem SAP-Lauf (S/4HANA 2025, Kernel 916) und werden in `doc/sprache.md` festgehalten.

## Stand (2026-11-28)
- Umgesetzt: Laufzeitfunktion `abap_rt_int_to_chars` in `abap-rt/src/lib.rs` (Regel in `int_to_char_field`), Aufruf
  in `codegen/stmt.rs` (`TYPE i` an `TYPE c`), Deklaration in `codegen/mod.rs`. Kein Übersetzungsfehler und kein
  Laufzeitfehler mehr für ganze Zahlen. `WRITE` ist unverändert.
- Tests: Unit-Tests im Modul `tests` von `abap-rt/src/lib.rs`. Akzeptanzfälle `m21_t01_zahl_in_c15`,
  `m21_t02_zahl_in_c10`, `m21_t03_zahl_in_c9_und_c5`, `m21_t04_write_ganze_zahl`, `m21_t09_ueberlauf_c5`,
  `m21_t10_grenzen_c9_c10`, `m21_t12_randwerte_c5_c1`. Erwartungen in `doc/sprache.md`, Abschnitt 5e.
- Nicht umgesetzt: T5 (Typ `n`), T6 bis T8 und T11 (MATNR mit ALPHA, `CALL FUNCTION`). Sie hängen an FR-005 oder haben
  im Repo keine SAP-Ausgabe. Dezimalzahl an Zeichenfeld bleibt ein Übersetzungsfehler (Regel nicht belegt).

Offen:
1. (Erledigt) Die Zeile für 42 in c15 hatte in dieser Doku einen Tippfehler mit 19 Zeichen. Richtig ist
   `____________42_` (15 Zeichen). Die Akzeptanzfälle stimmen damit überein. Die vollständige SAP-Ausgabe steht in
   `doc/feature-requests/sap-belegtests-fr004-fr005.md`, Abschnitt „SAP-Ausgabe“. Alle Erwartungen der
   Akzeptanzfälle T1 bis T12 wurden dagegen geprüft. Die `zu-pruefen.txt`-Markierungen sind entfernt.
2. Länge 0 (`TYPE c LENGTH 0`) liefert eine leere Ausgabe. Das ist eine Annahme, nicht belegt.
4. Der Workaround-Marker `* WORKAROUND FR-004` fehlt in `poc-crm` weiterhin. Das Beispiel wurde nicht geändert.
