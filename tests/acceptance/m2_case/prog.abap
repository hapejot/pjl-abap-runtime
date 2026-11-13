REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 3.
  CASE x.
    WHEN 1.
      WRITE 'eins'.
    WHEN 2 OR 3.
      WRITE 'zwei-drei'.
    WHEN OTHERS.
      WRITE 'rest'.
  ENDCASE.
