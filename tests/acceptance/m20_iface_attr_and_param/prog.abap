REPORT ztest.
INTERFACE lif_store.
  METHODS get RETURNING VALUE(rv) TYPE i.
ENDINTERFACE.
CLASS lcl_const DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_store.
ENDCLASS.
CLASS lcl_const IMPLEMENTATION.
  METHOD lif_store~get.
    rv = 42.
  ENDMETHOD.
ENDCLASS.
CLASS lcl_user DEFINITION.
  PUBLIC SECTION.
    DATA mo_store TYPE REF TO lif_store.
    METHODS use IMPORTING io_store TYPE REF TO lif_store RETURNING VALUE(rv) TYPE i.
    METHODS get_store RETURNING VALUE(ro) TYPE REF TO lif_store.
ENDCLASS.
CLASS lcl_user IMPLEMENTATION.
  METHOD use.
    mo_store = io_store.
    rv = io_store->get( ).
  ENDMETHOD.
  METHOD get_store.
    ro = mo_store.
  ENDMETHOD.
ENDCLASS.
DATA lo_user TYPE REF TO lcl_user.
DATA lo_const TYPE REF TO lcl_const.
DATA lo_store TYPE REF TO lif_store.
DATA lv TYPE i.
START-OF-SELECTION.
  CREATE OBJECT lo_user.
  CREATE OBJECT lo_const.
  lv = lo_user->use( io_store = lo_const ).
  IF lv = 42.
    WRITE / 'param ok'.
  ENDIF.
  lo_store = lo_user->get_store( ).
  lv = lo_store->get( ).
  IF lv = 42.
    WRITE / 'attr ok'.
  ENDIF.
