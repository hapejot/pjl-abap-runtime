REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
DATA n TYPE i.
START-OF-SELECTION.
  " Regel: LOOP läuft über die Zeilen, die beim Start vorhanden sind
  APPEND 1 TO t.
  LOOP AT t INTO wa.
    APPEND wa TO t.
  ENDLOOP.
  DESCRIBE TABLE t LINES n.
  WRITE n.
