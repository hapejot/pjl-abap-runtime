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
START-OF-SELECTION.
  TRY.
    SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
      WRITE / ls_flug-connid.
      CREATE OBJECT exception.
      RAISE EXCEPTION exception.
    ENDSELECT.
  CATCH cx_static_check INTO exception.
    WRITE / 'caught'.
  ENDTRY.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'AA'.
  WRITE / ls_flug-connid.
