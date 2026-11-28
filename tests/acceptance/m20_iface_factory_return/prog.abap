REPORT ztest.
INTERFACE lif_calc.
  METHODS run RETURNING VALUE(rv) TYPE string.
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
    rv = 'mul'.
  ENDMETHOD.
ENDCLASS.
CLASS lcl_factory DEFINITION.
  PUBLIC SECTION.
    METHODS make IMPORTING iv_kind TYPE i RETURNING VALUE(ro) TYPE REF TO lif_calc.
ENDCLASS.
CLASS lcl_factory IMPLEMENTATION.
  METHOD make.
    IF iv_kind = 1.
      CREATE OBJECT ro TYPE lcl_add.
    ELSE.
      CREATE OBJECT ro TYPE lcl_mul.
    ENDIF.
  ENDMETHOD.
ENDCLASS.
DATA lo_factory TYPE REF TO lcl_factory.
DATA lo TYPE REF TO lif_calc.
DATA lv TYPE string.
START-OF-SELECTION.
  CREATE OBJECT lo_factory.
  lo = lo_factory->make( iv_kind = 2 ).
  lv = lo->run( ).
  WRITE / lv.
  lo = lo_factory->make( iv_kind = 1 ).
  lv = lo->run( ).
  WRITE / lv.
