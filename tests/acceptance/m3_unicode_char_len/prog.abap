REPORT ztest.
DATA c TYPE c LENGTH 3.
START-OF-SELECTION.
  " Regel: Literale und Felder zählen in Zeichen, nicht in Bytes
  c = 'äöü'.
  WRITE c.
