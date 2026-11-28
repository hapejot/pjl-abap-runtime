REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
CLASS lcl DEFINITION.
  PUBLIC SECTION.
    DATA mt TYPE STANDARD TABLE OF zflug WITH EMPTY KEY.
    METHODS load.
    METHODS show.
ENDCLASS.
CLASS lcl IMPLEMENTATION.
  METHOD load.
    SELECT * FROM zflug INTO TABLE mt WHERE carrid = 'LH' ORDER BY connid.
  ENDMETHOD.
  METHOD show.
    DATA wa TYPE zflug.
    LOOP AT mt INTO wa.
      WRITE / wa-connid.
    ENDLOOP.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO lcl.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->load( ).
  o->show( ).
