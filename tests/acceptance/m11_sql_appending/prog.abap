REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA lt_flug TYPE TABLE OF zflug.
DATA lv_n TYPE i.
START-OF-SELECTION.
  SELECT * FROM zflug INTO TABLE lt_flug WHERE carrid = 'LH' ORDER BY connid.
  SELECT * FROM zflug APPENDING TABLE lt_flug WHERE carrid = 'AA' ORDER BY connid.
  DESCRIBE TABLE lt_flug LINES lv_n.
  IF lv_n = 4.
    WRITE / 'appended'.
  ENDIF.
  LOOP AT lt_flug INTO ls_flug.
    WRITE / ls_flug-carrid.
  ENDLOOP.
