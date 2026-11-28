REPORT ztest.
DATA lv_i TYPE i.
START-OF-SELECTION.
  " T4 WRITE 42 (Vergleich, Ausgabe mit WRITE bleibt unverändert)
  lv_i = 42.
  WRITE / lv_i.
  " T4 WRITE -42
  lv_i = -42.
  WRITE / lv_i.
