REPORT ztest.
DATA x TYPE i.
START-OF-SELECTION.
  " Regel: negative Zahl wird mit Vorzeichen am Ende ausgegeben
  x = 2 - 5.
  WRITE x.
