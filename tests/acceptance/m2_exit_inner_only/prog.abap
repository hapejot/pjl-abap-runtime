REPORT ztest.
DATA s TYPE i.
START-OF-SELECTION.
  s = 0.
  DO 2 TIMES.
    DO.
      EXIT.
    ENDDO.
    s = s + 1.
  ENDDO.
  WRITE s.
