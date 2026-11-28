REPORT ztest.
CLASS maske DEFINITION.
  PUBLIC SECTION.
    DATA: BEGIN OF ms_maske,
            anzahl TYPE i,
            summe TYPE i,
          END OF ms_maske.
    METHODS init.
ENDCLASS.
CLASS maske IMPLEMENTATION.
  METHOD init.
    ms_maske-anzahl = 3.
    ms_maske-summe = 40.
  ENDMETHOD.
ENDCLASS.
DATA o TYPE REF TO maske.
DATA x TYPE i.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->init( ).
  x = o->ms_maske-summe.
  WRITE / x.
  WRITE / o->ms_maske-anzahl.
