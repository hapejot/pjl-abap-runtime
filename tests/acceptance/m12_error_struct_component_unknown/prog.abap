REPORT zdynpro_struct_unknown.
DATA: BEGIN OF zkunde,
        name1 TYPE c LENGTH 30,
      END OF zkunde.
START-OF-SELECTION.
  CALL SCREEN 0100.
MODULE status_0100 OUTPUT.
ENDMODULE.
MODULE user_command_0100 INPUT.
ENDMODULE.
