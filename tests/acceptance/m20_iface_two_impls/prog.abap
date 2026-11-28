REPORT ztest.
INTERFACE lif_calc.
  METHODS run IMPORTING iv TYPE string RETURNING VALUE(rv) TYPE string.
ENDINTERFACE.
CLASS lcl_add DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_calc.
ENDCLASS.
CLASS lcl_add IMPLEMENTATION.
  METHOD lif_calc~run.
    rv = 'add'.
  ENDMETHOD.
ENDCLASS.
CLASS lcl_mul DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_calc.
ENDCLASS.
CLASS lcl_mul IMPLEMENTATION.
  METHOD lif_calc~run.
    rv = iv.
  ENDMETHOD.
ENDCLASS.
DATA lo TYPE REF TO lif_calc.
DATA lv TYPE string.
START-OF-SELECTION.
  CREATE OBJECT lo TYPE lcl_add.
  lv = lo->run( iv = 'x' ).
  WRITE / lv.
  CREATE OBJECT lo TYPE lcl_mul.
  lv = lo->run( iv = 'x' ).
  WRITE / lv.
