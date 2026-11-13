REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  x = 0.
  WHILE x < 5.
    x = x + 1.
  ENDWHILE.
  WRITE x.
