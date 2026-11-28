REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  UPDATE zflug SET seatsmax = 200 WHERE carrid = 'LH'.
  COMMIT WORK.
  UPDATE zflug SET seatsmax = 300 WHERE carrid = 'LH'.
  ROLLBACK WORK.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH'.
  IF ls_flug-seatsmax = 200.
    WRITE / 'committed kept'.
  ENDIF.
  IF ls_flug-seatsmax = 300.
    WRITE / 'rolled back'.
  ENDIF.
