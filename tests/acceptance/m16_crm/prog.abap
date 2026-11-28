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
* Vertrag der DAOs: je Tabelle ein Interface. Services und View-Model kennen nur diese Interfaces.
* Listen kommen als Tabellenparameter, sortiert nach dem Schluessel (aufsteigend).
* Kein COMMIT WORK in den DAOs (das macht der Service).
* ---------------------------------------------------------------
INTERFACE lif_dao_kunde.
  METHODS read
    IMPORTING iv_kunnr TYPE i
    EXPORTING es_kunde TYPE zkunde
              ev_found TYPE c.
  METHODS read_all
    EXPORTING et_kunden TYPE TABLE OF zkunde.
  METHODS read_last
    EXPORTING es_kunde TYPE zkunde
              ev_found TYPE c.
  METHODS insert
    IMPORTING is_kunde TYPE zkunde
    EXPORTING ev_ok TYPE c.
  METHODS update
    IMPORTING is_kunde TYPE zkunde
    EXPORTING ev_ok TYPE c.
  METHODS delete
    IMPORTING iv_kunnr TYPE i
    EXPORTING ev_ok TYPE c.
ENDINTERFACE.

INTERFACE lif_dao_kontakt.
  METHODS read
    IMPORTING iv_knr TYPE i
    EXPORTING es_kontakt TYPE zkontakt
              ev_found TYPE c.
  METHODS read_last
    EXPORTING es_kontakt TYPE zkontakt
              ev_found TYPE c.
  METHODS read_all_for
    IMPORTING iv_kunnr TYPE i
    EXPORTING et_kontakte TYPE TABLE OF zkontakt.
  METHODS insert
    IMPORTING is_kontakt TYPE zkontakt
    EXPORTING ev_ok TYPE c.
  METHODS update
    IMPORTING is_kontakt TYPE zkontakt
    EXPORTING ev_ok TYPE c.
  METHODS delete
    IMPORTING iv_knr TYPE i.
  METHODS delete_for_kunde
    IMPORTING iv_kunnr TYPE i.
ENDINTERFACE.

INTERFACE lif_dao_chance.
  METHODS read
    IMPORTING iv_cnr TYPE i
    EXPORTING es_chance TYPE zchance
              ev_found TYPE c.
  METHODS read_all
    EXPORTING et_chancen TYPE TABLE OF zchance.
  METHODS read_last
    EXPORTING es_chance TYPE zchance
              ev_found TYPE c.
  METHODS read_all_for
    IMPORTING iv_kunnr TYPE i
    EXPORTING et_chancen TYPE TABLE OF zchance.
  METHODS insert
    IMPORTING is_chance TYPE zchance
    EXPORTING ev_ok TYPE c.
  METHODS update
    IMPORTING is_chance TYPE zchance
    EXPORTING ev_ok TYPE c.
  METHODS delete
    IMPORTING iv_cnr TYPE i.
  METHODS delete_for_kunde
    IMPORTING iv_kunnr TYPE i.
ENDINTERFACE.

* ---------------------------------------------------------------
* DAO-Implementierungen: das einzige SQL. Jede implementiert ein Interface.
* ---------------------------------------------------------------
CLASS lcl_dao_kunde_sql DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao_kunde.
ENDCLASS.

CLASS lcl_dao_kunde_sql IMPLEMENTATION.
  METHOD lif_dao_kunde~read.
    SELECT SINGLE * FROM zkunde INTO es_kunde WHERE kunnr = @iv_kunnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kunde~read_all.
    SELECT * FROM zkunde INTO TABLE et_kunden ORDER BY kunnr.
  ENDMETHOD.

  METHOD lif_dao_kunde~read_last.
    SELECT SINGLE * FROM zkunde INTO es_kunde ORDER BY kunnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kunde~insert.
    INSERT zkunde FROM is_kunde.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kunde~update.
    UPDATE zkunde FROM is_kunde.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kunde~delete.
    DELETE FROM zkunde WHERE kunnr = @iv_kunnr.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.
ENDCLASS.

CLASS lcl_dao_kontakt_sql DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao_kontakt.
ENDCLASS.

