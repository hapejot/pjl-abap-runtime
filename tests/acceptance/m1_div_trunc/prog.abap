REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  " Regel: Division bei i rundet kaufmännisch (Projektfestlegung, doc/sprache.md Abschnitt 7)
  x = 7 / 2.
  WRITE x.
  x = -7 / 2.
  WRITE / x.
