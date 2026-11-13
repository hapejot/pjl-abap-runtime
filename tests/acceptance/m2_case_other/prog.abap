REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 7.
  CASE x.
    WHEN 1.
      WRITE 'eins'.
    WHEN OTHERS.
      WRITE 'rest'.
  ENDCASE.
