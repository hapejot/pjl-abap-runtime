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
    LOOP AT it INTO ls.
      WRITE / ls-name.
    ENDLOOP.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF ty_row.
DATA wa TYPE ty_row.
START-OF-SELECTION.
  CREATE OBJECT o.
  wa-name = 'alpha'.
  APPEND wa TO lt.
  wa-name = 'beta'.
  APPEND wa TO lt.
  o->show( EXPORTING it = lt ).
