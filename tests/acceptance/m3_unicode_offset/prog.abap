REPORT ztest.
DATA c TYPE c LENGTH 4.
DATA r TYPE c LENGTH 2.
START-OF-SELECTION.
  c = 'äöüß'.
  r = c+1(2).
  WRITE r.
