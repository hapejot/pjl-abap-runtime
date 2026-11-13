REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
DATA s TYPE i.
START-OF-SELECTION.
  wa = 1. APPEND wa TO t.
  wa = 2. APPEND wa TO t.
  wa = 3. APPEND wa TO t.
  s = 0.
  LOOP AT t INTO wa.
    s = s + wa.
  ENDLOOP.
  WRITE s.
