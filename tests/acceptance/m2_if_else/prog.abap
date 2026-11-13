REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 1.
  IF x = 5.
    WRITE 'a'.
  ELSE.
    WRITE 'b'.
  ENDIF.
