REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA: ls_flug TYPE zflug,
      ls_aa TYPE zflug.
START-OF-SELECTION.
  SELECT * FROM zflug INTO @ls_flug WHERE carrid = 'LH' ORDER BY connid.
    SELECT SINGLE * FROM zflug INTO @ls_aa WHERE carrid = 'LH' AND connid = '0400'.
    WRITE / ls_flug-connid.
  ENDSELECT.
  IF sy-dbcnt = 2.
    WRITE / 'loop count two'.
  ENDIF.
