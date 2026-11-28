REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    IF sy-dbcnt = 1.
      WRITE / 'first'.
    ENDIF.
    WRITE / ls_flug-connid.
  ENDSELECT.
  IF sy-subrc = 0.
    WRITE / 'after loop'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    WRITE / ls_flug-connid.
    EXIT.
  ENDSELECT.
  IF sy-subrc = 0.
    WRITE / 'exit ok'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    CHECK ls_flug-connid = '0401'.
    WRITE / 'checked'.
  ENDSELECT.
