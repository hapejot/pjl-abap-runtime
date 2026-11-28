REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  ls_flug-carrid = 'LH'.
  ls_flug-connid = '0400'.
  ls_flug-seatsmax = 111.
  MODIFY zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'modified'.
  ENDIF.
  IF sy-dbcnt = 1.
    WRITE / 'one row'.
  ENDIF.
  ls_flug-connid = '0401'.
  ls_flug-seatsmax = 222.
  MODIFY zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'inserted'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0400'.
  IF ls_flug-seatsmax = 111.
    WRITE / 'seats 111'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0401'.
  IF ls_flug-seatsmax = 222.
    WRITE / 'seats 222'.
  ENDIF.
