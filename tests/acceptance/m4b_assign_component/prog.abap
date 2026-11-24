TYPES: BEGIN OF state,
         value TYPE i,
       END OF state.
DATA record TYPE state.
FIELD-SYMBOLS <value> TYPE i.
ASSIGN COMPONENT 'VALUE' OF STRUCTURE record TO <value>.
<value> = 42.
WRITE / record-value.
