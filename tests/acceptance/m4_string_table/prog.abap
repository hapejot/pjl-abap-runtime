REPORT ztest.
DATA t TYPE TABLE OF string.
DATA s TYPE string.
START-OF-SELECTION.
  APPEND 'eins' TO t.
  APPEND 'zwei' && 'x' TO t.
  LOOP AT t INTO s.
    WRITE / s.
  ENDLOOP.
