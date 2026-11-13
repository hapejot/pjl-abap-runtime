REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
         b TYPE string,
       END OF ts.
DATA t TYPE TABLE OF ts.
DATA wa TYPE ts.
START-OF-SELECTION.
  wa-a = 1.
  wa-b = 'eins'.
  APPEND wa TO t.
  wa-a = 2.
  wa-b = 'zwei'.
  APPEND wa TO t.
  LOOP AT t INTO wa.
    WRITE wa-a.
    WRITE wa-b.
  ENDLOOP.
