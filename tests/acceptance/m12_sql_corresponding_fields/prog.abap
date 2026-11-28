REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
TYPES: BEGIN OF zrow,
         seatsmax TYPE i,
         carrid TYPE c LENGTH 3,
         extra TYPE c LENGTH 2,
       END OF zrow.
DATA ls_row TYPE zrow.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  ls_row-extra = 'XY'.
  SELECT SINGLE * FROM zflug INTO CORRESPONDING FIELDS OF ls_row WHERE carrid = 'LH' AND connid = '0400'.
  WRITE / ls_row-carrid.
  IF ls_row-seatsmax = 100.
    WRITE / 'seats 100'.
  ENDIF.
  IF ls_row-extra = 'XY'.
    WRITE / 'extra kept'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO CORRESPONDING FIELDS OF ls_flug WHERE carrid = 'AA'.
  WRITE / ls_flug-connid.
