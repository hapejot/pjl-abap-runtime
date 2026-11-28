REPORT ztest.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS check IMPORTING iv TYPE sy-ucomm.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD check.
    IF iv = 'SAVE'.
      WRITE / 'save'.
    ELSE.
      WRITE / 'other'.
    ENDIF.
  ENDMETHOD.
ENDCLASS.
DATA ok_code TYPE sy-ucomm.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  ok_code = 'SAVE'.
  o->check( ok_code ).
  ok_code = 'CANCEL'.
  o->check( ok_code ).
