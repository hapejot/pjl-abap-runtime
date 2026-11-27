# Dokumentation abapc

Stand: 2026-11-27. Diese Dokumentation beschreibt den Ist-Zustand des Compilers `abapc` und seiner
Laufzeit `abap-rt`. Sie ist kein Pflichtenheft.

## Inhalt

- [sprache.md](sprache.md): Welche ABAP-Sprachmittel unterstützt sind, wie ihre Semantik festgelegt ist
  und wo wir vom ABAP-Standard abweichen oder noch nicht sicher sind.
- [uebersetzung.md](uebersetzung.md): Wie ein Programm übersetzt wird (Aufruf, Ablauf, Fehler),
  wie Akzeptanzfälle angelegt werden und wie eine neue Anweisung hinzukommt.
- [llvm-installation.md](llvm-installation.md): Voraussetzungen für LLVM 21 und die Build-Umgebung
  unter Windows, Linux und macOS.

## Kennzeichnung der Aussagen

Die Dokumentation unterscheidet drei Arten von Aussagen:

- **belegt**: durch einen Akzeptanzfall unter `tests/acceptance/` festgelegt. Der Name des Falls steht dabei.
- **Entscheidung**: bewusste Umsetzungsentscheidung des Projekts, die nicht aus dem ABAP-Standard
  stammt. Sie ist nicht automatisch falsch, muss aber als Abweichung gelten.
- **zu prüfen**: Verhalten, das gegen den ABAP-Standard belegt werden muss. Die Erwartung im Test
  kann dabei falsch sein.

Die Regel dafür steht in `CLAUDE.md`: Erwartungswerte kommen aus dem ABAP-Standard, oder aus Vorgaben in sprache.md.

Fragliche Akzeptanzfälle sind im Fallverzeichnis mit einer Datei `zu-pruefen.txt` markiert (Grund und
Verweis auf [sprache.md](sprache.md)). Der Runner gibt sie vor dem Testlauf aus, die Tests selbst laufen
unverändert. Aktuell markiert: `m1_div_trunc`, `m7_write_chain_slash`, `m1_write_int`.
