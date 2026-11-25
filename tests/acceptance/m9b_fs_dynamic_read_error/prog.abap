REPORT zt.
DATA lv TYPE i.
FIELD-SYMBOLS <dyn>.
START-OF-SELECTION.
  lv = 3.
  ASSIGN lv TO <dyn>.
  WRITE <dyn>.
