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
  IF sy-subrc = 0.
    WRITE / 'found'.
  ENDIF.
  DESCRIBE TABLE lt_flug LINES lv_n.
  IF lv_n = 3.
    WRITE / 'lines ok'.
  ENDIF.
  LOOP AT lt_flug INTO ls_flug.
    WRITE / ls_flug-connid.
  ENDLOOP.
  SELECT * FROM zflug INTO TABLE lt_flug WHERE carrid = 'XX'.
  IF sy-subrc = 4.
    WRITE / 'empty'.
  ENDIF.
  DESCRIBE TABLE lt_flug LINES lv_n.
  IF lv_n = 0.
    WRITE / 'cleared'.
  ENDIF.
