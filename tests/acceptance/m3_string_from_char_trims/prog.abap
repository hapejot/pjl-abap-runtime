REPORT ztest.
DATA c TYPE c LENGTH 5.
DATA s TYPE string.
START-OF-SELECTION.
  " Regel: beim Übergang Zeichenfeld -> string werden nachgestellte Leerzeichen entfernt
  c = 'ab'.
  s = c.
  WRITE s.
