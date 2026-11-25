REPORT zt.
DATA lv TYPE i.
DATA x TYPE i.
FIELD-SYMBOLS <dyn>.
START-OF-SELECTION.
  lv = 9.
  ASSIGN lv TO <dyn>.
  x = <dyn> + 1.
  WRITE x.
