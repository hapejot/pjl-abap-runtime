REPORT zkunde_demo.

DATA: zkunnr  TYPE c LENGTH 10,
      zname1  TYPE c LENGTH 30,
      zort    TYPE c LENGTH 30,
      ok_code TYPE sy-ucomm.

START-OF-SELECTION.
  CALL SCREEN 0100.
  WRITE / zkunnr.

MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'SAVE'.
      MESSAGE 'Customer saved' TYPE 'S'.
    WHEN 'BACK' OR 'CANCEL'.
      LEAVE TO SCREEN 0.
  ENDCASE.
ENDMODULE.

MODULE exit_0100 INPUT.
  LEAVE PROGRAM.
ENDMODULE.
