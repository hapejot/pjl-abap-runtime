REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c5 TYPE c LENGTH 5.
DATA lv_c1 TYPE c LENGTH 1.
START-OF-SELECTION.
  " T12 i=123456 -> c5
  lv_i = 123456.
  lv_c5 = lv_i.
  WRITE / lv_c5.
  " T12 i=-123456 -> c5
  lv_i = -123456.
  lv_c5 = lv_i.
  WRITE / lv_c5.
  " T12 i=1234 -> c5
  lv_i = 1234.
  lv_c5 = lv_i.
  WRITE / lv_c5.
  " T12 i=0 -> c1
  lv_i = 0.
  lv_c1 = lv_i.
  WRITE / lv_c1.
  " T12 i=-5 -> c1
  lv_i = -5.
  lv_c1 = lv_i.
  WRITE / lv_c1.
