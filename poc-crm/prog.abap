REPORT zcrm.

* Tabellen: Typname = Tabellenname (wie im Fall m11). Spaltenfolge wie in db.sql.
TYPES: BEGIN OF zkunde,
         kunnr   TYPE i,
         name    TYPE c LENGTH 30,
         ort     TYPE c LENGTH 30,
         branche TYPE c LENGTH 20,
       END OF zkunde.
TYPES: BEGIN OF zkontakt,
         knr      TYPE i,
         kunnr    TYPE i,
         vorname  TYPE c LENGTH 20,
         nachname TYPE c LENGTH 30,
         email    TYPE c LENGTH 40,
         telefon  TYPE c LENGTH 20,
       END OF zkontakt.
TYPES: BEGIN OF zchance,
         cnr       TYPE i,
         kunnr     TYPE i,
         titel     TYPE c LENGTH 30,
         wert      TYPE i,
         phase     TYPE c LENGTH 10,
         abschluss TYPE c LENGTH 8,
       END OF zchance.

* Bildschirmzustand: alle Felder der Dynpros. Die Dynpro-Felder (unten) sind nur die Bruecke dorthin.
TYPES: BEGIN OF ty_maske,
         kunnr     TYPE i,
         name      TYPE c LENGTH 30,
         ort       TYPE c LENGTH 30,
         branche   TYPE c LENGTH 20,
         nkont     TYPE i,
         nchance   TYPE i,
         info      TYPE c LENGTH 70,
         knr       TYPE i,
         vorname   TYPE c LENGTH 20,
         nachname  TYPE c LENGTH 30,
         email     TYPE c LENGTH 40,
         telefon   TYPE c LENGTH 20,
         kpos      TYPE c LENGTH 20,
         cnr       TYPE i,
         titel     TYPE c LENGTH 30,
         wert      TYPE i,
         phase     TYPE c LENGTH 10,
         abschluss TYPE c LENGTH 8,
         cpos      TYPE c LENGTH 20,
         frage     TYPE c LENGTH 56,
       END OF ty_maske.

* Ausnahme der Validierung. Der Text steht im Attribut.
CLASS lcx_crm DEFINITION INHERITING FROM cx_static_check.
  PUBLIC SECTION.
    DATA mv_text TYPE c LENGTH 70.
ENDCLASS.

CLASS lcx_crm IMPLEMENTATION.
ENDCLASS.

* Validierung der Eingaben. Ungueltige Werte loesen lcx_crm aus. Keine Datenbank, kein Dynpro.
CLASS lcl_validator DEFINITION.
  PUBLIC SECTION.
    METHODS check_kunde
      IMPORTING iv_name TYPE c LENGTH 30
      RAISING lcx_crm.
    METHODS check_kontakt
      IMPORTING iv_nachname TYPE c LENGTH 30
                iv_email TYPE c LENGTH 40
      RAISING lcx_crm.
    METHODS check_chance
      IMPORTING iv_titel TYPE c LENGTH 30
                iv_wert TYPE i
                iv_phase TYPE c LENGTH 10
                iv_abschluss TYPE c LENGTH 8
                iv_alt TYPE c LENGTH 10
      RAISING lcx_crm.
ENDCLASS.

CLASS lcl_validator IMPLEMENTATION.
  METHOD check_kunde.
    DATA lx TYPE REF TO lcx_crm.
    IF iv_name IS INITIAL.
      CREATE OBJECT lx.
      lx->mv_text = 'Name ist ein Pflichtfeld.'.
      RAISE EXCEPTION lx.
    ENDIF.
  ENDMETHOD.

  METHOD check_kontakt.
    DATA lx TYPE REF TO lcx_crm.
    IF iv_nachname IS INITIAL.
      CREATE OBJECT lx.
      lx->mv_text = 'Nachname fehlt.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF NOT iv_email CS '@'.
      CREATE OBJECT lx.
      lx->mv_text = 'E-Mail muss ein @ enthalten.'.
      RAISE EXCEPTION lx.
    ENDIF.
  ENDMETHOD.

  METHOD check_chance.
    DATA lx TYPE REF TO lcx_crm.
    IF iv_titel IS INITIAL.
      CREATE OBJECT lx.
      lx->mv_text = 'Titel fehlt.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_wert <= 0.
      CREATE OBJECT lx.
      lx->mv_text = 'Wert muss groesser 0 sein.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_phase <> 'offen' AND iv_phase <> 'gewonnen' AND iv_phase <> 'verloren'.
      CREATE OBJECT lx.
      lx->mv_text = 'Phase muss offen, gewonnen oder verloren sein.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_alt <> 'offen' AND iv_phase = 'offen'.
      CREATE OBJECT lx.
      lx->mv_text = 'Abgeschlossene Chance kann nicht wieder geoeffnet werden.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_phase <> 'offen' AND iv_abschluss IS INITIAL.
      CREATE OBJECT lx.
      lx->mv_text = 'Abschlussdatum fehlt.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_abschluss CN '0123456789 '.
      CREATE OBJECT lx.
      lx->mv_text = 'Abschlussdatum muss JJJJMMTT sein.'.
      RAISE EXCEPTION lx.
    ENDIF.
    IF iv_abschluss IS NOT INITIAL AND iv_abschluss+7(1) = ' '.
      CREATE OBJECT lx.
      lx->mv_text = 'Abschlussdatum hat 8 Ziffern (JJJJMMTT).'.
      RAISE EXCEPTION lx.
    ENDIF.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* DAO Kunde: einziger Ort mit SQL fuer zkunde. Liefert immer einzelne Saetze (keine Tabellen-Parameter).
* Kein COMMIT WORK hier (das macht der Service).
* WORKAROUND FR-001: Interface lif_dao_kunde und Factory fehlen. Services erhalten diese konkrete Klasse.
* WORKAROUND FR-002: Tabellen als Parameter sind nicht unterstuetzt. Listen entstehen im Service durch Schleifen.
* ---------------------------------------------------------------
CLASS lcl_dao_kunde DEFINITION.
  PUBLIC SECTION.
    METHODS select_one
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS select_first
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS select_next
      IMPORTING iv_after TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS select_prev
      IMPORTING iv_before TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS select_last
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS insert_one
      IMPORTING is_kunde TYPE zkunde
      EXPORTING ev_ok TYPE c.
    METHODS update_one
      IMPORTING is_kunde TYPE zkunde
      EXPORTING ev_ok TYPE c.
    METHODS delete_one
      IMPORTING iv_kunnr TYPE i
      EXPORTING ev_ok TYPE c.
ENDCLASS.

