REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c15 TYPE c LENGTH 15.
START-OF-SELECTION.
  " T1 i=42 -> c15
  lv_i = 42.
  lv_c15 = lv_i.
  WRITE / lv_c15.
  " T1 i=-42 -> c15
  lv_i = -42.
  lv_c15 = lv_i.
  WRITE / lv_c15.
