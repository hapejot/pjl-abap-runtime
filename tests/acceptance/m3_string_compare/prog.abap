REPORT ztest.
DATA s TYPE string.
START-OF-SELECTION.
  s = 'abc'.
  IF s = 'abc'.
    WRITE 'gleich'.
  ENDIF.
  IF s <> 'abd'.
    WRITE 'ungleich'.
  ENDIF.
