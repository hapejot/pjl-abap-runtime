REPORT zt.
DATA: BEGIN OF s,
  f TYPE c LENGTH 2,
 END OF s.
DATA lv TYPE i.
FIELD-SYMBOLS <dyn>.
START-OF-SELECTION.
  lv = 1.
  ASSIGN lv TO <dyn>.
  s-f = 'ab'.
  ASSIGN s-f TO <dyn>.
  WRITE <dyn>.
