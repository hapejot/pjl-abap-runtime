---
name: abap-architektur
description: Programmierregeln für die ABAP-Beispiele (poc-crm, poc-finance) in pjl-abap-runtime. Schichten (Dynpro, View-Model, Service, Validierung, DAO), Benennung, Fehlerbehandlung und Testfälle. Laden, bevor ABAP-Beispielcode geschrieben oder geändert wird.
---

# ABAP-Architektur für die PoC-Programme

Ziel: Der Code soll auf einem SAP-System ohne Änderung laufen, so weit der Compiler das abdeckt.
Abweichungen vom ABAP-Standard, die wegen fehlender Compiler-Funktionen nötig sind, werden im Code
markiert (Abschnitt 8).

## 1. Schichten

Von oben nach unten. Eine Schicht ruft nur die Schicht direkt darunter auf.

| Schicht              | Inhalt                                                                           | Darf                                                                     |
| -------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Dynpro (`prog.dynp`) | Layout, Felder, Flow Logic                                                       | Nur PBO- und PAI-Module, die an das View-Model delegieren                |
| View-Model           | Zustand einer Anzeige: Felder, Funktionscodes, Meldungen, Gitter, Blättern       | Dynpro-Felder halten und Ereignisse an den Service weitergeben. Kein SQL |
| Service              | Anwendungsfälle (anlegen, ändern, löschen, berichten), Transaktionsgrenzen (LUW) | Validierung und DAOs aufrufen. Kein Dynpro                               |
| Validierung          | Reine Prüfungen der Eingaben, lösen Ausnahmen aus                                | Nur Daten prüfen. Keine Datenbank, kein Dynpro                           |
| DAO                  | Ein DAO je Tabelle, kapselt Open SQL                                             | Nur SQL. Keine Geschäftsregeln                                           |

Begründung: Dynpro und Datenbank sind die beiden Stellen, die sich am häufigsten ändern.
Logik darin zu verteilen macht Tests schwer.

## 2. Dynpro und View-Model

- PBO-Module laden Werte aus dem View-Model in die Dynpro-Felder. PAI-Module übergeben die Eingaben an das
  View-Model. Keine Validierung im PAI-Modul selbst.
- Dynpro-Felder sind globale Variablen des Programms (Namen wie in `prog.dynp`). Das View-Model kann sie nicht
  direkt binden. Deshalb kopieren die Module in beide Richtungen: PBO kopiert vom View-Model in die globalen
  Felder, PAI zurück. Dafür gibt es je eine Form (`to_dynpro`, `from_dynpro`), nicht Kopien in jedem Modul.
- Module rufen keine Methode mit Ergebnis in einer `IF`-Bedingung auf. Das Ergebnis geht zuerst in eine
  Variable (Fehler B-2 in `doc/feature-requests/README.md`).
- Funktionscodes (`sy-ucomm`) werden im View-Model in eine Aktion übersetzt, z. B. `save`, `delete`, `next`.
- Meldungen aus Ausnahmen gehen ins View-Model (ein Feld `mv_message`), nicht direkt ins Dynpro.
- Gitter-Anzeige: zunächst die einfachste umsetzbare Variante. Die Darstellung wird schrittweise verbessert.
  Verbesserungen entstehen über Iterationen und Feature-Requests, nicht über eine Vorab-Festlegung.

## 3. Service

- Ein Service je Fachbereich (z. B. `lcl_crm_service`). Methoden sind Anwendungsfälle, keine Tabellenzugriffe.
- Eine Methode = eine LUW. Schreibende Methoden rufen `COMMIT WORK` nur am Ende auf, nach allen Prüfungen.
  Bei Fehlern wird nichts geschrieben: `ROLLBACK WORK` oder vorher prüfen.
- Ein Löschen über mehrere Tabellen (z. B. Kunde mit Kontakten) läuft in einer LUW.

## 4. Validierung

- Reine Methoden ohne Zustand. Eingaben rein, Ausnahme bei Fehler, sonst nichts.
- Eine Ausnahmeklasse für den Fachbereich, z. B. `lcx_crm`, mit Textattribut.
- Der Text ist in der Ausnahme, nicht im Aufrufer. Aufrufer übersetzen nicht.

## 5. DAO