CLASS lcl_dao_kunde IMPLEMENTATION.
  METHOD select_one.
    SELECT SINGLE * FROM zkunde INTO es_kunde WHERE kunnr = @iv_kunnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_first.
    SELECT SINGLE * FROM zkunde INTO es_kunde ORDER BY kunnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_next.
    SELECT SINGLE * FROM zkunde INTO es_kunde WHERE kunnr > @iv_after ORDER BY kunnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_prev.
    SELECT SINGLE * FROM zkunde INTO es_kunde WHERE kunnr < @iv_before ORDER BY kunnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_last.
    SELECT SINGLE * FROM zkunde INTO es_kunde ORDER BY kunnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD insert_one.
    INSERT zkunde FROM is_kunde.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD update_one.
    UPDATE zkunde FROM is_kunde.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_one.
    DELETE FROM zkunde WHERE kunnr = @iv_kunnr.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* DAO Kontakt: einziger Ort mit SQL fuer zkontakt.
* WORKAROUND FR-001: siehe lcl_dao_kunde.
* ---------------------------------------------------------------
CLASS lcl_dao_kontakt DEFINITION.
  PUBLIC SECTION.
    METHODS select_one
      IMPORTING iv_knr TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS select_first_for_kunde
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS select_next_for_kunde
      IMPORTING iv_kunnr TYPE i
                iv_after TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS select_prev_for_kunde
      IMPORTING iv_kunnr TYPE i
                iv_before TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS select_last
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS insert_one
      IMPORTING is_kontakt TYPE zkontakt
      EXPORTING ev_ok TYPE c.
    METHODS update_one
      IMPORTING is_kontakt TYPE zkontakt
      EXPORTING ev_ok TYPE c.
    METHODS delete_one
      IMPORTING iv_knr TYPE i.
    METHODS delete_for_kunde
      IMPORTING iv_kunnr TYPE i.
ENDCLASS.

CLASS lcl_dao_kontakt IMPLEMENTATION.
  METHOD select_one.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt WHERE knr = @iv_knr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_first_for_kunde.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt WHERE kunnr = @iv_kunnr ORDER BY knr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_next_for_kunde.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt WHERE kunnr = @iv_kunnr AND knr > @iv_after ORDER BY knr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_prev_for_kunde.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt WHERE kunnr = @iv_kunnr AND knr < @iv_before ORDER BY knr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_last.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt ORDER BY knr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD insert_one.
    INSERT zkontakt FROM is_kontakt.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD update_one.
    UPDATE zkontakt FROM is_kontakt.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_one.
    DELETE FROM zkontakt WHERE knr = @iv_knr.
  ENDMETHOD.

  METHOD delete_for_kunde.
    DELETE FROM zkontakt WHERE kunnr = @iv_kunnr.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* DAO Verkaufschance: einziger Ort mit SQL fuer zchance.
* WORKAROUND FR-001: siehe lcl_dao_kunde.
* ---------------------------------------------------------------
CLASS lcl_dao_chance DEFINITION.
  PUBLIC SECTION.
    METHODS select_one
      IMPORTING iv_cnr TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_first_for_kunde
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_next_for_kunde
      IMPORTING iv_kunnr TYPE i
                iv_after TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_prev_for_kunde
      IMPORTING iv_kunnr TYPE i
                iv_before TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_first
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_next
      IMPORTING iv_after TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS select_last
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS insert_one
      IMPORTING is_chance TYPE zchance
      EXPORTING ev_ok TYPE c.
    METHODS update_one
      IMPORTING is_chance TYPE zchance
      EXPORTING ev_ok TYPE c.
    METHODS delete_one
      IMPORTING iv_cnr TYPE i.
    METHODS delete_for_kunde
      IMPORTING iv_kunnr TYPE i.
ENDCLASS.

CLASS lcl_dao_chance IMPLEMENTATION.
  METHOD select_one.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE cnr = @iv_cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_first_for_kunde.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE kunnr = @iv_kunnr ORDER BY cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_next_for_kunde.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE kunnr = @iv_kunnr AND cnr > @iv_after ORDER BY cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_prev_for_kunde.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE kunnr = @iv_kunnr AND cnr < @iv_before ORDER BY cnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_first.
    SELECT SINGLE * FROM zchance INTO es_chance ORDER BY cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_next.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE cnr > @iv_after ORDER BY cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD select_last.
    SELECT SINGLE * FROM zchance INTO es_chance ORDER BY cnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD insert_one.
    INSERT zchance FROM is_chance.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD update_one.
    UPDATE zchance FROM is_chance.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_one.
    DELETE FROM zchance WHERE cnr = @iv_cnr.
  ENDMETHOD.

  METHOD delete_for_kunde.
    DELETE FROM zchance WHERE kunnr = @iv_kunnr.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* Service: Anwendungsfaelle und Transaktionsgrenzen (LUW). Kein Dynpro, keine Anzeigetexte.
* Listen entstehen hier durch Schleifen ueber Einzelsaetze der DAOs.
* Status-Codes beim Speichern: 'O' = ok, 'U' = Satz fehlt beim Aendern, 'I' = Schluessel ist schon vergeben.
* ---------------------------------------------------------------
CLASS lcl_crm_service DEFINITION.
  PUBLIC SECTION.
    DATA mo_kunde TYPE REF TO lcl_dao_kunde.
    DATA mo_kontakt TYPE REF TO lcl_dao_kontakt.
    DATA mo_chance TYPE REF TO lcl_dao_chance.
    METHODS constructor
      IMPORTING io_kunde TYPE REF TO lcl_dao_kunde
                io_kontakt TYPE REF TO lcl_dao_kontakt
                io_chance TYPE REF TO lcl_dao_chance.
    METHODS read_kunde
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS first_kunde
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS next_kunde
      IMPORTING iv_after TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS prev_kunde
      IMPORTING iv_before TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS next_kunnr
      RETURNING VALUE(rv_next) TYPE i.
    METHODS save_kunde
      IMPORTING is_kunde TYPE zkunde
                iv_neu TYPE c
      EXPORTING ev_status TYPE c.
    METHODS delete_kunde
      IMPORTING iv_kunnr TYPE i
      EXPORTING ev_ok TYPE c.
    METHODS read_kontakt
      IMPORTING iv_knr TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS first_kontakt
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS next_kontakt
      IMPORTING iv_kunnr TYPE i
                iv_after TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS prev_kontakt
      IMPORTING iv_kunnr TYPE i
                iv_before TYPE i
      EXPORTING es_kontakt TYPE zkontakt
                ev_found TYPE c.
    METHODS count_kontakte
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_n) TYPE i.
    METHODS position_kontakt
      IMPORTING iv_kunnr TYPE i
                iv_knr TYPE i
      RETURNING VALUE(rv_pos) TYPE i.
    METHODS next_knr
      RETURNING VALUE(rv_next) TYPE i.
    METHODS save_kontakt
      IMPORTING is_kontakt TYPE zkontakt
                iv_neu TYPE c
      EXPORTING ev_status TYPE c.
    METHODS delete_kontakt
      IMPORTING iv_knr TYPE i.
    METHODS read_chance
      IMPORTING iv_cnr TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS first_chance
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS next_chance
      IMPORTING iv_kunnr TYPE i
                iv_after TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS prev_chance
      IMPORTING iv_kunnr TYPE i
                iv_before TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS count_chancen
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_n) TYPE i.
    METHODS position_chance
      IMPORTING iv_kunnr TYPE i
                iv_cnr TYPE i
      RETURNING VALUE(rv_pos) TYPE i.
    METHODS next_cnr
      RETURNING VALUE(rv_next) TYPE i.
    METHODS save_chance
      IMPORTING is_chance TYPE zchance
                iv_neu TYPE c
      EXPORTING ev_status TYPE c.
    METHODS delete_chance
      IMPORTING iv_cnr TYPE i.
    METHODS first_chance_any
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS next_chance_any
      IMPORTING iv_after TYPE i
      EXPORTING es_chance TYPE zchance
                ev_found TYPE c.
    METHODS sum_won_for_kunde
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_sum) TYPE i.
ENDCLASS.

