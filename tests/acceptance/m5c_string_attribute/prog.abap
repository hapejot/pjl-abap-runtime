REPORT ztest.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA s TYPE string.
    METHODS add IMPORTING t TYPE string.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD add.
    s = s && t.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->add( 'ab' ).
  o->add( 'cd' ).
  WRITE o->s.
