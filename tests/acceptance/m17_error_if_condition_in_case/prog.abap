REPORT ztest.
CLASS worker DEFINITION.
  PUBLIC SECTION.
    METHODS getp IMPORTING iv TYPE c RETURNING VALUE(rv) TYPE c LENGTH 1.
    METHODS run.
ENDCLASS.
CLASS worker IMPLEMENTATION.
  METHOD getp.
    rv = iv.
  ENDMETHOD.
  METHOD run.
    DATA ok_code TYPE sy-ucomm.
    CASE ok_code.
      WHEN 'SAVE'.
        IF getp( ok_code ) = 'X'.
          WRITE / 'x'.
        ENDIF.
      WHEN OTHERS.
        WRITE / 'other'.
    ENDCASE.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO worker.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->run( ).
