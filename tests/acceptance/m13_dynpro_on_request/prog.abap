REPORT zon_request.

DATA: zkunnr    TYPE c LENGTH 10,
      zname     TYPE c LENGTH 30,
      zwerk     TYPE c LENGTH 4,
      zlgort    TYPE c LENGTH 4,
      cnt_kunnr TYPE i,
      cnt_name  TYPE i,
      cnt_chain TYPE i,
      ok_code   TYPE sy-ucomm.

START-OF-SELECTION.
  CALL SCREEN 0100.
  WRITE / cnt_kunnr.
  WRITE / cnt_name.
  WRITE / cnt_chain.

MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
ENDMODULE.

MODULE check_kunnr INPUT.
  cnt_kunnr = cnt_kunnr + 1.
ENDMODULE.

MODULE check_name INPUT.
  cnt_name = cnt_name + 1.
ENDMODULE.

MODULE check_chain INPUT.
  cnt_chain = cnt_chain + 1.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  IF ok_code = 'BACK'.
    LEAVE TO SCREEN 0.
  ENDIF.
ENDMODULE.
