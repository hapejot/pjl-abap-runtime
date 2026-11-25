REPORT zt.
DATA lv TYPE i.
FIELD-SYMBOLS <dyn>.
START-OF-SELECTION.
  ASSIGN lv TO <dyn>.
  <dyn> = 5.
  WRITE lv.
