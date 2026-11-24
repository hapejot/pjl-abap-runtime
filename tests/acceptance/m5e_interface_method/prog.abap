INTERFACE if_contract.
  METHODS show IMPORTING value TYPE i.
ENDINTERFACE.

CLASS printer DEFINITION.
  PUBLIC SECTION.
    INTERFACES if_contract.
ENDCLASS.

CLASS printer IMPLEMENTATION.
  METHOD if_contract~show.
    WRITE / value.
  ENDMETHOD.
ENDCLASS.

DATA instance TYPE REF TO printer.
CREATE OBJECT instance.
instance->if_contract~show( 7 ).
