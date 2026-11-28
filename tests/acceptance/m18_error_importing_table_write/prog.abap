REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS show IMPORTING it TYPE TABLE OF ty_row.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD show.
    DATA ls TYPE ty_row.
    ls-name = 'x'.
    APPEND ls TO it.
  ENDMETHOD.
ENDCLASS.
START-OF-SELECTION.
  WRITE / 'translated'.
