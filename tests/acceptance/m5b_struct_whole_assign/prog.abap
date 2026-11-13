REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA s TYPE ts.
DATA t TYPE ts.
START-OF-SELECTION.
  s-a = 1.
  " Regel: Zuweisung kopiert alle Komponenten
  t = s.
  t-a = 2.
  WRITE s-a.
  WRITE / t-a.
