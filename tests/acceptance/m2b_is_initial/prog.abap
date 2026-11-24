DATA number TYPE i.
DATA text TYPE string.
IF number IS INITIAL.
  WRITE / 'integer'.
ENDIF.
IF text IS INITIAL.
  WRITE / 'string'.
ENDIF.
text = 'x'.
IF text IS NOT INITIAL.
  WRITE / 'filled'.
ENDIF.
