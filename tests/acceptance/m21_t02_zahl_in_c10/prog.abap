REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c10 TYPE c LENGTH 10.
START-OF-SELECTION.
  " T2 i=42 -> c10
  lv_i = 42.
  lv_c10 = lv_i.
  WRITE / lv_c10.
  " T2 i=-42 -> c10
  lv_i = -42.
  lv_c10 = lv_i.
  WRITE / lv_c10.
