REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA: lt_flug TYPE TABLE OF zflug,
      lt_dup TYPE TABLE OF zflug,
      ls_flug TYPE zflug.
START-OF-SELECTION.
  ls_flug-carrid = 'LH'.
  ls_flug-connid = '0401'.
  ls_flug-seatsmax = 250.
  APPEND ls_flug TO lt_flug.
  ls_flug-connid = '0402'.
  ls_flug-seatsmax = 50.
  APPEND ls_flug TO lt_flug.
  ls_flug-carrid = 'BA'.
  ls_flug-connid = '0900'.
  ls_flug-seatsmax = 275.
  APPEND ls_flug TO lt_flug.
  INSERT zflug FROM TABLE lt_flug.
  IF sy-subrc = 0.
    WRITE / 'inserted'.
  ENDIF.
  IF sy-dbcnt = 3.
    WRITE / 'three rows'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  ls_flug-carrid = 'LH'.
  ls_flug-connid = '0400'.
  ls_flug-seatsmax = 1.
  APPEND ls_flug TO lt_dup.
  ls_flug-connid = '0403'.
  ls_flug-seatsmax = 2.
  APPEND ls_flug TO lt_dup.
  INSERT zflug FROM TABLE lt_dup ACCEPTING DUPLICATE KEYS.
  IF sy-subrc = 4.
    WRITE / 'duplicate skipped'.
  ENDIF.
  IF sy-dbcnt = 1.
    WRITE / 'one new row'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
