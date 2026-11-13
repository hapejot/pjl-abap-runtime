REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
FIELD-SYMBOLS <fs> TYPE i.
START-OF-SELECTION.
  " Regel: Zeiger auf Zeilen bleiben nach APPEND gültig (feste Speicherplätze)
  APPEND 1 TO t.
  APPEND 2 TO t.
  LOOP AT t ASSIGNING <fs>.
    APPEND 7 TO t.
    <fs> = <fs> + 1.
  ENDLOOP.
  LOOP AT t INTO wa.
    WRITE wa.
  ENDLOOP.
