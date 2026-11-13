REPORT ztest.
DATA: BEGIN OF s,
        a TYPE i,
      END OF s.
START-OF-SELECTION.
  s-a = 5.
  WRITE s-a.
