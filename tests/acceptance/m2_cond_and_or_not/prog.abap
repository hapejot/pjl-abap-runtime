REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 2.
  IF x > 1 AND NOT x = 3.
    WRITE 'a'.
  ENDIF.
  IF x = 9 OR x <> 2.
    WRITE 'b'.
  ENDIF.
  IF x <= 2 AND x >= 2.
    WRITE 'c'.
  ENDIF.
