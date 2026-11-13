REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 4.
  IF x = 5.
    WRITE 'ja'.
  ENDIF.
  WRITE 'ende'.
