REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'XX'.
  IF sy-subrc = 4.
    COMMIT WORK.
    IF sy-subrc = 4.
      WRITE / 'subrc kept after commit'.
    ENDIF.
    ROLLBACK WORK.
    IF sy-subrc = 4.
      WRITE / 'subrc kept after rollback'.
    ENDIF.
  ENDIF.
