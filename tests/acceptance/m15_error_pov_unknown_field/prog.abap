REPORT zpov_demo.

DATA: zkunnr  TYPE c LENGTH 10,
      zlgort  TYPE c LENGTH 4,
      cnt_f4  TYPE i,
      ok_code TYPE sy-ucomm.

START-OF-SELECTION.
  CALL SCREEN 0100.
  WRITE / cnt_f4.
  WRITE / zlgort.

MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.

MODULE f4_lgort INPUT.
  zlgort = 'A001'.
  cnt_f4 = cnt_f4 + 1.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  IF ok_code = 'BACK'.
    LEAVE TO SCREEN 0.
  ENDIF.
ENDMODULE.
