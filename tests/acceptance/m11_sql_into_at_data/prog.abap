REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA lv_n TYPE i.
START-OF-SELECTION.
  SELECT SINGLE connid FROM zflug INTO @DATA(lv_nr) WHERE carrid = 'LH' AND seatsmax = 250.
  IF sy-subrc = 0.
    WRITE / lv_nr.
  ENDIF.
  SELECT SINGLE * FROM zflug INTO @DATA(ls_flug) WHERE carrid = 'AA'.
  WRITE / ls_flug-carrid.
  SELECT SINGLE carrid, connid FROM zflug INTO @DATA(ls_pair) WHERE carrid = 'BA'.
  WRITE / ls_pair-connid.
  SELECT * FROM zflug INTO TABLE @DATA(lt_flug) WHERE carrid = 'LH' ORDER BY connid.
  DESCRIBE TABLE lt_flug LINES lv_n.
  IF lv_n = 3.
    WRITE / 'lines ok'.
  ENDIF.
  LOOP AT lt_flug INTO ls_flug.
    WRITE / ls_flug-connid.
  ENDLOOP.
