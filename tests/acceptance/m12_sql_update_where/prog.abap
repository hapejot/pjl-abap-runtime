REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA lv_von TYPE c LENGTH 3.
DATA lv_max TYPE i.
START-OF-SELECTION.
  lv_von = 'LH'.
  lv_max = 500.
  UPDATE zflug SET seatsmax = @lv_max WHERE carrid = @lv_von.
  IF sy-subrc = 0.
    WRITE / 'updated'.
  ENDIF.
  IF sy-dbcnt = 2.
    WRITE / 'two rows'.
  ENDIF.
  lv_von = 'XX'.
  UPDATE zflug SET seatsmax = 1 WHERE carrid = @lv_von.
  IF sy-subrc = 4.
    WRITE / 'no row'.
  ENDIF.
  IF sy-dbcnt = 0.
    WRITE / 'count zero'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH' AND connid = '0401'.
  IF ls_flug-seatsmax = 500.
    WRITE / 'seats 500'.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'AA'.
  IF ls_flug-seatsmax = 300.
    WRITE / 'aa unchanged'.
  ENDIF.
