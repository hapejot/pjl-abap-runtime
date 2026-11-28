REPORT ztest.
CLASS maske DEFINITION.
  PUBLIC SECTION.
    DATA: BEGIN OF ms_maske,
            anzahl TYPE i,
            summe TYPE i,
          END OF ms_maske.
ENDCLASS.
CLASS maske IMPLEMENTATION.
ENDCLASS.
DATA o TYPE REF TO maske.
START-OF-SELECTION.
  CREATE OBJECT o.
  o->ms_maske-anzahl = 5.
  o->ms_maske-summe = 50.
  WRITE / o->ms_maske-anzahl.
  WRITE / o->ms_maske-summe.
