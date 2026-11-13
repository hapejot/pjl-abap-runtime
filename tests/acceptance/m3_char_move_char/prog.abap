REPORT ztest.
DATA a TYPE c LENGTH 4.
DATA b TYPE c LENGTH 2.
START-OF-SELECTION.
  a = 'xy'.
  b = a.
  WRITE b.
