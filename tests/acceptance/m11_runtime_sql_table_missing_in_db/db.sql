CREATE TABLE zflug (
  carrid TEXT NOT NULL,
  connid TEXT NOT NULL,
  seatsmax INTEGER NOT NULL
);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('LH', '0400', 100);

-- zother ist konfiguriert, fehlt aber in der Datenbank
CREATE TABLE zother (carrid TEXT NOT NULL);
DROP TABLE zother;
