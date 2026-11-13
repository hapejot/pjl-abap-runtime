REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
         b TYPE i,
       END OF ts.
DATA t TYPE TABLE OF ts.
DATA wa TYPE ts.
DATA sum TYPE i.
START-OF-SELECTION.
  wa-a = 2. wa-b = 3. APPEND wa TO t.
  wa-a = 4. wa-b = 5. APPEND wa TO t.
  sum = 0.
  LOOP AT t INTO wa.
    sum = sum + wa-a * wa-b.
  ENDLOOP.
  WRITE sum.
