REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA s TYPE ts.
START-OF-SELECTION.
  s-a = 5.
  CLEAR s.
  WRITE s-a.
