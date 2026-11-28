REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c5 TYPE c LENGTH 5.
START-OF-SELECTION.
  " T9 i=12345 -> c5
  lv_i = 12345.
  lv_c5 = lv_i.
  WRITE / lv_c5.
  " T9 i=-12345 -> c5
  lv_i = -12345.
  lv_c5 = lv_i.
  WRITE / lv_c5.