CLASS lcl_dao_kontakt_sql IMPLEMENTATION.
  METHOD lif_dao_kontakt~read.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt WHERE knr = @iv_knr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kontakt~read_last.
    SELECT SINGLE * FROM zkontakt INTO es_kontakt ORDER BY knr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kontakt~read_all_for.
    SELECT * FROM zkontakt INTO TABLE et_kontakte WHERE kunnr = @iv_kunnr ORDER BY knr.
  ENDMETHOD.

  METHOD lif_dao_kontakt~insert.
    INSERT zkontakt FROM is_kontakt.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kontakt~update.
    UPDATE zkontakt FROM is_kontakt.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_kontakt~delete.
    DELETE FROM zkontakt WHERE knr = @iv_knr.
  ENDMETHOD.

  METHOD lif_dao_kontakt~delete_for_kunde.
    DELETE FROM zkontakt WHERE kunnr = @iv_kunnr.
  ENDMETHOD.
ENDCLASS.

CLASS lcl_dao_chance_sql DEFINITION.
  PUBLIC SECTION.
    INTERFACES lif_dao_chance.
ENDCLASS.

CLASS lcl_dao_chance_sql IMPLEMENTATION.
  METHOD lif_dao_chance~read.
    SELECT SINGLE * FROM zchance INTO es_chance WHERE cnr = @iv_cnr.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_chance~read_all.
    SELECT * FROM zchance INTO TABLE et_chancen ORDER BY cnr.
  ENDMETHOD.

  METHOD lif_dao_chance~read_last.
    SELECT SINGLE * FROM zchance INTO es_chance ORDER BY cnr DESCENDING.
    IF sy-subrc = 0.
      ev_found = 'X'.
    ELSE.
      ev_found = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_chance~read_all_for.
    SELECT * FROM zchance INTO TABLE et_chancen WHERE kunnr = @iv_kunnr ORDER BY cnr.
  ENDMETHOD.

  METHOD lif_dao_chance~insert.
    INSERT zchance FROM is_chance.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_chance~update.
    UPDATE zchance FROM is_chance.
    IF sy-subrc = 0.
      ev_ok = 'X'.
    ELSE.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD lif_dao_chance~delete.
    DELETE FROM zchance WHERE cnr = @iv_cnr.
  ENDMETHOD.

  METHOD lif_dao_chance~delete_for_kunde.
    DELETE FROM zchance WHERE kunnr = @iv_kunnr.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* Factory: liefert die DAOs als Interface-Referenz. Das Hauptprogramm holt sie hierüber.
* ---------------------------------------------------------------
CLASS lcl_dao_factory DEFINITION.
  PUBLIC SECTION.
    CLASS-METHODS get_kunde_dao
      RETURNING VALUE(ro_dao) TYPE REF TO lif_dao_kunde.
    CLASS-METHODS get_kontakt_dao
      RETURNING VALUE(ro_dao) TYPE REF TO lif_dao_kontakt.
    CLASS-METHODS get_chance_dao
      RETURNING VALUE(ro_dao) TYPE REF TO lif_dao_chance.
ENDCLASS.

CLASS lcl_dao_factory IMPLEMENTATION.
  METHOD get_kunde_dao.
    CREATE OBJECT ro_dao TYPE lcl_dao_kunde_sql.
  ENDMETHOD.

  METHOD get_kontakt_dao.
    CREATE OBJECT ro_dao TYPE lcl_dao_kontakt_sql.
  ENDMETHOD.

  METHOD get_chance_dao.
    CREATE OBJECT ro_dao TYPE lcl_dao_chance_sql.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* Service: Anwendungsfaelle und Transaktionsgrenzen (LUW). Kein Dynpro, keine Anzeigetexte.
