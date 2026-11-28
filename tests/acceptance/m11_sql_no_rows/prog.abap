REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
DATA lt_flug TYPE TABLE OF zflug.
START-OF-SELECTION.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'XX'.
  IF sy-subrc = 4.
    WRITE / 'single: 4'.
  ENDIF.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'XX'.
    WRITE / 'row'.
  ENDSELECT.
  IF sy-subrc = 4.
    WRITE / 'loop: 4'.
  ENDIF.
  SELECT * FROM zflug INTO TABLE lt_flug WHERE carrid = 'XX'.
  IF sy-subrc = 4.
    WRITE / 'table: 4'.
  ENDIF.
