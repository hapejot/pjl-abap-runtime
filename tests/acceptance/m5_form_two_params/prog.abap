REPORT ztest.
DATA a TYPE i.
DATA b TYPE i.
FORM swap CHANGING x TYPE i y TYPE i.
  DATA tmp TYPE i.
  tmp = x.
  x = y.
  y = tmp.
ENDFORM.
START-OF-SELECTION.
  a = 1.
  b = 2.
  PERFORM swap CHANGING a b.
  WRITE a.
  WRITE / b.
