REPORT ztest.
DATA t TYPE string.
FORM app CHANGING s TYPE string.
  s = s && 'x'.
ENDFORM.
START-OF-SELECTION.
  t = 'a'.
  PERFORM app CHANGING t.
  PERFORM app CHANGING t.
  WRITE t.
