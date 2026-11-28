REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
TYPES: BEGIN OF zrow,
         carrid TYPE c LENGTH 5,
         connid TYPE n LENGTH 6,
         seatsmax TYPE i,
       END OF zrow.
DATA ls_row TYPE zrow.
START-OF-SELECTION.
  SELECT SINGLE * FROM zflug INTO CORRESPONDING FIELDS OF ls_row WHERE carrid = 'LH' AND connid = '0400'.
  WRITE / ls_row-carrid.
  WRITE / ls_row-connid.
