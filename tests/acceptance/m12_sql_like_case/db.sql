CREATE TABLE zflug (
  carrid TEXT NOT NULL,
  connid TEXT NOT NULL,
  seatsmax INTEGER NOT NULL,
  PRIMARY KEY (carrid, connid)
);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('LH', '0400', 100);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('lh', '0401', 200);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('LX', '0402', 300);
INSERT INTO zflug (carrid, connid, seatsmax) VALUES ('AA', '0017', 50);
