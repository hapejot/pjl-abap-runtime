REPORT zt.
DATA lv TYPE i.
FIELD-SYMBOLS <fs> TYPE i.
START-OF-SELECTION.
  lv = 7.
  ASSIGN lv TO <fs>.
  <fs> = 42.
  WRITE lv.
