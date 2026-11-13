REPORT ztest.
DATA c TYPE c LENGTH 4.
START-OF-SELECTION.
  c = 'abcd'.
  c+1(3) = 'X'.
  WRITE c.
