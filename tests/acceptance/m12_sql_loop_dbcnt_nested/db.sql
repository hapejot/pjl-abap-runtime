CREATE TABLE zflug (
  carrid TEXT NOT NULL,
  connid TEXT NOT NULL,
  seatsmax INTEGER NOT NULL,
  PRIMARY KEY (carrid, connid)
);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('LH', '0400', 180);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('LH', '0401', 200);
