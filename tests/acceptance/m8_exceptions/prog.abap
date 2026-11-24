CLASS failure DEFINITION INHERITING FROM cx_static_check.
ENDCLASS.

CLASS failure IMPLEMENTATION.
ENDCLASS.

DATA exception TYPE REF TO failure.
TRY.
  CREATE OBJECT exception.
  RAISE EXCEPTION exception.
  WRITE 'not reached'.
CATCH cx_static_check INTO exception.
  WRITE 'caught'.
ENDTRY.
