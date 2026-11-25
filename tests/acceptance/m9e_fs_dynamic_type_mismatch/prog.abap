REPORT zt.
DATA lv TYPE i.
FIELD-SYMBOLS <dyn>.
FIELD-SYMBOLS <fs> TYPE c.
START-OF-SELECTION.
  lv = 3.
  ASSIGN lv TO <dyn>.
  ASSIGN <dyn> TO <fs>.
  WRITE <fs>.
