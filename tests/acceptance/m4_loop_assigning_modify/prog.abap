REPORT ztest.
DATA t TYPE TABLE OF i.
DATA wa TYPE i.
FIELD-SYMBOLS <fs> TYPE i.
START-OF-SELECTION.
  APPEND 1 TO t.
  APPEND 2 TO t.
  " Regel: ASSIGNING liefert einen Zeiger; Änderungen wirken auf die Tabelle
  LOOP AT t ASSIGNING <fs>.
    <fs> = <fs> * 10.
  ENDLOOP.
  LOOP AT t INTO wa.
    WRITE wa.
  ENDLOOP.
