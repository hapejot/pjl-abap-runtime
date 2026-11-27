REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  " Regel: DIV bei i schneidet ab, im Gegensatz zu /
  x = 7 DIV 2.
  WRITE x.
  x = -7 DIV 2.
  WRITE / x.
