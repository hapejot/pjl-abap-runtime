REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
TYPES: BEGIN OF zother,
         carrid TYPE c LENGTH 3,
       END OF zother.
DATA: ls_flug  TYPE zflug,
      ls_other TYPE zother.
START-OF-SELECTION.
  SELECT SINGLE * FROM zflug INTO ls_flug WHERE carrid = 'LH'.
  WRITE / 'zflug read'.
  SELECT SINGLE * FROM zother INTO ls_other WHERE carrid = 'LH'.
  WRITE / 'not reached'.
