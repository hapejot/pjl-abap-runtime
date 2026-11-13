REPORT ztest.
DATA c TYPE c LENGTH 6.
DATA r TYPE c LENGTH 2.
START-OF-SELECTION.
  c = 'abcdef'.
  r = c+1(2).
  WRITE r.
