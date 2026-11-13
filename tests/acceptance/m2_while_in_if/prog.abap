REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 0.
  IF 1 = 1.
    WHILE x < 2.
      x = x + 1.
    ENDWHILE.
  ENDIF.
  WRITE x.
