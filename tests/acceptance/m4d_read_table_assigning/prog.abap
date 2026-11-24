TYPES: BEGIN OF row,
         value TYPE i,
       END OF row.
DATA rows TYPE TABLE OF row.
DATA current TYPE row.
current-value = 42.
APPEND current TO rows.
FIELD-SYMBOLS <row> LIKE LINE OF rows.
READ TABLE rows ASSIGNING <row> INDEX 1.
<row>-value = 43.
WRITE / <row>-value.
READ TABLE rows ASSIGNING <row> INDEX 2.
IF <row> IS NOT ASSIGNED.
  WRITE / 'missing'.
ENDIF.
