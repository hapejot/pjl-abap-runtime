REPORT ztest.
TYPES: BEGIN OF zflug,
         carrid TYPE c LENGTH 3,
         connid TYPE n LENGTH 4,
         seatsmax TYPE i,
       END OF zflug.
TYPES: BEGIN OF zkurz,
         carrid TYPE c LENGTH 3,
       END OF zkurz.
DATA ls_kurz TYPE zkurz.
START-OF-SELECTION.
  INSERT zflug FROM ls_kurz.
