REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
DATA lt TYPE TABLE OF ty_row.
DATA lt2 TYPE TABLE OF ty_row.
DATA ls TYPE ty_row.
START-OF-SELECTION.
  ls-name = 'a'.
  APPEND ls TO lt.
  ls-name = 'b'.
  APPEND ls TO lt.
  lt2 = lt.
  ls-name = 'c'.
  APPEND ls TO lt2.
  LOOP AT lt2 INTO ls.
    WRITE / ls-name.
  ENDLOOP.
  WRITE / 'original'.
  LOOP AT lt INTO ls.
    WRITE / ls-name.
  ENDLOOP.
