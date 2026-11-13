REPORT ztest.
DATA x TYPE i.
DATA s TYPE i.
START-OF-SELECTION.
  x = 0.
  s = 0.
  DO 5 TIMES.
    x = x + 1.
    CHECK x < 3.
    s = s + 1.
  ENDDO.
  WRITE s.
