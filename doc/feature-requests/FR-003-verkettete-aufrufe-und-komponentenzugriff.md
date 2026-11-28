# FR-003: Verkettete Methodenaufrufe und Komponentenzugriff auf Objektattribute

Status: umgesetzt (Einschränkungen siehe unten; Stand 2026-11-28)
Beispiel: poc-crm (Akzeptanzfall `m16_crm`)
Stelle im Beispiel: Die erste Fassung hatte `mo_service->mo_kunde->select_one( ... )`. Das wurde durch
eine Service-Methode ersetzt. Die Strukturkomponente `go_view->ms_maske-kunnr` von außen wurde durch
`get_maske` und `set_maske` ersetzt. Der Workaround-Marker `* WORKAROUND FR-003` fehlt im Beispiel und ist
noch nachzutragen.

## Problem
Zwei Zugriffsformen fehlen:

1. Verkettete Aufrufe: `a->attr->methode( )` und `a->methode( )->methode2( )`.
   `abapc` meldet `Chained method calls are not translated yet`.
2. Komponente eines Strukturattributs von außen: `obj->struktur-komponente`. `abapc` meldet
   `Attribute 'ms_maske-kunnr' unknown`. Innerhalb der Klasse funktioniert der Zugriff.

Der Workaround verlagert den Zugriff in Methoden. Das ist sauber, verdeckt aber, dass eine Schicht auf die
Interna der anderen zugreifen wollte.

## Echtes ABAP
Beide Formen sind Standard. Belegt durch die ABAP-Schlüsselwortdokumentation (Stichworte „Method calls“,
„Attributes“, „Components“). Die Fundstelle ist noch nicht im Repo festgehalten.

## Vorschlag für abapc
- Aufrufketten: Ergebnis eines Aufrufs als Basis des nächsten Aufrufs oder Zugriffs verwenden.
- Komponentenzugriff auf öffentliche Strukturattribute: `obj->attr-komp` lesend und schreibend.
- Abgrenzung: Zugriffe auf geschützte Attribute bleiben verboten (Sichtbarkeit wird geprüft).

## Auswirkung
- `ast.rs` (`Expr::Chain`, Zeile 30, vorhanden), `codegen/expr.rs` (Abweisung „Chained method calls are not
  translated yet“, Zeile 304), `codegen/data.rs` (Abweisung „Attribute '...' unknown“, Zeile 143),
  `lower/decl.rs` (Attribute).
- Akzeptanzfälle: Kette aus zwei Aufrufen, Lesen und Schreiben einer Strukturkomponente über ein Objekt.

## Umsetzung (Stand 2026-11-28)

- Aufrufketten: `Expr::Chain` wird in `codegen/expr.rs` ausgewertet. Jeder Schritt arbeitet auf dem
  Zwischenwert des vorigen. Aufruf auf Objektzeiger über `emit_call_on` in `codegen/routines.rs`.
- Komponentenzugriff: `codegen/data.rs` (`field_of`) löst `attr-komp` auf, auch verschachtelt. Lesen und
  Schreiben gehen über denselben Pfad.
- Sichtbarkeit: Der Compiler prüfte sie vorher nicht (weder Attribute noch Methoden). Jetzt gilt sie für
  Attribute (`lower/decl.rs`, `codegen/data.rs` `check_attr_access`). Regel und Festlegungen: `doc/sprache.md`, Abschnitt 5c.
- Akzeptanzfälle: `m19_chain_two_calls`, `m19_chain_attribute`, `m19_struct_component_read`,
  `m19_struct_component_write`, `m19_protected_in_subclass`, `m19_error_protected_attribute_from_outside`,
  `m19_error_private_write`.

**Offen:**

- Zuweisung an eine verkettete Stelle (`a->b->c = x`) ist nicht umgesetzt (Übersetzungsfehler).
- Sichtbarkeit für Methodenaufrufe und Klassenattribute (`cls=>attr`) wird nicht geprüft.
- Der Workaround-Marker `* WORKAROUND FR-003` fehlt im Beispiel poc-crm (nicht Teil dieses Auftrags; das Beispiel wurde nicht geändert).
- Der Fall „Unterklasse greift über Referenz des Oberklassentyps auf PROTECTED zu“ ist nicht eindeutig belegt; die strenge Festlegung gilt (siehe `doc/sprache.md`, 5c, Festlegung 3).
