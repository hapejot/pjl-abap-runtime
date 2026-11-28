REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
DATA: lt_flug TYPE TABLE OF zflug,
      ls_flug TYPE zflug.
START-OF-SELECTION.
  ls_flug-carrid = 'LH'.
  ls_flug-connid = '0401'.
  ls_flug-seatsmax = 1.
  APPEND ls_flug TO lt_flug.
  ls_flug-connid = '0400'.
  APPEND ls_flug TO lt_flug.
  INSERT zflug FROM TABLE lt_flug.
  WRITE / 'not reached'.
