REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
START-OF-SELECTION.
  APPEND 1 TO t.
  APPEND 2 TO t.
  APPEND 3 TO t.
  APPEND 4 TO t.
  LOOP AT t INTO wa.
    CHECK wa <> 2.
    IF wa = 3.
      EXIT.
    ENDIF.
    WRITE wa.
  ENDLOOP.
  WRITE 'ende'.
