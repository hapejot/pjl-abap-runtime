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
  ls_flug-seatsmax = 5.
  INSERT INTO zflug VALUES ls_flug.
  IF sy-subrc = 4.
    WRITE / 'duplicate'.
  ENDIF.
  IF sy-dbcnt = 0.
    WRITE / 'count zero'.
  ENDIF.
  ls_flug-connid = '0402'.
  INSERT zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'new row'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0400'.
  IF ls_flug-seatsmax = 100.
    WRITE / 'original kept'.
  ENDIF.
  WRITE / 'continues'.
