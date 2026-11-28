REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS add CHANGING ct TYPE TABLE OF ty_row.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD add.
    DATA ls TYPE ty_row.
    ls-name = 'neu'.
    APPEND ls TO ct.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF ty_row.
DATA wa TYPE ty_row.
START-OF-SELECTION.
  CREATE OBJECT o.
  wa-name = 'alt'.
  APPEND wa TO lt.
  o->add( CHANGING ct = lt ).
  LOOP AT lt INTO wa.
    WRITE / wa-name.
  ENDLOOP.
