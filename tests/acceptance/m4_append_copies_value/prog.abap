REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
DATA x TYPE i.
START-OF-SELECTION.
  " Regel: APPEND kopiert den Wert; spätere Änderung von wa wirkt nicht auf die Zeile
  wa = 5.
  APPEND wa TO t.
  wa = 9.
  LOOP AT t INTO x.
    WRITE x.
  ENDLOOP.
