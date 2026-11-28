REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
TYPES: BEGIN OF zrow,
         carrid TYPE c LENGTH 3,
         extra TYPE c LENGTH 2,
         amount TYPE p LENGTH 4 DECIMALS 2,
         seatsmax TYPE i,
       END OF zrow.
DATA: lt_rows TYPE TABLE OF zrow,
      ls_row TYPE zrow.
START-OF-SELECTION.
  SELECT * FROM zflug INTO CORRESPONDING FIELDS OF TABLE @lt_rows WHERE carrid = 'LH'.
  LOOP AT lt_rows INTO ls_row.
    IF ls_row-extra = '  '.
      WRITE / 'blank'.
    ENDIF.
    IF ls_row-amount = 0.
      WRITE / 'zero'.
    ENDIF.
    IF ls_row-seatsmax > 0.
      WRITE / 'seats set'.
    ENDIF.
  ENDLOOP.
