REPORT ztest.
DATA lv_i TYPE i.
DATA lv_c9 TYPE c LENGTH 9.
DATA lv_c5 TYPE c LENGTH 5.
START-OF-SELECTION.
  " T3 i=42 -> c9
  lv_i = 42.
  lv_c9 = lv_i.
  WRITE / lv_c9.
  " T3 i=42 -> c5 (kürzer als die Ausgabe)
  lv_c5 = lv_i.
  WRITE / lv_c5.
