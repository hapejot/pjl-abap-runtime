REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c9 TYPE c LENGTH 9.
DATA lv_c10 TYPE c LENGTH 10.
START-OF-SELECTION.
  " T10 i=12345678 -> c9
  lv_i = 12345678.
  lv_c9 = lv_i.
  WRITE / lv_c9.
  " T10 i=123456789 -> c9
  lv_i = 123456789.
  lv_c9 = lv_i.
  WRITE / lv_c9.
  " T10 i=123456789 -> c10
  lv_c10 = lv_i.
  WRITE / lv_c10.
