REPORT ztest.
DATA s TYPE i.
START-OF-SELECTION.
  s = 0.
  DO 4 TIMES.
    s = s + 1.
  ENDDO.
  WRITE s.
