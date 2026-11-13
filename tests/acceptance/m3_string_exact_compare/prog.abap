REPORT ztest.
DATA s TYPE string.
DATA t TYPE string.
START-OF-SELECTION.
  " Regel: Strings sind ungleich, wenn Längen verschieden sind (keine Auffüllung)
  s = 'ab'.
  t = 'ab' && ' '.
  IF s = t.
    WRITE 'gleich'.
  ENDIF.
  WRITE 'ende'.
