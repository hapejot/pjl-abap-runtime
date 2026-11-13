REPORT ztest.
TYPES: BEGIN OF ts,
         name TYPE string,
       END OF ts.
DATA s TYPE ts.
START-OF-SELECTION.
  WRITE / s-name.
  s-name = 'x'.
  WRITE / s-name.
