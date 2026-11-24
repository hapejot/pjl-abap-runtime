CLASS typed DEFINITION.
  PUBLIC SECTION.
    TYPES ty_number TYPE i.
    DATA value TYPE ty_number.
    METHODS show.
ENDCLASS.

CLASS typed IMPLEMENTATION.
  METHOD show.
    WRITE / value.
  ENDMETHOD.
ENDCLASS.

DATA instance TYPE REF TO typed.
CREATE OBJECT instance.
instance->value = 19.
instance->show( ).
