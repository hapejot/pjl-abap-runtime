REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  ls_flug-carrid = 'UA'.
  ls_flug-connid = '0500'.
  ls_flug-seatsmax = 120.
  INSERT INTO zflug VALUES ls_flug.
  IF sy-subrc = 0.
    WRITE / 'inserted'.
  ENDIF.
  ls_flug-connid = '0600'.
  INSERT zflug FROM ls_flug.
  IF sy-subrc = 0.
    WRITE / 'inserted again'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'UA' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'UA' AND connid = '0500'.
  IF ls_flug-seatsmax = 120.
    WRITE / 'seats 120'.
  ENDIF.
