REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS load EXPORTING et TYPE TABLE OF ty_row.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD load.
    DATA ls TYPE ty_row.
    CLEAR et.
    ls-name = 'gamma'.
    APPEND ls TO et.
    ls-name = 'delta'.
    APPEND ls TO et.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF ty_row.
DATA wa TYPE ty_row.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->load( IMPORTING et = lt ).
  LOOP AT lt INTO wa.
    WRITE / wa-name.
  ENDLOOP.
