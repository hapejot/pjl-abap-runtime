# FR-001: Interfaces als Referenztyp und Factory für DAOs

Status: umgesetzt (Stufe 1, Compiler). Stufe 2 umgesetzt (Factory im Beispiel, poc-crm umgestellt).
Beispiel: poc-crm (Akzeptanzfall `m16_crm`)
Stelle im Beispiel: `poc-crm/prog.abap`, Interfaces `lif_dao_*`, Implementierungen `lcl_dao_*_sql`, Factory `lcl_dao_factory`. Die Workaround-Marker sind entfernt.

## Problem
Die DAOs sollen über ein Interface angesprochen werden (`lif_dao_kunde`), damit Services im Unit-Test
gegen Mocks laufen. Dafür fehlen in `abapc` drei Bausteine:

- Referenz auf ein Interface: `DATA mo TYPE REF TO lif_dao_kunde.`
- Erzeugen über eine konkrete Klasse mit Typangabe: `CREATE OBJECT mo TYPE lcl_dao_kunde.`
- Methodenaufruf über die Interface-Referenz: `mo->select_one( ... )`, die dynamisch zur Implementierung auflöst.

Der Workaround nutzt konkrete Klassen. Die Services bekommen sie über den Konstruktor. Mocks lassen sich so
nicht einsetzen.

## Echtes ABAP
Interfaces mit `INTERFACES` in der Klassendefinition und Aufruf über `~` funktionieren in `abapc` bereits
(`m5e_interface_method`, `m5i_interface_attribute`). Nicht unterstützt sind die Referenz auf ein Interface
und die dynamische Auflösung über die Referenz. Belegt durch die ABAP-Schlüsselwortdokumentation (Stichworte
„Interfaces“, „Reference types“, „CREATE OBJECT ... TYPE“). Die genaue Fundstelle ist noch nicht im Repo
festgehalten.

## Entscheidung
Festgelegt vom Nutzer (2026-11-28): **eine Methodentabelle pro Klasse**. Das ist die traditionelle Form und schnell.
- Zur Übersetzungszeit bekommt jede Klasse eine feste Tabelle mit den Adressen ihrer Methoden.
- Jedes implementierte Interface erhält darin einen festen Abschnitt (Offset). Die Position jeder Methode ist damit zur Übersetzungszeit bekannt.
- Aufruf über eine Interface-Referenz: Die Referenz zeigt auf das Objekt, der Aufruf liest den Eintrag an `Offset(Interface) + Index(Methode)`. Kein Namensvergleich zur Laufzeit.
- Das Objekt trägt einen Zeiger auf die Tabelle seiner Klasse.
- Noch offen für die Umsetzung: Behandlung der Oberklasse (Einträge übernehmen und überschreiben).

## Vorschlag für abapc
- `TYPE REF TO <interface>` in Attributen, Parametern und lokalen Variablen.
- `CREATE OBJECT <ref> TYPE <klasse>` mit Prüfung, dass die Klasse das Interface implementiert.
- Aufruf von Interface-Methoden über eine Interface-Referenz über die Methodentabelle (siehe Entscheidung).
- Stufen: 1 Referenz, `CREATE OBJECT ... TYPE` und Aufruf über die Tabelle. 2 Factory als ABAP-Code im Beispiel (kein Compiler-Thema).
- Abgrenzung: keine Vererbung von Interfaces untereinander in der ersten Stufe.

## Auswirkung
- `lower/decl.rs` (Typen, Referenzen auf Klassen über `Ty::Ref` heute nur für Klassen-Index).
- `lower/stmt.rs` (`CREATE OBJECT ... TYPE`), `codegen/layout.rs` (Methodentabelle pro Klasse, Offsets je Interface), `codegen/` (Aufruf über Index).
- Neue Akzeptanzfälle: Interface-Referenz zeigt auf unterschiedliche Implementierungen; Mock im Test.
- Nach Umsetzung: Workarounds im Beispiel entfernen, Factory `lcl_dao_factory` einführen.

## Stand (Stufe 1, umgesetzt)
- `TYPE REF TO <interface>` (Attribute, Parameter, Rückgabe, lokale Variablen), `CREATE OBJECT ... TYPE`
  mit Prüfung der Implementierung, Aufruf über die Methodentabelle, Upcast Klasse nach Interface.
- Umsetzung: `lower/decl.rs`, `lower/stmt.rs`, `lower/ir.rs` (`Ty::Iface`, `InterfaceDef`), `codegen/iface.rs`
  (Tabellen, Objektkopf, Aufruf, `CREATE`), `codegen/routines.rs`, `codegen/stmt.rs`, `codegen/expr.rs`.
- Akzeptanzfälle: `m20_iface_two_impls`, `m20_iface_mock_replaces_impl`, `m20_iface_attr_and_param`,
  `m20_iface_factory_return`, `m20_iface_inherited_table`, `m20_error_create_type_not_implemented`,
  `m20_error_create_iface_without_type`. Erwartungswerte und Festlegungen: `doc/sprache.md`, Abschnitt 5d.
- Festlegungen (Liste in `doc/sprache.md`, 5d): Offsets je Interface programmweit, Oberklassenregel
  (Kopie der Tabelle, eigene Implementierungen überschreiben), Objektkopf mit 8 Byte, Signaturprüfung.

## Offen
- Stufe 2 (Factory `lcl_dao_factory` im Beispiel, Entfernen der Workarounds) ist umgesetzt. poc-crm ist
  auf Interfaces und Factory umgestellt, die Workaround-Marker sind entfernt.
- Überschreiben von Interface-Methoden in Unterklassen (`REDEFINITION`): ohne Akzeptanzfall, Syntax nicht geprüft.
- CAST, statische Interface-Methoden und Interface-Attribute über eine Referenz.
- Die Fehlerklasse der Implementierungspflicht (Syntax oder Laufzeit) ist in ABAP nicht belegt.
