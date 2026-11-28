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
  DELETE FROM zflug WHERE carrid = @lv_von.
  IF sy-subrc = 0.
    WRITE / 'deleted'.
  ENDIF.
  IF sy-dbcnt = 2.
    WRITE / 'two deleted'.
  ENDIF.
  DELETE FROM zflug WHERE carrid = 'XX'.
  IF sy-subrc = 4.
    WRITE / 'nothing deleted'.
  ENDIF.
  IF sy-dbcnt = 0.
    WRITE / 'count zero'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH'.
  IF sy-subrc = 4.
    WRITE / 'no LH'.
  ENDIF.
  DELETE FROM zflug.
  IF sy-dbcnt = 2.
    WRITE / 'rest deleted'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'AA'.
  IF sy-subrc = 4.
    WRITE / 'table empty'.
  ENDIF.
