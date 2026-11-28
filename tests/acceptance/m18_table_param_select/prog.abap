REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    METHODS load EXPORTING et TYPE TABLE OF zflug.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD load.
    SELECT * FROM zflug INTO TABLE et WHERE carrid = 'LH' ORDER BY connid.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
DATA lt TYPE TABLE OF zflug.
DATA wa TYPE zflug.
START-OF-SELECTION.
  CREATE OBJECT o.
  wa-connid = '9999'.
  APPEND wa TO lt.
  o->load( IMPORTING et = lt ).
  LOOP AT lt INTO wa.
    WRITE / wa-connid.
  ENDLOOP.
