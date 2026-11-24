TYPES: BEGIN OF state,
         value TYPE i,
       END OF state.
DATA record TYPE state.
FIELD-SYMBOLS <value> TYPE i.
IF <value> IS NOT ASSIGNED.
  WRITE / 'unassigned'.
ENDIF.
ASSIGN COMPONENT 'VALUE' OF STRUCTURE record TO <value>.
IF <value> IS ASSIGNED.
  <value> = 7.
ENDIF.
WRITE / record-value.
