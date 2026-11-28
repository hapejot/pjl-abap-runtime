REPORT ztkv_plain.
DATA: lv_plain TYPE ztkv_plain,
      lv_nr    TYPE ztkv_nr.
START-OF-SELECTION.
  lv_plain = '000000000000000042'.
  WRITE / lv_plain.
  lv_nr = '0000000000000042'.
  WRITE / lv_nr.