CLASS lcl_crm_service IMPLEMENTATION.
  METHOD constructor.
    mo_kunde = io_kunde.
    mo_kontakt = io_kontakt.
    mo_chance = io_chance.
  ENDMETHOD.

  METHOD read_kunde.
    mo_kunde->select_one( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kunde = es_kunde ev_found = ev_found ).
  ENDMETHOD.

  METHOD first_kunde.
    mo_kunde->select_first( IMPORTING es_kunde = es_kunde ev_found = ev_found ).
  ENDMETHOD.

  METHOD next_kunde.
    mo_kunde->select_next( EXPORTING iv_after = iv_after IMPORTING es_kunde = es_kunde ev_found = ev_found ).
  ENDMETHOD.

  METHOD prev_kunde.
    mo_kunde->select_prev( EXPORTING iv_before = iv_before IMPORTING es_kunde = es_kunde ev_found = ev_found ).
  ENDMETHOD.

  METHOD next_kunnr.
    DATA ls_kunde TYPE zkunde.
    DATA lv_found TYPE c.
    mo_kunde->select_last( IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_kunde-kunnr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_kunde.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_kunde->insert_one( EXPORTING is_kunde = is_kunde IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_kunde->update_one( EXPORTING is_kunde = is_kunde IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'U'.
      ENDIF.
    ENDIF.
  ENDMETHOD.

  METHOD delete_kunde.
    DATA lv_ok TYPE c.
    " Logische Arbeitseinheit: Chancen, Kontakte und Kunde werden zusammen geloescht.
    mo_chance->delete_for_kunde( iv_kunnr = iv_kunnr ).
    mo_kontakt->delete_for_kunde( iv_kunnr = iv_kunnr ).
    mo_kunde->delete_one( EXPORTING iv_kunnr = iv_kunnr IMPORTING ev_ok = lv_ok ).
    IF lv_ok = 'X'.
      COMMIT WORK.
      ev_ok = 'X'.
    ELSE.
      ROLLBACK WORK.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD read_kontakt.
    mo_kontakt->select_one( EXPORTING iv_knr = iv_knr IMPORTING es_kontakt = es_kontakt ev_found = ev_found ).
  ENDMETHOD.

  METHOD first_kontakt.
    mo_kontakt->select_first_for_kunde( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kontakt = es_kontakt ev_found = ev_found ).
  ENDMETHOD.

  METHOD next_kontakt.
    mo_kontakt->select_next_for_kunde( EXPORTING iv_kunnr = iv_kunnr iv_after = iv_after
                                       IMPORTING es_kontakt = es_kontakt ev_found = ev_found ).
  ENDMETHOD.

  METHOD prev_kontakt.
    mo_kontakt->select_prev_for_kunde( EXPORTING iv_kunnr = iv_kunnr iv_before = iv_before
                                       IMPORTING es_kontakt = es_kontakt ev_found = ev_found ).
  ENDMETHOD.

  METHOD count_kontakte.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    rv_n = 0.
    first_kontakt( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    WHILE lv_found = 'X'.
      rv_n = rv_n + 1.
      next_kontakt( EXPORTING iv_kunnr = iv_kunnr iv_after = ls_kontakt-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.

  METHOD position_kontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    rv_pos = 0.
    first_kontakt( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    WHILE lv_found = 'X' AND ls_kontakt-knr <= iv_knr.
      rv_pos = rv_pos + 1.
      next_kontakt( EXPORTING iv_kunnr = iv_kunnr iv_after = ls_kontakt-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.

  METHOD next_knr.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    mo_kontakt->select_last( IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_kontakt-knr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_kontakt.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_kontakt->insert_one( EXPORTING is_kontakt = is_kontakt IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_kontakt->update_one( EXPORTING is_kontakt = is_kontakt IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'U'.
      ENDIF.
    ENDIF.
  ENDMETHOD.

  METHOD delete_kontakt.
    mo_kontakt->delete_one( iv_knr = iv_knr ).
    COMMIT WORK.
  ENDMETHOD.

  METHOD read_chance.
    mo_chance->select_one( EXPORTING iv_cnr = iv_cnr IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD first_chance.
    mo_chance->select_first_for_kunde( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD next_chance.
    mo_chance->select_next_for_kunde( EXPORTING iv_kunnr = iv_kunnr iv_after = iv_after
                                      IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD prev_chance.
    mo_chance->select_prev_for_kunde( EXPORTING iv_kunnr = iv_kunnr iv_before = iv_before
                                      IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD count_chancen.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    rv_n = 0.
    first_chance( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    WHILE lv_found = 'X'.
      rv_n = rv_n + 1.
      next_chance( EXPORTING iv_kunnr = iv_kunnr iv_after = ls_chance-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.

  METHOD position_chance.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    rv_pos = 0.
    first_chance( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    WHILE lv_found = 'X' AND ls_chance-cnr <= iv_cnr.
      rv_pos = rv_pos + 1.
      next_chance( EXPORTING iv_kunnr = iv_kunnr iv_after = ls_chance-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.

  METHOD next_cnr.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    mo_chance->select_last( IMPORTING es_chance = ls_chance ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_chance-cnr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_chance.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_chance->insert_one( EXPORTING is_chance = is_chance IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_chance->update_one( EXPORTING is_chance = is_chance IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'U'.
      ENDIF.
    ENDIF.
  ENDMETHOD.

  METHOD delete_chance.
    mo_chance->delete_one( iv_cnr = iv_cnr ).
    COMMIT WORK.
  ENDMETHOD.

  METHOD first_chance_any.
    mo_chance->select_first( IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD next_chance_any.
    mo_chance->select_next( EXPORTING iv_after = iv_after IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD sum_won_for_kunde.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    rv_sum = 0.
    first_chance( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    WHILE lv_found = 'X'.
      IF ls_chance-phase = 'gewonnen'.
        rv_sum = rv_sum + ls_chance-wert.
      ENDIF.
      next_chance( EXPORTING iv_kunnr = iv_kunnr iv_after = ls_chance-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* View-Model: Zustand der Bildschirme und Entscheidungen zu Tasten. Kein SQL, kein Dynpro, kein CALL SCREEN.
* Rueckgabe 'E' = Eingabefehler; das Modul gibt dann die Meldung aus.
* Die Position in Listen (Kontakte, Chancen) wird aus den Daten berechnet, nicht im Objekt gehalten.
* ---------------------------------------------------------------
CLASS lcl_crm_view DEFINITION.
  PUBLIC SECTION.
    DATA ms_maske TYPE ty_maske.
    DATA mo_service TYPE REF TO lcl_crm_service.
    DATA mo_val TYPE REF TO lcl_validator.
    DATA mv_kunnr TYPE i.
    DATA mv_kmode TYPE c LENGTH 1.
    DATA mv_kon TYPE c LENGTH 1.
    DATA mv_kmod TYPE c LENGTH 1.
    DATA mv_con TYPE c LENGTH 1.
    DATA mv_cmod TYPE c LENGTH 1.
    DATA mv_phase_alt TYPE c LENGTH 10.

    METHODS constructor
      IMPORTING io_service TYPE REF TO lcl_crm_service.
    METHODS get_maske
      EXPORTING es_maske TYPE ty_maske.
    METHODS set_maske
      IMPORTING is_maske TYPE ty_maske.
    METHODS set_info
      IMPORTING iv_info TYPE c LENGTH 70.
    METHODS clear_info.
    METHODS start.
    METHODS read_kunde
      IMPORTING iv_kunnr TYPE i.
    METHODS clear_kunde.
    METHODS refresh_counts.
    METHODS handle_0100
      IMPORTING iv_ucomm TYPE c LENGTH 70
      RETURNING VALUE(rv_error) TYPE c.
    METHODS can_delete_kunde
      RETURNING VALUE(rv_ok) TYPE c.
    METHODS delete_kunde.
    METHODS open_kontakte
      RETURNING VALUE(rv_ok) TYPE c.
    METHODS handle_0200
      IMPORTING iv_ucomm TYPE c LENGTH 70
      RETURNING VALUE(rv_error) TYPE c.
    METHODS can_delete_kontakt
      RETURNING VALUE(rv_ok) TYPE c.
    METHODS delete_kontakt.
    METHODS can_delete_chance
      RETURNING VALUE(rv_ok) TYPE c.
    METHODS delete_chance.
    METHODS cycle_phase.
    METHODS load_kontakte_view.
    METHODS show_kontakt.
    METHODS move_kontakt
      IMPORTING iv_step TYPE i.
    METHODS new_kontakt.
    METHODS load_chancen_view.
    METHODS show_chance.
    METHODS move_chance
      IMPORTING iv_step TYPE i.
    METHODS new_chance.
    METHODS save_0200
      RETURNING VALUE(rv_error) TYPE c.
ENDCLASS.

CLASS lcl_crm_view IMPLEMENTATION.
  METHOD constructor.
    mo_service = io_service.
    CREATE OBJECT mo_val.
    mv_kmode = 'N'.
    mv_kon = ' '.
    mv_kmod = 'N'.
    mv_con = ' '.
    mv_cmod = 'N'.
    mv_phase_alt = 'offen'.
  ENDMETHOD.

  METHOD get_maske.
    es_maske = ms_maske.
  ENDMETHOD.

  METHOD set_maske.
    ms_maske = is_maske.
  ENDMETHOD.

  METHOD set_info.
    ms_maske-info = iv_info.
  ENDMETHOD.

  METHOD clear_info.
    ms_maske-info = ' '.
  ENDMETHOD.

  METHOD start.
    DATA ls_kunde TYPE zkunde.
    DATA lv_found TYPE c.
    DATA lv_kunnr TYPE i.
    mo_service->first_kunde( IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    lv_kunnr = 0.
    IF lv_found = 'X'.
      lv_kunnr = ls_kunde-kunnr.
    ENDIF.
    read_kunde( lv_kunnr ).
  ENDMETHOD.

  METHOD read_kunde.
    DATA ls_kunde TYPE zkunde.
    DATA lv_found TYPE c.
    mo_service->read_kunde( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    mv_kunnr = iv_kunnr.
    ms_maske-kunnr = iv_kunnr.
    IF lv_found = 'X'.
      mv_kmode = 'L'.
      ms_maske-name = ls_kunde-name.
      ms_maske-ort = ls_kunde-ort.
      ms_maske-branche = ls_kunde-branche.
      ms_maske-info = ' '.
    ELSE.
      mv_kmode = 'N'.
      ms_maske-info = 'Kunde nicht vorhanden: Daten eingeben und mit F11 anlegen.'.
    ENDIF.
  ENDMETHOD.

  METHOD clear_kunde.
    mv_kunnr = 0.
    mv_kmode = 'N'.
    mv_kon = ' '.
    mv_con = ' '.
    ms_maske-kunnr = 0.
    CLEAR ms_maske-name.
    CLEAR ms_maske-ort.
    CLEAR ms_maske-branche.
    ms_maske-nkont = 0.
    ms_maske-nchance = 0.
  ENDMETHOD.

  METHOD refresh_counts.
    ms_maske-nkont = 0.
    ms_maske-nchance = 0.
    IF mv_kmode = 'L'.
      ms_maske-nkont = mo_service->count_kontakte( iv_kunnr = mv_kunnr ).
      ms_maske-nchance = mo_service->count_chancen( iv_kunnr = mv_kunnr ).
    ENDIF.
  ENDMETHOD.

  METHOD handle_0100.
    DATA lx TYPE REF TO lcx_crm.
    DATA ls_kunde TYPE zkunde.
    DATA lv_found TYPE c.
    DATA ls_neu TYPE zkunde.
    DATA lv_status TYPE c.
    rv_error = ' '.
    CASE iv_ucomm.
      WHEN 'CANCEL'.
        IF mv_kmode = 'L'.
          read_kunde( mv_kunnr ).
          ms_maske-info = 'Eingaben verworfen.'.
        ELSE.
          clear_kunde( ).
          ms_maske-info = 'Eingaben verworfen.'.
        ENDIF.
      WHEN 'PREV'.
        mo_service->prev_kunde( EXPORTING iv_before = mv_kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
        IF lv_found = 'X'.
          read_kunde( ls_kunde-kunnr ).
        ELSE.
          ms_maske-info = 'Erster Kunde erreicht.'.
        ENDIF.
      WHEN 'NEXT'.
        mo_service->next_kunde( EXPORTING iv_after = mv_kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
        IF lv_found = 'X'.
          read_kunde( ls_kunde-kunnr ).
        ELSE.
          ms_maske-info = 'Letzter Kunde erreicht.'.
        ENDIF.
      WHEN 'NEW'.
        clear_kunde( ).
        ms_maske-info = 'Neuer Kunde: Daten eingeben, dann F11 sichern.'.
      WHEN 'SAVE'.
        TRY.
            mo_val->check_kunde( ms_maske-name ).
            IF mv_kmode = 'N' AND ms_maske-kunnr = 0.
              ms_maske-kunnr = mo_service->next_kunnr( ).
            ENDIF.
            ls_neu-kunnr = ms_maske-kunnr.
            ls_neu-name = ms_maske-name.
            ls_neu-ort = ms_maske-ort.
            ls_neu-branche = ms_maske-branche.
            IF mv_kmode = 'L'.
              mo_service->save_kunde( EXPORTING is_kunde = ls_neu iv_neu = ' ' IMPORTING ev_status = lv_status ).
              IF lv_status = 'O'.
                ms_maske-info = 'Kunde gespeichert.'.
              ELSE.
                ms_maske-info = 'Kunde ist nicht mehr vorhanden.'.
              ENDIF.
            ELSE.
              mo_service->save_kunde( EXPORTING is_kunde = ls_neu iv_neu = 'X' IMPORTING ev_status = lv_status ).
              IF lv_status = 'O'.
                mv_kunnr = ms_maske-kunnr.
                mv_kmode = 'L'.
                ms_maske-info = 'Kunde angelegt.'.
              ELSE.
                ms_maske-info = 'Kundennummer ist bereits vergeben.'.
              ENDIF.
            ENDIF.
          CATCH lcx_crm INTO lx.
            ms_maske-info = lx->mv_text.
            rv_error = 'E'.
        ENDTRY.
      WHEN OTHERS.
        IF ms_maske-kunnr <> mv_kunnr.
          read_kunde( ms_maske-kunnr ).
        ENDIF.
    ENDCASE.
  ENDMETHOD.

  METHOD can_delete_kunde.
    IF mv_kmode = 'L'.
      ms_maske-frage = 'Kunde samt Kontakten und Chancen löschen?'.
      rv_ok = 'X'.
    ELSE.
      ms_maske-info = 'Kein gespeicherter Kunde ausgewählt.'.
      rv_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_kunde.
    DATA lv_ok TYPE c.
    mo_service->delete_kunde( EXPORTING iv_kunnr = mv_kunnr IMPORTING ev_ok = lv_ok ).
    IF lv_ok = 'X'.
      clear_kunde( ).
      ms_maske-info = 'Kunde mit Kontakten und Verkaufschancen gelöscht.'.
    ELSE.
      ms_maske-info = 'Kunde nicht gefunden, nichts gelöscht.'.
    ENDIF.
  ENDMETHOD.

  METHOD open_kontakte.
    IF mv_kmode = 'L'.
      ms_maske-knr = 0.
      ms_maske-cnr = 0.
      ms_maske-info = ' '.
      load_kontakte_view( ).
      load_chancen_view( ).
      rv_ok = 'X'.
    ELSE.
      ms_maske-info = 'Kunde zuerst mit F11 sichern.'.
      rv_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD handle_0200.
    rv_error = ' '.
    CASE iv_ucomm.
      WHEN 'KPREV'.
        move_kontakt( -1 ).
      WHEN 'KNEXT'.
        move_kontakt( 1 ).
      WHEN 'KNEW'.
        new_kontakt( ).
      WHEN 'CPREV'.
        move_chance( -1 ).
      WHEN 'CNEXT'.
        move_chance( 1 ).
      WHEN 'CNEW'.
        new_chance( ).
      WHEN 'SAVE'.
        rv_error = save_0200( ).
      WHEN OTHERS.
        ms_maske-info = ' '.
    ENDCASE.
  ENDMETHOD.

  METHOD can_delete_kontakt.
    IF mv_kon = 'X' AND mv_kmod = 'L'.
      ms_maske-frage = 'Kontakt wirklich löschen?'.
      rv_ok = 'X'.
    ELSE.
      ms_maske-info = 'Kein gespeicherter Kontakt ausgewählt.'.
      rv_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_kontakt.
    mo_service->delete_kontakt( iv_knr = ms_maske-knr ).
    ms_maske-info = 'Kontakt gelöscht.'.
    ms_maske-knr = 0.
    load_kontakte_view( ).
  ENDMETHOD.

  METHOD can_delete_chance.
    IF mv_con = 'X' AND mv_cmod = 'L'.
      ms_maske-frage = 'Verkaufschance wirklich löschen?'.
      rv_ok = 'X'.
    ELSE.
      ms_maske-info = 'Keine gespeicherte Verkaufschance ausgewählt.'.
      rv_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD delete_chance.
    mo_service->delete_chance( iv_cnr = ms_maske-cnr ).
    ms_maske-info = 'Verkaufschance gelöscht.'.
    ms_maske-cnr = 0.
    load_chancen_view( ).
  ENDMETHOD.

  METHOD cycle_phase.
    IF ms_maske-phase = 'offen'.
      ms_maske-phase = 'gewonnen'.
    ELSEIF ms_maske-phase = 'gewonnen'.
      ms_maske-phase = 'verloren'.
    ELSE.
      ms_maske-phase = 'offen'.
    ENDIF.
    ms_maske-info = 'Gültige Werte: offen, gewonnen, verloren.'.
  ENDMETHOD.

  METHOD load_kontakte_view.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    DATA lv_n TYPE i.
    lv_n = mo_service->count_kontakte( iv_kunnr = mv_kunnr ).
    IF lv_n = 0.
      ms_maske-knr = 0.
    ELSE.
      " Die gemerkte Nummer bleibt, wenn sie zum Kunden gehoert. Sonst wird der erste Kontakt gewaehlt.
      mo_service->read_kontakt( EXPORTING iv_knr = ms_maske-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
      IF lv_found <> 'X' OR ls_kontakt-kunnr <> mv_kunnr.
        mo_service->first_kontakt( EXPORTING iv_kunnr = mv_kunnr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
        ms_maske-knr = ls_kontakt-knr.
      ENDIF.
    ENDIF.
    show_kontakt( ).
  ENDMETHOD.

  METHOD show_kontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    DATA lv_pos TYPE i.
    DATA lv_n TYPE i.
    DATA lv_idx TYPE c LENGTH 10.
    DATA lv_len TYPE c LENGTH 10.
    IF ms_maske-knr <> 0.
      mo_service->read_kontakt( EXPORTING iv_knr = ms_maske-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X' AND ls_kontakt-kunnr = mv_kunnr.
      lv_pos = mo_service->position_kontakt( iv_kunnr = mv_kunnr iv_knr = ls_kontakt-knr ).
      lv_n = mo_service->count_kontakte( iv_kunnr = mv_kunnr ).
      ms_maske-knr = ls_kontakt-knr.
      ms_maske-vorname = ls_kontakt-vorname.
      ms_maske-nachname = ls_kontakt-nachname.
      ms_maske-email = ls_kontakt-email.
      ms_maske-telefon = ls_kontakt-telefon.
      ms_maske-kpos = |{ lv_pos } von { lv_n }|.
      mv_kon = 'X'.
      mv_kmod = 'L'.
    ELSE.
      ms_maske-knr = 0.
      CLEAR ms_maske-vorname.
      CLEAR ms_maske-nachname.
      CLEAR ms_maske-email.
      CLEAR ms_maske-telefon.
      ms_maske-kpos = 'keine Kontakte'.
      mv_kon = ' '.
      mv_kmod = 'N'.
    ENDIF.
  ENDMETHOD.

  METHOD move_kontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    IF iv_step > 0.
      mo_service->next_kontakt( EXPORTING iv_kunnr = mv_kunnr iv_after = ms_maske-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ELSE.
      mo_service->prev_kontakt( EXPORTING iv_kunnr = mv_kunnr iv_before = ms_maske-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X'.
      ms_maske-knr = ls_kontakt-knr.
      show_kontakt( ).
      ms_maske-info = ' '.
    ELSE.
      ms_maske-info = 'Kein weiterer Kontakt in dieser Richtung.'.
    ENDIF.
  ENDMETHOD.

  METHOD new_kontakt.
    mv_kon = 'X'.
    mv_kmod = 'N'.
    ms_maske-knr = 0.
    CLEAR ms_maske-vorname.
    CLEAR ms_maske-nachname.
    CLEAR ms_maske-email.
    CLEAR ms_maske-telefon.
    ms_maske-kpos = 'neu'.
    ms_maske-info = ' '.
  ENDMETHOD.

  METHOD load_chancen_view.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_n TYPE i.
    lv_n = mo_service->count_chancen( iv_kunnr = mv_kunnr ).
    IF lv_n = 0.
      ms_maske-cnr = 0.
    ELSE.
      mo_service->read_chance( EXPORTING iv_cnr = ms_maske-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
      IF lv_found <> 'X' OR ls_chance-kunnr <> mv_kunnr.
        mo_service->first_chance( EXPORTING iv_kunnr = mv_kunnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
        ms_maske-cnr = ls_chance-cnr.
      ENDIF.
    ENDIF.
    show_chance( ).
  ENDMETHOD.

  METHOD show_chance.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_pos TYPE i.
    DATA lv_n TYPE i.
    IF ms_maske-cnr <> 0.
      mo_service->read_chance( EXPORTING iv_cnr = ms_maske-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X' AND ls_chance-kunnr = mv_kunnr.
      lv_pos = mo_service->position_chance( iv_kunnr = mv_kunnr iv_cnr = ls_chance-cnr ).
      lv_n = mo_service->count_chancen( iv_kunnr = mv_kunnr ).
      ms_maske-cnr = ls_chance-cnr.
      ms_maske-titel = ls_chance-titel.
      ms_maske-wert = ls_chance-wert.
      ms_maske-phase = ls_chance-phase.
      ms_maske-abschluss = ls_chance-abschluss.
      mv_phase_alt = ls_chance-phase.
      ms_maske-cpos = |{ lv_pos } von { lv_n }|.
      mv_con = 'X'.
      mv_cmod = 'L'.
    ELSE.
      ms_maske-cnr = 0.
      CLEAR ms_maske-titel.
      ms_maske-wert = 0.
      CLEAR ms_maske-phase.
      CLEAR ms_maske-abschluss.
      mv_phase_alt = 'offen'.
      ms_maske-cpos = 'keine Chancen'.
      mv_con = ' '.
      mv_cmod = 'N'.
    ENDIF.
  ENDMETHOD.

  METHOD move_chance.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    IF iv_step > 0.
      mo_service->next_chance( EXPORTING iv_kunnr = mv_kunnr iv_after = ms_maske-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ELSE.
      mo_service->prev_chance( EXPORTING iv_kunnr = mv_kunnr iv_before = ms_maske-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X'.
      ms_maske-cnr = ls_chance-cnr.
      show_chance( ).
      ms_maske-info = ' '.
    ELSE.
      ms_maske-info = 'Keine weitere Verkaufschance in dieser Richtung.'.
    ENDIF.
  ENDMETHOD.

  METHOD new_chance.
    mv_con = 'X'.
    mv_cmod = 'N'.
    ms_maske-cnr = 0.
    CLEAR ms_maske-titel.
    ms_maske-wert = 0.
    ms_maske-phase = 'offen'.
    CLEAR ms_maske-abschluss.
    mv_phase_alt = 'offen'.
    ms_maske-cpos = 'neu'.
    ms_maske-info = ' '.
  ENDMETHOD.

  METHOD save_0200.
    DATA lx TYPE REF TO lcx_crm.
    DATA ls_kontakt TYPE zkontakt.
    DATA ls_chance TYPE zchance.
    DATA lv_status TYPE c.
    rv_error = ' '.
    TRY.
        IF mv_kon = 'X'.
          mo_val->check_kontakt( iv_nachname = ms_maske-nachname iv_email = ms_maske-email ).
        ENDIF.
        IF mv_con = 'X'.
          mo_val->check_chance( iv_titel = ms_maske-titel iv_wert = ms_maske-wert
                                iv_phase = ms_maske-phase iv_abschluss = ms_maske-abschluss
                                iv_alt = mv_phase_alt ).
        ENDIF.
        IF mv_kon = 'X'.
          IF mv_kmod = 'N'.
            ms_maske-knr = mo_service->next_knr( ).
          ENDIF.
          ls_kontakt-knr = ms_maske-knr.
          ls_kontakt-kunnr = mv_kunnr.
          ls_kontakt-vorname = ms_maske-vorname.
          ls_kontakt-nachname = ms_maske-nachname.
          ls_kontakt-email = ms_maske-email.
          ls_kontakt-telefon = ms_maske-telefon.
          IF mv_kmod = 'L'.
            mo_service->save_kontakt( EXPORTING is_kontakt = ls_kontakt iv_neu = ' ' IMPORTING ev_status = lv_status ).
          ELSE.
            mo_service->save_kontakt( EXPORTING is_kontakt = ls_kontakt iv_neu = 'X' IMPORTING ev_status = lv_status ).
          ENDIF.
          IF lv_status <> 'O'.
            IF lv_status = 'U'.
              ms_maske-info = 'Kontakt ist nicht mehr vorhanden.'.
            ELSE.
              ms_maske-info = 'Kontaktnummer ist bereits vergeben.'.
            ENDIF.
          ELSE.
            load_kontakte_view( ).
            ms_maske-info = 'Gespeichert.'.
          ENDIF.
        ENDIF.
        IF mv_con = 'X'.
          IF mv_cmod = 'N'.
            ms_maske-cnr = mo_service->next_cnr( ).
          ENDIF.
          ls_chance-cnr = ms_maske-cnr.
          ls_chance-kunnr = mv_kunnr.
          ls_chance-titel = ms_maske-titel.
          ls_chance-wert = ms_maske-wert.
          ls_chance-phase = ms_maske-phase.
          ls_chance-abschluss = ms_maske-abschluss.
          IF mv_cmod = 'L'.
            mo_service->save_chance( EXPORTING is_chance = ls_chance iv_neu = ' ' IMPORTING ev_status = lv_status ).
          ELSE.
            mo_service->save_chance( EXPORTING is_chance = ls_chance iv_neu = 'X' IMPORTING ev_status = lv_status ).
          ENDIF.
          IF lv_status <> 'O'.
            IF lv_status = 'U'.
              ms_maske-info = 'Verkaufschance ist nicht mehr vorhanden.'.
            ELSE.
              ms_maske-info = 'Chancennummer ist bereits vergeben.'.
            ENDIF.
          ELSE.
            load_chancen_view( ).
            ms_maske-info = 'Gespeichert.'.
          ENDIF.
        ENDIF.
      CATCH lcx_crm INTO lx.
        ms_maske-info = lx->mv_text.
        rv_error = 'E'.
    ENDTRY.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* Bericht: WRITE-Liste aus den Daten des Service.
* ---------------------------------------------------------------
CLASS lcl_report DEFINITION.
  PUBLIC SECTION.
    METHODS run
      IMPORTING io_service TYPE REF TO lcl_crm_service.
ENDCLASS.

CLASS lcl_report IMPLEMENTATION.
  METHOD run.
    DATA ls_kunde TYPE zkunde.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_sum TYPE i.
    DATA lv_sum_won TYPE i.
    DATA lv_anz TYPE i.
    DATA lv_kum TYPE i.
    DATA lv_pct TYPE i.
    DATA lv_line TYPE string.

    WRITE / 'CRM-Bericht'.
    WRITE / 'Offene Verkaufschancen'.
    lv_sum = 0.
    lv_anz = 0.
    io_service->first_chance_any( IMPORTING es_chance = ls_chance ev_found = lv_found ).
    WHILE lv_found = 'X'.
      IF ls_chance-phase = 'offen'.
        lv_sum = lv_sum + ls_chance-wert.
        lv_anz = lv_anz + 1.
        lv_line = |  Chance { ls_chance-cnr }, Kunde { ls_chance-kunnr }: { ls_chance-titel } { ls_chance-wert } EUR|.
        WRITE / lv_line.
      ENDIF.
      io_service->next_chance_any( EXPORTING iv_after = ls_chance-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDWHILE.
    lv_line = |  Anzahl { lv_anz }, Summe { lv_sum } EUR|.
    WRITE / lv_line.

    WRITE / 'Gewonnene Verkaufschancen'.
    lv_sum_won = 0.
    lv_anz = 0.
    io_service->first_chance_any( IMPORTING es_chance = ls_chance ev_found = lv_found ).
    WHILE lv_found = 'X'.
      IF ls_chance-phase = 'gewonnen'.
        lv_sum_won = lv_sum_won + ls_chance-wert.
        lv_anz = lv_anz + 1.
        lv_line = |  Chance { ls_chance-cnr }, Kunde { ls_chance-kunnr }: { ls_chance-titel } { ls_chance-wert } EUR|.
        WRITE / lv_line.
      ENDIF.
      io_service->next_chance_any( EXPORTING iv_after = ls_chance-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDWHILE.
    lv_line = |  Anzahl { lv_anz }, Summe { lv_sum_won } EUR|.
    WRITE / lv_line.

    WRITE / 'Umsatzanteil je Kunde (gewonnene Chancen)'.
    io_service->first_kunde( IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    WHILE lv_found = 'X'.
      lv_kum = io_service->sum_won_for_kunde( iv_kunnr = ls_kunde-kunnr ).
      lv_pct = 0.
      IF lv_sum_won > 0.
        lv_pct = ( lv_kum * 100 ) DIV lv_sum_won.
      ENDIF.
      lv_line = |  Kunde { ls_kunde-kunnr } { ls_kunde-name }: { lv_kum } EUR, { lv_pct } %|.
      WRITE / lv_line.
      io_service->next_kunde( EXPORTING iv_after = ls_kunde-kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    ENDWHILE.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* Glue: Dynpro-Felder (Namen wie in prog.dynp). Sie werden nur in den Modulen kopiert.
* ---------------------------------------------------------------
DATA: zkunnr     TYPE i,
      zname      TYPE c LENGTH 30,
      zort       TYPE c LENGTH 30,
      zbranche   TYPE c LENGTH 20,
      znkont     TYPE i,
      znchance   TYPE i,
      zinfo      TYPE c LENGTH 70,
      zknr       TYPE i,
      zvorname   TYPE c LENGTH 20,
      znachname  TYPE c LENGTH 30,
      zemail     TYPE c LENGTH 40,
      ztelefon   TYPE c LENGTH 20,
      zkpos      TYPE c LENGTH 20,
      zcnr       TYPE i,
      ztitel     TYPE c LENGTH 30,
      zwert      TYPE i,
      zphase     TYPE c LENGTH 10,
      zabschluss TYPE c LENGTH 8,
      zcpos      TYPE c LENGTH 20,
      zfrage     TYPE c LENGTH 56.

DATA ok_code TYPE c LENGTH 70.
DATA gv_ok TYPE c LENGTH 1.
DATA gv_error TYPE c LENGTH 1.
DATA gv_confirm TYPE c LENGTH 1.
DATA go_dao_kunde TYPE REF TO lcl_dao_kunde.
DATA go_dao_kontakt TYPE REF TO lcl_dao_kontakt.
DATA go_dao_chance TYPE REF TO lcl_dao_chance.
DATA go_service TYPE REF TO lcl_crm_service.
DATA go_view TYPE REF TO lcl_crm_view.
DATA go_report TYPE REF TO lcl_report.

START-OF-SELECTION.
  " WORKAROUND FR-001: DAOs werden direkt erzeugt. Eine Factory mit Interfaces fehlt.
  CREATE OBJECT go_dao_kunde.
  CREATE OBJECT go_dao_kontakt.
  CREATE OBJECT go_dao_chance.
  CREATE OBJECT go_service EXPORTING io_kunde = go_dao_kunde io_kontakt = go_dao_kontakt io_chance = go_dao_chance.
  CREATE OBJECT go_view EXPORTING io_service = go_service.
  CREATE OBJECT go_report.
  go_view->start( ).
  PERFORM to_dynpro.
  CALL SCREEN 0100.
  go_report->run( go_service ).

* Kopien zwischen Dynpro-Feldern und Maske. PBO kopiert in das Dynpro, PAI zurueck in die Maske.
FORM to_dynpro.
  DATA ls_m TYPE ty_maske.
  go_view->get_maske( IMPORTING es_maske = ls_m ).
  zkunnr = ls_m-kunnr.
  zname = ls_m-name.
  zort = ls_m-ort.
  zbranche = ls_m-branche.
  znkont = ls_m-nkont.
  znchance = ls_m-nchance.
  zinfo = ls_m-info.
  zknr = ls_m-knr.
  zvorname = ls_m-vorname.
  znachname = ls_m-nachname.
  zemail = ls_m-email.
  ztelefon = ls_m-telefon.
  zkpos = ls_m-kpos.
  zcnr = ls_m-cnr.
  ztitel = ls_m-titel.
  zwert = ls_m-wert.
  zphase = ls_m-phase.
  zabschluss = ls_m-abschluss.
  zcpos = ls_m-cpos.
  zfrage = ls_m-frage.
ENDFORM.

FORM from_dynpro.
  DATA ls_m TYPE ty_maske.
  go_view->get_maske( IMPORTING es_maske = ls_m ).
  ls_m-kunnr = zkunnr.
  ls_m-name = zname.
  ls_m-ort = zort.
  ls_m-branche = zbranche.
  ls_m-info = zinfo.
  ls_m-knr = zknr.
  ls_m-vorname = zvorname.
  ls_m-nachname = znachname.
  ls_m-email = zemail.
  ls_m-telefon = ztelefon.
  ls_m-cnr = zcnr.
  ls_m-titel = ztitel.
  ls_m-wert = zwert.
  ls_m-phase = zphase.
  ls_m-abschluss = zabschluss.
  go_view->set_maske( EXPORTING is_maske = ls_m ).
ENDFORM.

* ---------------------------------------------------------------
* Dynpro 0100: Kunde
* ---------------------------------------------------------------
MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
  go_view->refresh_counts( ).
  PERFORM to_dynpro.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  PERFORM from_dynpro.
  CASE ok_code.
    WHEN 'BACK'.
      LEAVE TO SCREEN 0.
    WHEN 'KONTAKT'.
      gv_ok = go_view->open_kontakte( ).
      IF gv_ok = 'X'.
        CALL SCREEN 0200.
        go_view->clear_info( ).
      ENDIF.
    WHEN 'DELETE'.
      gv_ok = go_view->can_delete_kunde( ).
      IF gv_ok = 'X'.
        PERFORM to_dynpro.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          go_view->delete_kunde( ).
        ELSE.
          go_view->set_info( 'Löschen abgebrochen.' ).
        ENDIF.
      ENDIF.
    WHEN OTHERS.
      gv_error = go_view->handle_0100( ok_code ).
      IF gv_error = 'E'.
        MESSAGE 'Eingabe prüfen' TYPE 'E'.
      ENDIF.
  ENDCASE.
ENDMODULE.

* ---------------------------------------------------------------
* Dynpro 0200: Kontakt und Verkaufschance
* ---------------------------------------------------------------
MODULE status_0200 OUTPUT.
  SET PF-STATUS 'STATUS_0200'.
  PERFORM to_dynpro.
ENDMODULE.

MODULE user_command_0200 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  PERFORM from_dynpro.
  CASE ok_code.
    WHEN 'BACK'.
      LEAVE SCREEN.
    WHEN 'CANCEL'.
      LEAVE SCREEN.
    WHEN 'KDEL'.
      gv_ok = go_view->can_delete_kontakt( ).
      IF gv_ok = 'X'.
        PERFORM to_dynpro.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          go_view->delete_kontakt( ).
        ENDIF.
      ENDIF.
    WHEN 'CDEL'.
      gv_ok = go_view->can_delete_chance( ).
      IF gv_ok = 'X'.
        PERFORM to_dynpro.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          go_view->delete_chance( ).
        ENDIF.
      ENDIF.
    WHEN OTHERS.
      gv_error = go_view->handle_0200( ok_code ).
      IF gv_error = 'E'.
        MESSAGE 'Eingabe prüfen' TYPE 'E'.
      ENDIF.
  ENDCASE.
ENDMODULE.

* F4 auf Phase: Wertehilfe. Das Modul setzt den Wert selbst (Laufzeit ohne Suchhilfe).
MODULE f4_phase INPUT.
  PERFORM from_dynpro.
  go_view->cycle_phase( ).
  PERFORM to_dynpro.
ENDMODULE.

* ---------------------------------------------------------------
* Dynpro 0300: Bestaetigung (Modalfenster)
* ---------------------------------------------------------------
MODULE status_0300 OUTPUT.
  SET PF-STATUS 'STATUS_0300'.
ENDMODULE.

MODULE user_command_0300 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'YES'.
      gv_confirm = 'X'.
      LEAVE SCREEN.
    WHEN OTHERS.
      gv_confirm = ' '.
      LEAVE SCREEN.
  ENDCASE.
ENDMODULE.
