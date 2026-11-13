REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA s TYPE ts.
FORM inc CHANGING p TYPE ts.
  p-a = p-a + 1.
ENDFORM.
START-OF-SELECTION.
  s-a = 41.
  PERFORM inc CHANGING s.
  WRITE s-a.
