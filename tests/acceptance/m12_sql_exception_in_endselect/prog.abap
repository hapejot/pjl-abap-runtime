REPORT ztest.
CLASS failure DEFINITION INHERITING FROM cx_static_check.
ENDCLASS.
CLASS failure IMPLEMENTATION.
ENDCLASS.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA exception TYPE REF TO failure.
CLASS checker DEFINITION.
  PUBLIC SECTION.
    METHODS verify IMPORTING n TYPE c LENGTH 4.
ENDCLASS.
CLASS checker IMPLEMENTATION.
  METHOD verify.
    DATA lx TYPE REF TO failure.
    IF n = '0401'.
      CREATE OBJECT lx.
      RAISE EXCEPTION lx.
    ENDIF.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO checker.
START-OF-SELECTION.
  CREATE OBJECT o.
  TRY.
    SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
      WRITE / ls_flug-connid.
      o->verify( ls_flug-connid ).
      WRITE / 'after call'.
    ENDSELECT.
    WRITE / 'after loop'.
  CATCH cx_static_check INTO exception.
    WRITE / 'caught'.
  ENDTRY.
