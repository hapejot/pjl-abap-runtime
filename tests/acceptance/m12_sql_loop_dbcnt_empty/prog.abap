REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA: lt_flug TYPE TABLE OF zflug,
      ls_flug TYPE zflug.
START-OF-SELECTION.
  SELECT * FROM zflug INTO TABLE @lt_flug WHERE carrid = 'LH'.
  IF sy-dbcnt = 2.
    WRITE / 'two rows read'.
  ENDIF.
  SELECT * FROM zflug INTO @ls_flug WHERE carrid = 'XX'.
    WRITE / 'never'.
  ENDSELECT.
  IF sy-subrc = 4.
    WRITE / 'no rows'.
  ENDIF.
  IF sy-dbcnt = 0.
    WRITE / 'empty loop dbcnt zero'.
  ENDIF.
