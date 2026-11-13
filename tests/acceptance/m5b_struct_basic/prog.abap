REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
         b TYPE c LENGTH 2,
       END OF ts.
DATA s TYPE ts.
START-OF-SELECTION.
  s-a = 3.
  s-b = 'xy'.
  WRITE s-a.
  WRITE s-b.
