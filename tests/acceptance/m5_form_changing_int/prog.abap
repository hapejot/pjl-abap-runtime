REPORT ztest.
DATA x TYPE i.
FORM add_one CHANGING v TYPE i.
  v = v + 1.
ENDFORM.
START-OF-SELECTION.
  x = 41.
  PERFORM add_one CHANGING x.
  WRITE x.
