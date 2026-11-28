REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA ls_flug TYPE zflug.
START-OF-SELECTION.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH'.
  WRITE / 'nach'.
