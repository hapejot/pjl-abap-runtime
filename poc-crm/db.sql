-- CRM-Demo: Tabellen mit Primärschlüssel und Startdaten (frei erfundene Beispieldaten).
CREATE TABLE zkunde (
  kunnr INTEGER NOT NULL,
  name TEXT NOT NULL,
  ort TEXT NOT NULL,
  branche TEXT NOT NULL,
  PRIMARY KEY (kunnr)
);
CREATE TABLE zkontakt (
  knr INTEGER NOT NULL,
  kunnr INTEGER NOT NULL,
  vorname TEXT NOT NULL,
  nachname TEXT NOT NULL,
  email TEXT NOT NULL,
  telefon TEXT NOT NULL,
  PRIMARY KEY (knr)
);
CREATE TABLE zchance (
  cnr INTEGER NOT NULL,
  kunnr INTEGER NOT NULL,
  titel TEXT NOT NULL,
  wert INTEGER NOT NULL,
  phase TEXT NOT NULL,
  abschluss TEXT NOT NULL,
  PRIMARY KEY (cnr)
);
INSERT INTO zkunde (kunnr, name, ort, branche) VALUES (1, 'Nordlicht Handel', 'Hamburg', 'Handel');
INSERT INTO zkunde (kunnr, name, ort, branche) VALUES (2, 'Rheinbau AG', 'Koeln', 'Bau');
INSERT INTO zkunde (kunnr, name, ort, branche) VALUES (3, 'Alpenwerk KG', 'Muenchen', 'Industrie');
INSERT INTO zkontakt (knr, kunnr, vorname, nachname, email, telefon) VALUES (1, 1, 'Anna', 'Schmidt', 'anna.schmidt@nordlicht.example', '040 1234500');
INSERT INTO zkontakt (knr, kunnr, vorname, nachname, email, telefon) VALUES (2, 1, 'Jens', 'Meier', 'jens.meier@nordlicht.example', '040 1234501');
INSERT INTO zkontakt (knr, kunnr, vorname, nachname, email, telefon) VALUES (3, 2, 'Lena', 'Vogel', 'lena.vogel@rheinbau.example', '0221 987600');
INSERT INTO zkontakt (knr, kunnr, vorname, nachname, email, telefon) VALUES (4, 3, 'Tobias', 'Krause', 'tobias.krause@alpenwerk.example', '089 555000');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (1, 1, 'Lagerhalle Ausbau', 50000, 'offen', '');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (2, 1, 'Software-Wartung', 12000, 'gewonnen', '20260315');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (3, 2, 'Maschinenpark', 80000, 'offen', '');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (4, 2, 'Gabelstapler', 25000, 'verloren', '20260201');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (5, 3, 'Werkserweiterung', 150000, 'gewonnen', '20260910');
INSERT INTO zchance (cnr, kunnr, titel, wert, phase, abschluss) VALUES (6, 3, 'Schulungen', 8000, 'offen', '');
