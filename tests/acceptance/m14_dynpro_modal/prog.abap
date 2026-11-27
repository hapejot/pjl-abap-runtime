REPORT zmodal_demo.

DATA: zname1  TYPE c LENGTH 30,
      ok_code TYPE sy-ucomm.

START-OF-SELECTION.
  CALL SCREEN 0100.
  WRITE / zname1.

MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'POPUP'.
      CALL SCREEN 0200 STARTING AT 10 5 ENDING AT 60 15.
    WHEN 'BACK' OR 'CANCEL'.
      LEAVE TO SCREEN 0.
  ENDCASE.
ENDMODULE.

MODULE status_0200 OUTPUT.
  SET PF-STATUS 'STATUS_0200'.
ENDMODULE.

MODULE user_command_0200 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'BACK'.
      LEAVE SCREEN.
  ENDCASE.
ENDMODULE.
