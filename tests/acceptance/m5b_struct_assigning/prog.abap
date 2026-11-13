REPORT ztest.
TYPES: BEGIN OF ts,
         a TYPE i,
       END OF ts.
DATA t TYPE TABLE OF ts.
DATA wa TYPE ts.
FIELD-SYMBOLS <fs> TYPE ts.
START-OF-SELECTION.
  wa-a = 1.
  APPEND wa TO t.
  wa-a = 2.
  APPEND wa TO t.
  LOOP AT t ASSIGNING <fs>.
    <fs>-a = <fs>-a * 10.
  ENDLOOP.
  LOOP AT t INTO wa.
    WRITE / wa-a.
  ENDLOOP.
