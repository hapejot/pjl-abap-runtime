REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  WRITE / 'Q1'.
  SELECT * FROM zflug INTO ls_flug WHERE carrid = 'LH' ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q2'.
  SELECT * FROM zflug INTO ls_flug WHERE carrid <> 'LH' AND seatsmax > 200 ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q3'.
  SELECT * FROM zflug INTO ls_flug WHERE seatsmax BETWEEN 100 AND 275 ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q4'.
  SELECT * FROM zflug INTO ls_flug WHERE connid IN ('0017', '0402') ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q5'.
  SELECT * FROM zflug INTO ls_flug WHERE NOT connid IN ('0400', '0401') ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q6'.
  SELECT * FROM zflug INTO ls_flug WHERE carrid LIKE 'B%' OR seatsmax < 60 ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q7'.
  SELECT * FROM zflug INTO ls_flug WHERE ( carrid = 'AA' OR carrid = 'BA' ) AND NOT seatsmax = 300 ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
  WRITE / 'Q8'.
  SELECT * FROM zflug INTO ls_flug
    WHERE connid >= '0400' AND seatsmax > 60 AND seatsmax < 300 AND connid <= '0901' AND connid <> '0500'
    ORDER BY connid.
    WRITE / ls_flug-connid.
  ENDSELECT.
