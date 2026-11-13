REPORT ztest.
TYPES: BEGIN OF ts,
         c TYPE c LENGTH 3,
       END OF ts.
DATA s TYPE ts.
START-OF-SELECTION.
  " Regel: Zeichenkomponenten beginnen mit Leerzeichen
  WRITE s-c.
