REPORT ztest.
TYPES: BEGIN OF ty_row,
         id TYPE i,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA mt_rows TYPE STANDARD TABLE OF ty_row WITH EMPTY KEY.
    METHODS run.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD run.
    DATA ls_row TYPE ty_row.
    ls_row-id = 1.
    MODIFY mt_rows FROM ls_row.
    DELETE mt_rows FROM ls_row.
  ENDMETHOD.
ENDCLASS.
START-OF-SELECTION.
  WRITE / 'translated'.
