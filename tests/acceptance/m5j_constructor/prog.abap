CLASS item DEFINITION.
  PUBLIC SECTION.
    DATA value TYPE i.
    METHODS constructor IMPORTING initial TYPE i.
ENDCLASS.

CLASS item IMPLEMENTATION.
  METHOD constructor.
    value = initial.
  ENDMETHOD.
ENDCLASS.

DATA instance TYPE REF TO item.
CREATE OBJECT instance EXPORTING initial = 42.
WRITE / instance->value.
