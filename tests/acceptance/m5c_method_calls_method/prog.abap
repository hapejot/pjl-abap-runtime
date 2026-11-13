REPORT ztest.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA v TYPE i.
    METHODS inc.
    METHODS twice.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD inc.
    v = v + 1.
  ENDMETHOD.
  METHOD twice.
    me->inc( ).
    me->inc( ).
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->twice( ).
  WRITE o->v.
