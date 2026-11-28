REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA lv_pat TYPE c LENGTH 3.
START-OF-SELECTION.
  SELECT * FROM zflug INTO ls_flug WHERE carrid LIKE 'L%' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'lower'.
  SELECT * FROM zflug INTO ls_flug WHERE carrid LIKE 'l%' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  lv_pat = '_H%'.
  SELECT * FROM zflug INTO ls_flug WHERE carrid LIKE @lv_pat ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