* Listen kommen je Aufruf vom DAO. Blaettern und Zaehlen macht das View-Model auf diesen Listen.
* Status-Codes beim Speichern: 'O' = ok, 'U' = Satz fehlt beim Aendern, 'I' = Schluessel ist schon vergeben.
* ---------------------------------------------------------------
CLASS lcl_crm_service DEFINITION.
  PUBLIC SECTION.
    METHODS constructor
      IMPORTING io_kunde_dao TYPE REF TO lif_dao_kunde
                io_kontakt_dao TYPE REF TO lif_dao_kontakt
                io_chance_dao TYPE REF TO lif_dao_chance.
    METHODS read_kunde
      IMPORTING iv_kunnr TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS list_kunden
      EXPORTING et_kunden TYPE TABLE OF zkunde.
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
    METHODS list_kontakte
      IMPORTING iv_kunnr TYPE i
      EXPORTING et_kontakte TYPE TABLE OF zkontakt.
    METHODS count_kontakte
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_n) TYPE i.
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
    METHODS list_chancen
      IMPORTING iv_kunnr TYPE i
      EXPORTING et_chancen TYPE TABLE OF zchance.
    METHODS list_chancen_all
      EXPORTING et_chancen TYPE TABLE OF zchance.
    METHODS count_chancen
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_n) TYPE i.
    METHODS next_cnr
      RETURNING VALUE(rv_next) TYPE i.
    METHODS save_chance
      IMPORTING is_chance TYPE zchance
                iv_neu TYPE c
      EXPORTING ev_status TYPE c.
    METHODS delete_chance
      IMPORTING iv_cnr TYPE i.
    METHODS sum_won_for_kunde
      IMPORTING iv_kunnr TYPE i
      RETURNING VALUE(rv_sum) TYPE i.
  PRIVATE SECTION.
    DATA mo_kunde TYPE REF TO lif_dao_kunde.
    DATA mo_kontakt TYPE REF TO lif_dao_kontakt.
    DATA mo_chance TYPE REF TO lif_dao_chance.
ENDCLASS.

