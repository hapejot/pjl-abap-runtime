REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 2.
  IF x = 1.
    WRITE 'eins'.
  ELSEIF x = 2.
    WRITE 'zwei'.
  ELSEIF x = 3.
    WRITE 'drei'.
  ELSE.
    WRITE 'rest'.
  ENDIF.
