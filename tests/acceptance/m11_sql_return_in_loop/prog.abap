REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
FORM first_connid.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    WRITE / ls_flug-connid.
    RETURN.
  ENDSELECT.
  WRITE / 'not reached'.
ENDFORM.
START-OF-SELECTION.
  PERFORM first_connid.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'AA'.
  WRITE / ls_flug-connid.
