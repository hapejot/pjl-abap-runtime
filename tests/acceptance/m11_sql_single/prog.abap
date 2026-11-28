REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA lv_von TYPE c LENGTH 3.
START-OF-SELECTION.
  lv_von = 'LH'.
  SELECT SINGLE * FROM zflug INTO ls_flug
    WHERE carrid = @lv_von AND connid = '0400'.
  IF sy-subrc = 0.
    WRITE / 'found'.
    WRITE / ls_flug-carrid.
    WRITE / ls_flug-connid.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug
    WHERE carrid = @lv_von AND connid = '9999'.
  IF sy-subrc = 4.
    WRITE / 'not found'.
  ENDIF.
