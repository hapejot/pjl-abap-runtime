CLASS counter DEFINITION.
  PUBLIC SECTION.
    CLASS-DATA value TYPE i.
    CLASS-METHODS add IMPORTING amount TYPE i.
ENDCLASS.

CLASS counter IMPLEMENTATION.
  METHOD add.
    counter=>value = counter=>value + amount.
  ENDMETHOD.
ENDCLASS.

counter=>add( 40 ).
counter=>value = counter=>value + 2.
WRITE / counter=>value.
