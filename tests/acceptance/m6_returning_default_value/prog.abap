REPORT ztest.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA v TYPE i.
    METHODS get RETURNING VALUE(r) TYPE i.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD get.
    r = v * 2.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  " Regel: der Rückgabewert startet mit dem Initialwert des Typs
  WRITE o->get( ).
