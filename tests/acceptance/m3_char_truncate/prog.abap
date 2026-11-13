REPORT ztest.
DATA c TYPE c LENGTH 3.
START-OF-SELECTION.
  " Regel: Zuweisung kürzt auf die Feldlänge
  c = 'abcdef'.
  WRITE c.
