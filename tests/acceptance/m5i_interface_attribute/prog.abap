INTERFACE if_contract.
  DATA value TYPE i.
ENDINTERFACE.

CLASS worker DEFINITION.
  PUBLIC SECTION.
    INTERFACES if_contract.
    METHODS show.
ENDCLASS.

CLASS worker IMPLEMENTATION.
  METHOD show.
    if_contract~value = 23.
    WRITE / if_contract~value.
  ENDMETHOD.
ENDCLASS.

DATA instance TYPE REF TO worker.
CREATE OBJECT instance.
instance->show( ).
