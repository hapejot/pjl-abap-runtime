REPORT ztest.
DATA g TYPE i.
FORM bump.
  g = g + 10.
ENDFORM.
START-OF-SELECTION.
  g = 1.
  PERFORM bump.
  PERFORM bump.
  WRITE g.
