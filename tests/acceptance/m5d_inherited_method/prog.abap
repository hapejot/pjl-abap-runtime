CLASS base DEFINITION.
  PUBLIC SECTION.
    DATA value TYPE i.
    METHODS show.
ENDCLASS.

CLASS base IMPLEMENTATION.
  METHOD show.
    WRITE / value.
  ENDMETHOD.
ENDCLASS.

CLASS child DEFINITION INHERITING FROM base.
  PUBLIC SECTION.
    METHODS set_value IMPORTING new_value TYPE i.
ENDCLASS.

CLASS child IMPLEMENTATION.
  METHOD set_value.
    value = new_value.
  ENDMETHOD.
ENDCLASS.

DATA instance TYPE REF TO child.
CREATE OBJECT instance.
instance->set_value( 42 ).
instance->show( ).
