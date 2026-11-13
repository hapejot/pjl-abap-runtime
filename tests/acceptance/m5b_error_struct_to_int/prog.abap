REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA s TYPE ts.
DATA x TYPE i.
START-OF-SELECTION.
  x = s.