CLASS lcl_crm_service IMPLEMENTATION.
  METHOD constructor.
    mo_kunde = io_kunde_dao.
    mo_kontakt = io_kontakt_dao.
    mo_chance = io_chance_dao.
  ENDMETHOD.

  METHOD read_kunde.
    mo_kunde->read( EXPORTING iv_kunnr = iv_kunnr IMPORTING es_kunde = es_kunde ev_found = ev_found ).
  ENDMETHOD.

  METHOD list_kunden.
    mo_kunde->read_all( IMPORTING et_kunden = et_kunden ).
  ENDMETHOD.

  METHOD next_kunnr.
    DATA ls_kunde TYPE zkunde.
    DATA lv_found TYPE c.
    mo_kunde->read_last( IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_kunde-kunnr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_kunde.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_kunde->insert( EXPORTING is_kunde = is_kunde IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_kunde->update( EXPORTING is_kunde = is_kunde IMPORTING ev_ok = lv_ok ).
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
    mo_kunde->delete( EXPORTING iv_kunnr = iv_kunnr IMPORTING ev_ok = lv_ok ).
    IF lv_ok = 'X'.
      COMMIT WORK.
      ev_ok = 'X'.
    ELSE.
      ROLLBACK WORK.
      ev_ok = ' '.
    ENDIF.
  ENDMETHOD.

  METHOD read_kontakt.
    mo_kontakt->read( EXPORTING iv_knr = iv_knr IMPORTING es_kontakt = es_kontakt ev_found = ev_found ).
  ENDMETHOD.

  METHOD list_kontakte.
    mo_kontakt->read_all_for( EXPORTING iv_kunnr = iv_kunnr IMPORTING et_kontakte = et_kontakte ).
  ENDMETHOD.

  METHOD count_kontakte.
    DATA lt_kontakte TYPE TABLE OF zkontakt.
    list_kontakte( EXPORTING iv_kunnr = iv_kunnr IMPORTING et_kontakte = lt_kontakte ).
    DESCRIBE TABLE lt_kontakte LINES rv_n.
  ENDMETHOD.

  METHOD next_knr.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    mo_kontakt->read_last( IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_kontakt-knr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_kontakt.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_kontakt->insert( EXPORTING is_kontakt = is_kontakt IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_kontakt->update( EXPORTING is_kontakt = is_kontakt IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'U'.
      ENDIF.
    ENDIF.
  ENDMETHOD.

  METHOD delete_kontakt.
    mo_kontakt->delete( iv_knr = iv_knr ).
    COMMIT WORK.
  ENDMETHOD.

  METHOD read_chance.
    mo_chance->read( EXPORTING iv_cnr = iv_cnr IMPORTING es_chance = es_chance ev_found = ev_found ).
  ENDMETHOD.

  METHOD list_chancen.
    mo_chance->read_all_for( EXPORTING iv_kunnr = iv_kunnr IMPORTING et_chancen = et_chancen ).
  ENDMETHOD.

  METHOD list_chancen_all.
    mo_chance->read_all( IMPORTING et_chancen = et_chancen ).
  ENDMETHOD.

  METHOD count_chancen.
    DATA lt_chancen TYPE TABLE OF zchance.
    list_chancen( EXPORTING iv_kunnr = iv_kunnr IMPORTING et_chancen = lt_chancen ).
    DESCRIBE TABLE lt_chancen LINES rv_n.
  ENDMETHOD.

  METHOD next_cnr.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    mo_chance->read_last( IMPORTING es_chance = ls_chance ev_found = lv_found ).
    IF lv_found = 'X'.
      rv_next = ls_chance-cnr + 1.
    ELSE.
      rv_next = 1.
    ENDIF.
  ENDMETHOD.

  METHOD save_chance.
    DATA lv_ok TYPE c.
    IF iv_neu = 'X'.
      mo_chance->insert( EXPORTING is_chance = is_chance IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'I'.
      ENDIF.
    ELSE.
      mo_chance->update( EXPORTING is_chance = is_chance IMPORTING ev_ok = lv_ok ).
      IF lv_ok = 'X'.
        COMMIT WORK.
        ev_status = 'O'.
      ELSE.
        ev_status = 'U'.
      ENDIF.
    ENDIF.
  ENDMETHOD.

  METHOD delete_chance.
    mo_chance->delete( iv_cnr = iv_cnr ).
    COMMIT WORK.
  ENDMETHOD.

  METHOD sum_won_for_kunde.
    DATA lt_chancen TYPE TABLE OF zchance.
    DATA ls_chance TYPE zchance.
    rv_sum = 0.
    mo_chance->read_all_for( EXPORTING iv_kunnr = iv_kunnr IMPORTING et_chancen = lt_chancen ).
    LOOP AT lt_chancen INTO ls_chance.
      IF ls_chance-phase = 'gewonnen'.
        rv_sum = rv_sum + ls_chance-wert.
      ENDIF.
    ENDLOOP.
  ENDMETHOD.
ENDCLASS.

* ---------------------------------------------------------------
* View-Model: Zustand der Bildschirme und Entscheidungen zu Tasten. Kein SQL, kein Dynpro, kein CALL SCREEN.
* Rueckgabe 'E' = Eingabefehler; das Modul gibt dann die Meldung aus.
* Blaettern (vorheriger, naechster Eintrag) geschieht auf den Listen des Service.
* Die Position in Listen (Kontakte, Chancen) wird aus den Daten berechnet, nicht im Objekt gehalten.
* Das Dynpro liest und schreibt ms_maske direkt (Komponentenzugriff).
* ---------------------------------------------------------------
CLASS lcl_crm_view DEFINITION.
  PUBLIC SECTION.
    DATA ms_maske TYPE ty_maske.

    METHODS constructor
      IMPORTING io_service TYPE REF TO lcl_crm_service.
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
    METHODS save_0200
      RETURNING VALUE(rv_error) TYPE c.
  PRIVATE SECTION.
    DATA mo_service TYPE REF TO lcl_crm_service.
    DATA mo_val TYPE REF TO lcl_validator.
    DATA mv_kunnr TYPE i.
    DATA mv_kmode TYPE c LENGTH 1.
    DATA mv_kon TYPE c LENGTH 1.
    DATA mv_kmod TYPE c LENGTH 1.
    DATA mv_con TYPE c LENGTH 1.
    DATA mv_cmod TYPE c LENGTH 1.
    DATA mv_phase_alt TYPE c LENGTH 10.

    METHODS prev_kunde
      IMPORTING iv_before TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
    METHODS next_kunde
      IMPORTING iv_after TYPE i
      EXPORTING es_kunde TYPE zkunde
                ev_found TYPE c.
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

  METHOD set_info.
    ms_maske-info = iv_info.
  ENDMETHOD.

  METHOD clear_info.
    ms_maske-info = ' '.
  ENDMETHOD.

  METHOD start.
    DATA lt_kunden TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
    DATA lv_kunnr TYPE i.
    lv_kunnr = 0.
    mo_service->list_kunden( IMPORTING et_kunden = lt_kunden ).
    LOOP AT lt_kunden INTO ls_kunde.
      lv_kunnr = ls_kunde-kunnr.
      EXIT.
    ENDLOOP.
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
        prev_kunde( EXPORTING iv_before = mv_kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
        IF lv_found = 'X'.
          read_kunde( ls_kunde-kunnr ).
        ELSE.
          ms_maske-info = 'Erster Kunde erreicht.'.
        ENDIF.
      WHEN 'NEXT'.
        next_kunde( EXPORTING iv_after = mv_kunnr IMPORTING es_kunde = ls_kunde ev_found = lv_found ).
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

  METHOD prev_kunde.
    DATA lt_kunden TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
    ev_found = ' '.
    mo_service->list_kunden( IMPORTING et_kunden = lt_kunden ).
    " Sortiert nach Kundennummer: der letzte Treffer unterhalb der Vorgabe ist der vorherige Kunde.
    LOOP AT lt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr < iv_before.
        es_kunde = ls_kunde.
        ev_found = 'X'.
      ENDIF.
    ENDLOOP.
  ENDMETHOD.

  METHOD next_kunde.
    DATA lt_kunden TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
    ev_found = ' '.
    mo_service->list_kunden( IMPORTING et_kunden = lt_kunden ).
    " Sortiert nach Kundennummer: der erste Treffer oberhalb der Vorgabe ist der naechste Kunde.
    LOOP AT lt_kunden INTO ls_kunde.
      IF ls_kunde-kunnr > iv_after.
        es_kunde = ls_kunde.
        ev_found = 'X'.
        EXIT.
      ENDIF.
    ENDLOOP.
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
    DATA lt_kontakte TYPE TABLE OF zkontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    DATA lv_n TYPE i.
    mo_service->list_kontakte( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_kontakte = lt_kontakte ).
    DESCRIBE TABLE lt_kontakte LINES lv_n.
    IF lv_n = 0.
      ms_maske-knr = 0.
    ELSE.
      " Die gemerkte Nummer bleibt, wenn sie zum Kunden gehoert. Sonst wird der erste Kontakt gewaehlt.
      lv_found = ' '.
      LOOP AT lt_kontakte INTO ls_kontakt.
        IF ls_kontakt-knr = ms_maske-knr.
          lv_found = 'X'.
        ENDIF.
      ENDLOOP.
      IF lv_found <> 'X'.
        LOOP AT lt_kontakte INTO ls_kontakt.
          ms_maske-knr = ls_kontakt-knr.
          EXIT.
        ENDLOOP.
      ENDIF.
    ENDIF.
    show_kontakt( ).
  ENDMETHOD.

  METHOD show_kontakt.
    DATA lt_kontakte TYPE TABLE OF zkontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA ls_zeile TYPE zkontakt.
    DATA lv_found TYPE c.
    DATA lv_pos TYPE i.
    DATA lv_n TYPE i.
    lv_found = ' '.
    IF ms_maske-knr <> 0.
      mo_service->read_kontakt( EXPORTING iv_knr = ms_maske-knr IMPORTING es_kontakt = ls_kontakt ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X' AND ls_kontakt-kunnr = mv_kunnr.
      mo_service->list_kontakte( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_kontakte = lt_kontakte ).
      DESCRIBE TABLE lt_kontakte LINES lv_n.
      " Position: Anzahl der Kontakte bis einschliesslich dieses Kontakts (Liste ist aufsteigend sortiert).
      lv_pos = 0.
      LOOP AT lt_kontakte INTO ls_zeile.
        IF ls_zeile-knr <= ls_kontakt-knr.
          lv_pos = lv_pos + 1.
        ENDIF.
      ENDLOOP.
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
    DATA lt_kontakte TYPE TABLE OF zkontakt.
    DATA ls_kontakt TYPE zkontakt.
    DATA lv_found TYPE c.
    DATA lv_knr TYPE i.
    lv_found = ' '.
    mo_service->list_kontakte( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_kontakte = lt_kontakte ).
    IF iv_step > 0.
      " Naechster Kontakt: erster mit groesserer Nummer.
      LOOP AT lt_kontakte INTO ls_kontakt.
        IF ls_kontakt-knr > ms_maske-knr.
          lv_knr = ls_kontakt-knr.
          lv_found = 'X'.
          EXIT.
        ENDIF.
      ENDLOOP.
    ELSE.
      " Vorheriger Kontakt: letzter mit kleinerer Nummer.
      LOOP AT lt_kontakte INTO ls_kontakt.
        IF ls_kontakt-knr < ms_maske-knr.
          lv_knr = ls_kontakt-knr.
          lv_found = 'X'.
        ENDIF.
      ENDLOOP.
    ENDIF.
    IF lv_found = 'X'.
      ms_maske-knr = lv_knr.
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
    DATA lt_chancen TYPE TABLE OF zchance.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_n TYPE i.
    mo_service->list_chancen( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_chancen = lt_chancen ).
    DESCRIBE TABLE lt_chancen LINES lv_n.
    IF lv_n = 0.
      ms_maske-cnr = 0.
    ELSE.
      " Die gemerkte Nummer bleibt, wenn sie zum Kunden gehoert. Sonst wird die erste Chance gewaehlt.
      lv_found = ' '.
      LOOP AT lt_chancen INTO ls_chance.
        IF ls_chance-cnr = ms_maske-cnr.
          lv_found = 'X'.
        ENDIF.
      ENDLOOP.
      IF lv_found <> 'X'.
        LOOP AT lt_chancen INTO ls_chance.
          ms_maske-cnr = ls_chance-cnr.
          EXIT.
        ENDLOOP.
      ENDIF.
    ENDIF.
    show_chance( ).
  ENDMETHOD.

  METHOD show_chance.
    DATA lt_chancen TYPE TABLE OF zchance.
    DATA ls_chance TYPE zchance.
    DATA ls_zeile TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_pos TYPE i.
    DATA lv_n TYPE i.
    lv_found = ' '.
    IF ms_maske-cnr <> 0.
      mo_service->read_chance( EXPORTING iv_cnr = ms_maske-cnr IMPORTING es_chance = ls_chance ev_found = lv_found ).
    ENDIF.
    IF lv_found = 'X' AND ls_chance-kunnr = mv_kunnr.
      mo_service->list_chancen( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_chancen = lt_chancen ).
      DESCRIBE TABLE lt_chancen LINES lv_n.
      " Position: Anzahl der Chancen bis einschliesslich dieser Chance (Liste ist aufsteigend sortiert).
      lv_pos = 0.
      LOOP AT lt_chancen INTO ls_zeile.
        IF ls_zeile-cnr <= ls_chance-cnr.
          lv_pos = lv_pos + 1.
        ENDIF.
      ENDLOOP.
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
    DATA lt_chancen TYPE TABLE OF zchance.
    DATA ls_chance TYPE zchance.
    DATA lv_found TYPE c.
    DATA lv_cnr TYPE i.
    lv_found = ' '.
    mo_service->list_chancen( EXPORTING iv_kunnr = mv_kunnr IMPORTING et_chancen = lt_chancen ).
    IF iv_step > 0.
      " Naechste Chance: erste mit groesserer Nummer.
      LOOP AT lt_chancen INTO ls_chance.
        IF ls_chance-cnr > ms_maske-cnr.
          lv_cnr = ls_chance-cnr.
          lv_found = 'X'.
          EXIT.
        ENDIF.
      ENDLOOP.
    ELSE.
      " Vorherige Chance: letzte mit kleinerer Nummer.
      LOOP AT lt_chancen INTO ls_chance.
        IF ls_chance-cnr < ms_maske-cnr.
          lv_cnr = ls_chance-cnr.
          lv_found = 'X'.
        ENDIF.
      ENDLOOP.
    ENDIF.
    IF lv_found = 'X'.
      ms_maske-cnr = lv_cnr.
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
* Bericht: WRITE-Liste aus den Listen des Service.
* ---------------------------------------------------------------
CLASS lcl_report DEFINITION.
  PUBLIC SECTION.
    METHODS run
      IMPORTING io_service TYPE REF TO lcl_crm_service.
ENDCLASS.

CLASS lcl_report IMPLEMENTATION.
  METHOD run.
    DATA lt_chancen TYPE TABLE OF zchance.
    DATA ls_chance TYPE zchance.
    DATA lt_kunden TYPE TABLE OF zkunde.
    DATA ls_kunde TYPE zkunde.
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
    io_service->list_chancen_all( IMPORTING et_chancen = lt_chancen ).
    LOOP AT lt_chancen INTO ls_chance.
      IF ls_chance-phase = 'offen'.
        lv_sum = lv_sum + ls_chance-wert.
        lv_anz = lv_anz + 1.
        lv_line = |  Chance { ls_chance-cnr }, Kunde { ls_chance-kunnr }: { ls_chance-titel } { ls_chance-wert } EUR|.
        WRITE / lv_line.
      ENDIF.
    ENDLOOP.
    lv_line = |  Anzahl { lv_anz }, Summe { lv_sum } EUR|.
    WRITE / lv_line.

    WRITE / 'Gewonnene Verkaufschancen'.
    lv_sum_won = 0.
    lv_anz = 0.
    LOOP AT lt_chancen INTO ls_chance.
      IF ls_chance-phase = 'gewonnen'.
        lv_sum_won = lv_sum_won + ls_chance-wert.
        lv_anz = lv_anz + 1.
        lv_line = |  Chance { ls_chance-cnr }, Kunde { ls_chance-kunnr }: { ls_chance-titel } { ls_chance-wert } EUR|.
        WRITE / lv_line.
      ENDIF.
    ENDLOOP.
    lv_line = |  Anzahl { lv_anz }, Summe { lv_sum_won } EUR|.
    WRITE / lv_line.

    WRITE / 'Umsatzanteil je Kunde (gewonnene Chancen)'.
    io_service->list_kunden( IMPORTING et_kunden = lt_kunden ).
    LOOP AT lt_kunden INTO ls_kunde.
      lv_kum = io_service->sum_won_for_kunde( iv_kunnr = ls_kunde-kunnr ).
      lv_pct = 0.
      IF lv_sum_won > 0.
        lv_pct = ( lv_kum * 100 ) DIV lv_sum_won.
      ENDIF.
      lv_line = |  Kunde { ls_kunde-kunnr } { ls_kunde-name }: { lv_kum } EUR, { lv_pct } %|.
      WRITE / lv_line.
    ENDLOOP.
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
DATA go_dao_kunde TYPE REF TO lif_dao_kunde.
DATA go_dao_kontakt TYPE REF TO lif_dao_kontakt.
DATA go_dao_chance TYPE REF TO lif_dao_chance.
DATA go_service TYPE REF TO lcl_crm_service.
DATA go_view TYPE REF TO lcl_crm_view.
DATA go_report TYPE REF TO lcl_report.

START-OF-SELECTION.
  go_dao_kunde = lcl_dao_factory=>get_kunde_dao( ).
  go_dao_kontakt = lcl_dao_factory=>get_kontakt_dao( ).
  go_dao_chance = lcl_dao_factory=>get_chance_dao( ).
  CREATE OBJECT go_service EXPORTING io_kunde_dao = go_dao_kunde io_kontakt_dao = go_dao_kontakt io_chance_dao = go_dao_chance.
  CREATE OBJECT go_view EXPORTING io_service = go_service.
  CREATE OBJECT go_report.
  go_view->start( ).
  PERFORM to_dynpro.
  CALL SCREEN 0100.
  go_report->run( go_service ).

* Kopien zwischen Dynpro-Feldern und Maske. PBO kopiert in das Dynpro, PAI zurueck in die Maske.
FORM to_dynpro.
  zkunnr = go_view->ms_maske-kunnr.
  zname = go_view->ms_maske-name.
  zort = go_view->ms_maske-ort.
  zbranche = go_view->ms_maske-branche.
  znkont = go_view->ms_maske-nkont.
  znchance = go_view->ms_maske-nchance.
  zinfo = go_view->ms_maske-info.
  zknr = go_view->ms_maske-knr.
  zvorname = go_view->ms_maske-vorname.
  znachname = go_view->ms_maske-nachname.
  zemail = go_view->ms_maske-email.
  ztelefon = go_view->ms_maske-telefon.
  zkpos = go_view->ms_maske-kpos.
  zcnr = go_view->ms_maske-cnr.
  ztitel = go_view->ms_maske-titel.
  zwert = go_view->ms_maske-wert.
  zphase = go_view->ms_maske-phase.
  zabschluss = go_view->ms_maske-abschluss.
  zcpos = go_view->ms_maske-cpos.
  zfrage = go_view->ms_maske-frage.
ENDFORM.

FORM from_dynpro.
  go_view->ms_maske-kunnr = zkunnr.
  go_view->ms_maske-name = zname.
  go_view->ms_maske-ort = zort.
  go_view->ms_maske-branche = zbranche.
  go_view->ms_maske-info = zinfo.
  go_view->ms_maske-knr = zknr.
  go_view->ms_maske-vorname = zvorname.
  go_view->ms_maske-nachname = znachname.
  go_view->ms_maske-email = zemail.
  go_view->ms_maske-telefon = ztelefon.
  go_view->ms_maske-cnr = zcnr.
  go_view->ms_maske-titel = ztitel.
  go_view->ms_maske-wert = zwert.
  go_view->ms_maske-phase = zphase.
  go_view->ms_maske-abschluss = zabschluss.
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
