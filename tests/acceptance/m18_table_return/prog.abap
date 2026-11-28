REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS make RETURNING VALUE(rt) TYPE TABLE OF ty_row.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD make.
    DATA ls TYPE ty_row.
    ls-name = 'eins'.
    APPEND ls TO rt.
    ls-name = 'zwei'.
    APPEND ls TO rt.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF ty_row.
DATA wa TYPE ty_row.
START-OF-SELECTION.
  CREATE OBJECT o.
  lt = o->make( ).
  LOOP AT lt INTO wa.
    WRITE / wa-name.
  ENDLOOP.
