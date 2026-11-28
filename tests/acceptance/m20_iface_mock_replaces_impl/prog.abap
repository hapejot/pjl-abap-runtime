REPORT ztest.
INTERFACE lif_dao.
  METHODS select_one IMPORTING iv_id TYPE i RETURNING VALUE(rv_name) TYPE string.
ENDINTERFACE.
CLASS lcl_dao_real DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao.
ENDCLASS.
CLASS lcl_dao_real IMPLEMENTATION.
  METHOD lif_dao~select_one.
    rv_name = 'database'.
  ENDMETHOD.
ENDCLASS.
CLASS lcl_dao_mock DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao.
ENDCLASS.
CLASS lcl_dao_mock IMPLEMENTATION.
  METHOD lif_dao~select_one.
    rv_name = 'mock'.
  ENDMETHOD.
ENDCLASS.
CLASS lcl_service DEFINITION.
  PUBLIC SECTION.
    DATA mo_dao TYPE REF TO lif_dao.
    METHODS constructor IMPORTING io_dao TYPE REF TO lif_dao.
    METHODS load RETURNING VALUE(rv) TYPE string.
ENDCLASS.
CLASS lcl_service IMPLEMENTATION.
  METHOD constructor.
    mo_dao = io_dao.
  ENDMETHOD.
  METHOD load.
    rv = mo_dao->select_one( iv_id = 7 ).
  ENDMETHOD.
ENDCLASS.
DATA lo_svc TYPE REF TO lcl_service.
DATA lo_real TYPE REF TO lcl_dao_real.
DATA lo_mock TYPE REF TO lcl_dao_mock.
DATA lv TYPE string.
START-OF-SELECTION.
  CREATE OBJECT lo_real.
  CREATE OBJECT lo_mock.
  CREATE OBJECT lo_svc EXPORTING io_dao = lo_mock.
  lv = lo_svc->load( ).
  WRITE / lv.
  CREATE OBJECT lo_svc EXPORTING io_dao = lo_real.
  lv = lo_svc->load( ).
  WRITE / lv.
