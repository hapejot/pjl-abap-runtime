REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
TYPES: BEGIN OF ty_other,
         id TYPE i,
       END OF ty_other.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS show IMPORTING it TYPE TABLE OF ty_row.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD show.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF ty_other.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->show( EXPORTING it = lt ).
