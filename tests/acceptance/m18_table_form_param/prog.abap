REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
DATA lt TYPE TABLE OF ty_row.
DATA ls TYPE ty_row.
FORM show USING it TYPE TABLE OF ty_row.
  DATA wa TYPE ty_row.
  LOOP AT it INTO wa.
    WRITE / wa-name.
  ENDLOOP.
ENDFORM.
START-OF-SELECTION.
  ls-name = 'form'.
  APPEND ls TO lt.
  PERFORM show USING lt.