- Ein DAO je Tabelle: `lcl_dao_kunde`, `lcl_dao_kontakt`, `lcl_dao_chance`.
- Methoden: `read`, `read_all_for`, `insert`, `update`, `delete`. Parameter und Ergebnisse als `TYPES`-Zeilen.
- SQL steht nur im DAO. Außerhalb keine `SELECT`-, `INSERT`-, `UPDATE`- oder `DELETE`-Anweisung.
- **Umsetzungsstand (poc-crm):** Die DAOs sind konkrete Klassen, die direkt erzeugt und per Konstruktor in
  den Service gegeben werden (`WORKAROUND FR-001`). Sie liefern nur Einzelsätze, weil Tabellen als Parameter
  nicht unterstützt sind (`WORKAROUND FR-002`). Listen entstehen im Service durch Schleifen. Das ist das
  Zwischenziel, bis die Feature-Requests umgesetzt sind.
- DAOs sind echte Instanzen, damit sie in Unit-Tests durch Mocks ersetzt werden können. Dreiteilung je Tabelle:
  - Interface `lif_dao_kunde` mit den Methoden (Vertrag). Services und View-Models kennen nur das Interface.
  - Implementierung `lcl_dao_kunde` mit Open SQL. Nur sie enthält SQL für diese Tabelle.
  - Factory `lcl_dao_factory` mit Methoden wie `get_kunde_dao( ) RETURNING ro_dao TYPE REF TO lif_dao_kunde`.
    Das Hauptprogramm holt die DAOs über die Factory und übergibt sie an die Services.
- Services bekommen ihre DAOs per Konstruktor (Parameter `io_..._dao TYPE REF TO lif_dao_...`), nicht über
  eine globale Variable. So lässt sich im Test ein Mock einsetzen.
- Mocks: Testklassen, die dasselbe Interface implementieren und Daten in internen Tabellen halten. Sie
  liegen beim Test, nicht im Produktionscode.
- Ob der Compiler Interfaces mit `REF TO`-Parametern und Konstruktor-Injektion vollständig kann, wird
  am Beispiel geprüft. Lücken werden als Feature-Request erfasst (Abschnitt 8).

## 6. Benennung

- Lokale Klassen: `lcl_` (Logik), `lcx_` (Ausnahmen), `lif_` (Interfaces).
- Tabellen-Typen: `TYPES` mit dem Namen der DB-Tabelle, z. B. `zkunde` (wie in `poc-crm`).
- Attribute im View-Model: `mv_` (Wert), `mt_` (Tabelle), `mo_` (Objekt). Parameter: `iv_`, `it_`, `io_`,
  `ev_`, `et_`, `ro_` für Rückgabe.
- Namen und Meldungstexte in Englisch, wenn sie in den Compiler-Meldungen auftauchen; Kommentare auf Deutsch.

## 7. Fehlerbehandlung

- Erwartbare Fehler sind Ausnahmen (`RAISE EXCEPTION`), mit `TRY`/`CATCH` nur an Schichtgrenzen
  (View-Model, Service).
- `sy-subrc` nur direkt nach einer Anweisung prüfen, die es setzt. Keine Ketten.
- Keine stillen Fehler: jede Ausnahme wird behandelt oder weitergegeben.

## 8. Abweichungen vom Standard

Wenn eine Sprachfunktion im Compiler fehlt, aber im echten ABAP vorhanden ist:

1. Den Code so schreiben, wie er im echten ABAP stünde.
2. Die Stelle mit `* WORKAROUND FR-NNN: <kurz>` markieren.
3. Einen Feature-Request nach `doc/feature-requests/` anlegen.

Den Compiler selbst nicht ändern, um das Beispiel zu retten.

## 9. Tests

- Jedes Beispiel hat einen Akzeptanzlauf: Eingabeskript (`input.keys`) und erwartete Ausgabe.
- Erwartungen kommen aus dem ABAP-Standard. Eigene Vorgaben werden im Sprachdokument festgehalten.
- Nicht belegte Erwartungen stehen in `zu-pruefen.txt`.
- Beispieldaten sind frei erfunden. Keine Kunden- oder SAP-Daten.
- Unit-Tests mit Mocks sind das Ziel der DAO-Struktur (Abschnitt 5). ABAP Unit (`FOR TESTING`) kennt `abapc`
  bisher nicht. Bis dahin prüft der Akzeptanzlauf das Verhalten, und die Mocks liegen schon bereit.
