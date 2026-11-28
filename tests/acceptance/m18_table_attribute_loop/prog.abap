REPORT ztest.
TYPES: BEGIN OF ty_row,
         name TYPE string,
       END OF ty_row.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA mt TYPE STANDARD TABLE OF ty_row WITH EMPTY KEY.
    METHODS add IMPORTING iv TYPE string.
    METHODS show.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD add.
    DATA ls TYPE ty_row.
    ls-name = iv.
    APPEND ls TO mt.
  ENDMETHOD.
  METHOD show.
    DATA ls TYPE ty_row.
    LOOP AT mt INTO ls.
      WRITE / ls-name.
    ENDLOOP.
  ENDMETHOD.
ENDCLASS.
DATA o1 TYPE REF TO lcl.
DATA o2 TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o1.
  CREATE OBJECT o2.
  o1->add( EXPORTING iv = 'eins' ).
  o1->add( EXPORTING iv = 'zwei' ).
  o1->show( ).
  WRITE / 'o2'.
  o2->show( ).
