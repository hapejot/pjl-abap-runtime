REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA s TYPE ts.
FORM show.
  WRITE s-a.
ENDFORM.
START-OF-SELECTION.
  s-a = 7.
  PERFORM show.
