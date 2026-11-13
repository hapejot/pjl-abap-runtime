REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 0.
  DO.
    x = x + 1.
    IF x = 3.
      EXIT.
    ENDIF.
  ENDDO.
  WRITE x.
