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
  ls_flug-seatsmax = 999.
  UPDATE zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'updated by key'.
  ENDIF.
  IF sy-dbcnt = 1.
    WRITE / 'one changed'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0400'.
  IF ls_flug-seatsmax = 999.
    WRITE / 'seats 999'.
  ENDIF.
  ls_flug-connid = '0401'.
  UPDATE zflug FROM ls_flug.
  IF sy-subrc = 4.
    WRITE / 'no key match'.
  ENDIF.
  ls_flug-connid = '0400'.
  DELETE zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'deleted by key'.
  ENDIF.
  IF sy-dbcnt = 1.
    WRITE / 'one deleted'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0400'.
  IF sy-subrc = 4.
    WRITE / 'gone'.
  ENDIF.
