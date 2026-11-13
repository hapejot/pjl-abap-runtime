REPORT ztest.
DATA c TYPE c LENGTH 5.
START-OF-SELECTION.
  c = 'aaaaa'.
  c+1(2) = 'XY'.
  WRITE c.
