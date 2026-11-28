REPORT ztest.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS get RETURNING VALUE(rv) TYPE c LENGTH 1.
    METHODS run IMPORTING iv TYPE i.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD get.
    rv = 'X'.
  ENDMETHOD.
  METHOD run.
    CASE iv.
      WHEN 1.
        WRITE / 'one'.
      WHEN 2.
        IF me->get( ) = 'X'.
          WRITE / 'two yes'.
        ENDIF.
      WHEN OTHERS.
        IF me->get( ) = 'X'.
          WRITE / 'other yes'.
        ELSE.
          WRITE / 'other no'.
        ENDIF.
    ENDCASE.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->run( 2 ).
  o->run( 3 ).
  o->run( 1 ).
