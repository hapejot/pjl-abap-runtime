REPORT ztest.
DATA c TYPE c LENGTH 4.
START-OF-SELECTION.
  " Regel: bei c-Vergleich werden nachgestellte Leerzeichen ignoriert
  c = 'ab'.
  IF c = 'ab'.
    WRITE 'ja'.
  ENDIF.
