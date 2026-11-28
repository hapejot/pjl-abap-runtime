* ---------------------------------------------------------------
* Mock fuer lif_dao_kunde (Testhilfe).
*
* WICHTIG: Diese Datei wird vom Programm poc-crm/prog.abap NICHT verwendet und darf dort nicht eingebunden werden.
* Sie wartet auf ABAP Unit (FOR TESTING), das abapc noch nicht kennt. Erst dann wird sie zusammen mit einem
* Testfall uebersetzt, der einen Service mit diesem Mock aufbaut.
*
* Voraussetzung: TYPES zkunde und INTERFACE lif_dao_kunde aus prog.abap (gleicher Vertrag).
* Die Daten liegen in mt_kunden. add haengt an; read_last setzt aufsteigend sortierte Eintraege voraus,
* wie der echte DAO (ORDER BY kunnr).
* ---------------------------------------------------------------
CLASS lcl_dao_kunde_mock DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao_kunde.
    DATA mt_kunden TYPE TABLE OF zkunde.
    METHODS add
      IMPORTING is_kunde TYPE zkunde.
ENDCLASS.

CLASS lcl_dao_kunde_mock IMPLEMENTATION.
  METHOD add.
    APPEND is_kunde TO mt_kunden.
  ENDMETHOD.

  METHOD lif_dao_kunde~read.
    DATA ls_kunde TYPE zkunde.
    ev_found = ' '.
    LOOP AT mt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr = iv_kunnr.
        es_kunde = ls_kunde.
        ev_found = 'X'.
      ENDIF.
    ENDLOOP.
  ENDMETHOD.

  METHOD lif_dao_kunde~read_all.
    et_kunden = mt_kunden.
  ENDMETHOD.

  METHOD lif_dao_kunde~read_last.
    DATA ls_kunde TYPE zkunde.
    ev_found = ' '.
    LOOP AT mt_kunden INTO ls_kunde.
      es_kunde = ls_kunde.
      ev_found = 'X'.
    ENDLOOP.
  ENDMETHOD.

  METHOD lif_dao_kunde~insert.
    DATA ls_kunde TYPE zkunde.
    ev_ok = 'X'.
    LOOP AT mt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr = is_kunde-kunnr.
        ev_ok = ' '.
      ENDIF.
    ENDLOOP.
    IF ev_ok = 'X'.
      APPEND is_kunde TO mt_kunden.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kunde~update.
    DATA lt_neu TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
    ev_ok = ' '.
    LOOP AT mt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr = is_kunde-kunnr.
        ls_kunde = is_kunde.
        ev_ok = 'X'.
      ENDIF.
      APPEND ls_kunde TO lt_neu.
    ENDLOOP.
    mt_kunden = lt_neu.
  ENDMETHOD.

  METHOD lif_dao_kunde~delete.
    DATA lt_neu TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
    ev_ok = ' '.
    LOOP AT mt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr = iv_kunnr.
        ev_ok = 'X'.
      ELSE.
        APPEND ls_kunde TO lt_neu.
      ENDIF.
    ENDLOOP.
    mt_kunden = lt_neu.
  ENDMETHOD.
ENDCLASS.
