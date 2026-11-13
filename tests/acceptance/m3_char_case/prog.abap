REPORT ztest.
DATA c TYPE c LENGTH 2.
START-OF-SELECTION.
  c = 'b'.
  CASE c.
    WHEN 'a'.
      WRITE 'A'.
    WHEN 'b'.
      WRITE 'B'.
  ENDCASE.
