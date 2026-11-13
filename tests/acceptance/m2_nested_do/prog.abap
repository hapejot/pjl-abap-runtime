REPORT ztest.
DATA s TYPE i.
START-OF-SELECTION.
  s = 0.
  DO 2 TIMES.
    DO 3 TIMES.
      s = s + 1.
    ENDDO.
  ENDDO.
  WRITE s.
