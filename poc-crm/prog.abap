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

* Ausnahme der Validierung. Der Text steht im Attribut.
CLASS lcx_crm DEFINITION INHERITING FROM cx_static_check.
  PUBLIC SECTION.
    DATA mv_text TYPE c LENGTH 70.
ENDCLASS.

CLASS lcx_crm IMPLEMENTATION.
ENDCLASS.

* Validierung der Eingaben. Ungueltige Werte loesen lcx_crm aus.
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

* Dynpro-Felder (Namen wie in prog.dynp).
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

* Arbeitsdaten.
DATA: ok_code      TYPE sy-ucomm,
      gt_kunden    TYPE TABLE OF zkunde,
      gs_kunde     TYPE zkunde,
      gt_kontakte  TYPE TABLE OF zkontakt,
      gs_kontakt   TYPE zkontakt,
      gt_chancen   TYPE TABLE OF zchance,
      gs_chance    TYPE zchance,
      gt_alle_c    TYPE TABLE OF zchance,
      gt_alle_k    TYPE TABLE OF zkontakt,
      gv_kunnr     TYPE i,
      gv_kmode     TYPE c LENGTH 1,
      gv_kidx      TYPE i,
      gv_klen      TYPE i,
      gv_kon       TYPE c LENGTH 1,
      gv_kmod      TYPE c LENGTH 1,
      gv_cidx      TYPE i,
      gv_clen      TYPE i,
      gv_con       TYPE c LENGTH 1,
      gv_cmod      TYPE c LENGTH 1,
      gv_phase_alt TYPE c LENGTH 10,
      gv_confirm   TYPE c LENGTH 1,
      gv_ok        TYPE c LENGTH 1,
      gv_step      TYPE i,
      gv_next      TYPE i,
      gv_pos       TYPE i,
      gv_prev      TYPE i,
      gv_found     TYPE c LENGTH 1,
      gv_sum       TYPE i,
      gv_sum_won   TYPE i,
      gv_anz       TYPE i,
      gv_kum       TYPE i,
      gv_pct       TYPE i,
      gv_line      TYPE string,
      go_val       TYPE REF TO lcl_validator,
      gx           TYPE REF TO lcx_crm.

FIELD-SYMBOLS <ls_k> LIKE LINE OF gt_kontakte.
FIELD-SYMBOLS <ls_c> LIKE LINE OF gt_chancen.

START-OF-SELECTION.
  CREATE OBJECT go_val.
  PERFORM f_load_kunden.
  LOOP AT gt_kunden INTO gs_kunde.
    zkunnr = gs_kunde-kunnr.
    EXIT.
  ENDLOOP.
  PERFORM f_read_kunde.
  CALL SCREEN 0100.
  PERFORM f_report.

* ---------------------------------------------------------------
* Dynpro 0100: Kunde
* ---------------------------------------------------------------
MODULE status_0100 OUTPUT.
  SET PF-STATUS 'STATUS_0100'.
  PERFORM f_counts.
ENDMODULE.

MODULE user_command_0100 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'BACK'.
      LEAVE TO SCREEN 0.
    WHEN 'CANCEL'.
      IF gv_kmode = 'L'.
        zkunnr = gv_kunnr.
        PERFORM f_read_kunde.
        zinfo = 'Eingaben verworfen.'.
      ELSE.
        PERFORM f_clear_kunde.
        zinfo = 'Eingaben verworfen.'.
      ENDIF.
    WHEN 'PREV'.
      PERFORM f_prev_kunde.
    WHEN 'NEXT'.
      PERFORM f_next_kunde.
    WHEN 'NEW'.
      PERFORM f_clear_kunde.
      zinfo = 'Neuer Kunde: Daten eingeben, dann F11 sichern.'.
    WHEN 'SAVE'.
      TRY.
          go_val->check_kunde( zname ).
          PERFORM f_save_kunde.
        CATCH lcx_crm INTO gx.
          zinfo = gx->mv_text.
          MESSAGE 'Eingabe prüfen' TYPE 'E'.
      ENDTRY.
    WHEN 'KONTAKT'.
      IF gv_kmode = 'L'.
        CLEAR zknr.
        PERFORM f_load_kontakte.
        CLEAR zcnr.
        PERFORM f_load_chancen.
        CLEAR zinfo.
        CALL SCREEN 0200.
        CLEAR zinfo.
      ELSE.
        zinfo = 'Kunde zuerst mit F11 sichern.'.
      ENDIF.
    WHEN 'DELETE'.
      IF gv_kmode = 'L'.
        zfrage = 'Kunde samt Kontakten und Chancen löschen?'.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          PERFORM f_delete_kunde.
        ELSE.
          zinfo = 'Löschen abgebrochen.'.
        ENDIF.
      ELSE.
        zinfo = 'Kein gespeicherter Kunde ausgewählt.'.
      ENDIF.
    WHEN OTHERS.
      IF zkunnr <> gv_kunnr.
        PERFORM f_read_kunde.
      ENDIF.
  ENDCASE.
ENDMODULE.

* ---------------------------------------------------------------
* Dynpro 0200: Kontakt und Verkaufschance
* ---------------------------------------------------------------
MODULE status_0200 OUTPUT.
  SET PF-STATUS 'STATUS_0200'.
ENDMODULE.

MODULE user_command_0200 INPUT.
  ok_code = sy-ucomm.
  CLEAR sy-ucomm.
  CASE ok_code.
    WHEN 'BACK'.
      LEAVE SCREEN.
    WHEN 'CANCEL'.
      LEAVE SCREEN.
    WHEN 'KPREV'.
      gv_step = -1.
      PERFORM f_move_kontakt.
    WHEN 'KNEXT'.
      gv_step = 1.
      PERFORM f_move_kontakt.
    WHEN 'KNEW'.
      PERFORM f_new_kontakt.
    WHEN 'CPREV'.
      gv_step = -1.
      PERFORM f_move_chance.
    WHEN 'CNEXT'.
      gv_step = 1.
      PERFORM f_move_chance.
    WHEN 'CNEW'.
      PERFORM f_new_chance.
    WHEN 'SAVE'.
      TRY.
          IF gv_kon = 'X'.
            go_val->check_kontakt( iv_nachname = znachname iv_email = zemail ).
          ENDIF.
          IF gv_con = 'X'.
            go_val->check_chance( iv_titel = ztitel iv_wert = zwert iv_phase = zphase
                                  iv_abschluss = zabschluss iv_alt = gv_phase_alt ).
          ENDIF.
          IF gv_kon = 'X'.
            PERFORM f_save_kontakt.
          ENDIF.
          IF gv_con = 'X'.
            PERFORM f_save_chance.
          ENDIF.
        CATCH lcx_crm INTO gx.
          zinfo = gx->mv_text.
          MESSAGE 'Eingabe prüfen' TYPE 'E'.
      ENDTRY.
    WHEN 'KDEL'.
      IF gv_kon = 'X' AND gv_kmod = 'L'.
        zfrage = 'Kontakt wirklich löschen?'.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          PERFORM f_delete_kontakt.
        ENDIF.
      ELSE.
        zinfo = 'Kein gespeicherter Kontakt ausgewählt.'.
      ENDIF.
    WHEN 'CDEL'.
      IF gv_con = 'X' AND gv_cmod = 'L'.
        zfrage = 'Verkaufschance wirklich löschen?'.
        gv_confirm = ' '.
        CALL SCREEN 0300 STARTING AT 10 8.
        IF gv_confirm = 'X'.
          PERFORM f_delete_chance.
        ENDIF.
      ELSE.
        zinfo = 'Keine gespeicherte Verkaufschance ausgewählt.'.
      ENDIF.
    WHEN OTHERS.
      CLEAR zinfo.
  ENDCASE.
ENDMODULE.

* F4 auf Phase: Wertehilfe. Das Modul setzt den Wert selbst (Laufzeit ohne Suchhilfe).
MODULE f4_phase INPUT.
  IF zphase = 'offen'.
    zphase = 'gewonnen'.
  ELSEIF zphase = 'gewonnen'.
    zphase = 'verloren'.
  ELSE.
    zphase = 'offen'.
  ENDIF.
  zinfo = 'Gültige Werte: offen, gewonnen, verloren.'.
ENDMODULE.

* ---------------------------------------------------------------
* Dynpro 0300: Bestätigung (Modalfenster)
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

* ---------------------------------------------------------------
* Kunde
* ---------------------------------------------------------------
FORM f_load_kunden.
  SELECT * FROM zkunde INTO TABLE gt_kunden ORDER BY kunnr.
ENDFORM.

FORM f_show_kunde.
  zkunnr = gs_kunde-kunnr.
  zname = gs_kunde-name.
  zort = gs_kunde-ort.
  zbranche = gs_kunde-branche.
ENDFORM.

* Liest den Kunden mit der Nummer zkunnr. Unbekannte Nummer: Neuanlage mit dieser Nummer.
FORM f_read_kunde.
  SELECT SINGLE * FROM zkunde INTO gs_kunde WHERE kunnr = @zkunnr.
  IF sy-subrc = 0.
    gv_kunnr = zkunnr.
    gv_kmode = 'L'.
    PERFORM f_show_kunde.
    CLEAR zinfo.
  ELSE.
    gv_kunnr = zkunnr.
    gv_kmode = 'N'.
    zinfo = 'Kunde nicht vorhanden: Daten eingeben und mit F11 anlegen.'.
  ENDIF.
ENDFORM.

FORM f_clear_kunde.
  gv_kunnr = 0.
  gv_kmode = 'N'.
  gv_kon = ' '.
  gv_con = ' '.
  zkunnr = 0.
  CLEAR zname.
  CLEAR zort.
  CLEAR zbranche.
  znkont = 0.
  znchance = 0.
ENDFORM.

FORM f_counts.
  znkont = 0.
  znchance = 0.
  IF gv_kmode = 'L'.
    SELECT * FROM zkontakt INTO TABLE gt_kontakte WHERE kunnr = @gv_kunnr ORDER BY knr.
    DESCRIBE TABLE gt_kontakte LINES znkont.
    SELECT * FROM zchance INTO TABLE gt_chancen WHERE kunnr = @gv_kunnr ORDER BY cnr.
    DESCRIBE TABLE gt_chancen LINES znchance.
  ENDIF.
ENDFORM.

FORM f_prev_kunde.
  PERFORM f_load_kunden.
  gv_found = ' '.
  LOOP AT gt_kunden INTO gs_kunde.
    IF gs_kunde-kunnr < gv_kunnr.
      gv_found = 'X'.
      gv_prev = gs_kunde-kunnr.
    ENDIF.
  ENDLOOP.
  IF gv_found = 'X'.
    zkunnr = gv_prev.
    PERFORM f_read_kunde.
  ELSE.
    zinfo = 'Erster Kunde erreicht.'.
  ENDIF.
ENDFORM.

FORM f_next_kunde.
  PERFORM f_load_kunden.
  gv_found = ' '.
  LOOP AT gt_kunden INTO gs_kunde.
    IF gs_kunde-kunnr > gv_kunnr AND gv_found = ' '.
      gv_found = 'X'.
      gv_prev = gs_kunde-kunnr.
    ENDIF.
  ENDLOOP.
  IF gv_found = 'X'.
    zkunnr = gv_prev.
    PERFORM f_read_kunde.
  ELSE.
    zinfo = 'Letzter Kunde erreicht.'.
  ENDIF.
ENDFORM.

FORM f_next_kunnr.
  PERFORM f_load_kunden.
  gv_next = 0.
  LOOP AT gt_kunden INTO gs_kunde.
    IF gs_kunde-kunnr > gv_next.
      gv_next = gs_kunde-kunnr.
    ENDIF.
  ENDLOOP.
  gv_next = gv_next + 1.
ENDFORM.

FORM f_save_kunde.
  IF gv_kmode = 'N' AND zkunnr = 0.
    PERFORM f_next_kunnr.
    zkunnr = gv_next.
  ENDIF.
  gs_kunde-kunnr = zkunnr.
  gs_kunde-name = zname.
  gs_kunde-ort = zort.
  gs_kunde-branche = zbranche.
  IF gv_kmode = 'L'.
    UPDATE zkunde FROM gs_kunde.
    IF sy-subrc = 0.
      COMMIT WORK.
      zinfo = 'Kunde gespeichert.'.
    ELSE.
      zinfo = 'Kunde ist nicht mehr vorhanden.'.
    ENDIF.
  ELSE.
    INSERT zkunde FROM gs_kunde.
    IF sy-subrc = 0.
      COMMIT WORK.
      gv_kunnr = zkunnr.
      gv_kmode = 'L'.
      zinfo = 'Kunde angelegt.'.
    ELSE.
      zinfo = 'Kundennummer ist bereits vergeben.'.
    ENDIF.
  ENDIF.
ENDFORM.

* Logische Arbeitseinheit: Verkaufschancen, Kontakte und Kunde werden zusammen geloescht.
FORM f_delete_kunde.
  TRY.
      DELETE FROM zchance WHERE kunnr = @gv_kunnr.
      DELETE FROM zkontakt WHERE kunnr = @gv_kunnr.
      DELETE FROM zkunde WHERE kunnr = @gv_kunnr.
      IF sy-subrc <> 0.
        CREATE OBJECT gx.
        gx->mv_text = 'Kunde nicht gefunden, nichts gelöscht.'.
        RAISE EXCEPTION gx.
      ENDIF.
      COMMIT WORK.
      PERFORM f_clear_kunde.
      zinfo = 'Kunde mit Kontakten und Verkaufschancen gelöscht.'.
    CATCH lcx_crm INTO gx.
      ROLLBACK WORK.
      zinfo = gx->mv_text.
  ENDTRY.
ENDFORM.

* ---------------------------------------------------------------
* Kontakte (Block oben in 0200)
* ---------------------------------------------------------------
* Laedt die Kontakte des Kunden. Steht zknr auf einer Kontaktnummer, wird dieser Kontakt gewaehlt,
* sonst der erste.
FORM f_load_kontakte.
  SELECT * FROM zkontakt INTO TABLE gt_kontakte WHERE kunnr = @gv_kunnr ORDER BY knr.
  DESCRIBE TABLE gt_kontakte LINES gv_klen.
  gv_kidx = 0.
  gv_pos = 0.
  LOOP AT gt_kontakte INTO gs_kontakt.
    gv_pos = gv_pos + 1.
    IF gs_kontakt-knr = zknr.
      gv_kidx = gv_pos.
      EXIT.
    ENDIF.
  ENDLOOP.
  IF gv_kidx = 0 AND gv_klen > 0.
    gv_kidx = 1.
  ENDIF.
  PERFORM f_show_kontakt.
ENDFORM.

FORM f_show_kontakt.
  IF gv_kidx > 0.
    READ TABLE gt_kontakte ASSIGNING <ls_k> INDEX gv_kidx.
    gs_kontakt = <ls_k>.
    zknr = gs_kontakt-knr.
    zvorname = gs_kontakt-vorname.
    znachname = gs_kontakt-nachname.
    zemail = gs_kontakt-email.
    ztelefon = gs_kontakt-telefon.
    zkpos = |{ gv_kidx } von { gv_klen }|.
    gv_kon = 'X'.
    gv_kmod = 'L'.
  ELSE.
    zknr = 0.
    CLEAR zvorname.
    CLEAR znachname.
    CLEAR zemail.
    CLEAR ztelefon.
    zkpos = 'keine Kontakte'.
    gv_kon = ' '.
    gv_kmod = 'N'.
  ENDIF.
ENDFORM.

FORM f_move_kontakt.
  gv_pos = gv_kidx + gv_step.
  IF gv_pos >= 1 AND gv_pos <= gv_klen.
    gv_kidx = gv_pos.
    PERFORM f_show_kontakt.
    CLEAR zinfo.
  ELSE.
    zinfo = 'Kein weiterer Kontakt in dieser Richtung.'.
  ENDIF.
ENDFORM.

FORM f_new_kontakt.
  gv_kon = 'X'.
  gv_kmod = 'N'.
  zknr = 0.
  CLEAR zvorname.
  CLEAR znachname.
  CLEAR zemail.
  CLEAR ztelefon.
  zkpos = 'neu'.
  CLEAR zinfo.
ENDFORM.

FORM f_next_knr.
  SELECT * FROM zkontakt INTO TABLE gt_alle_k ORDER BY knr.
  gv_next = 0.
  LOOP AT gt_alle_k INTO gs_kontakt.
    IF gs_kontakt-knr > gv_next.
      gv_next = gs_kontakt-knr.
    ENDIF.
  ENDLOOP.
  gv_next = gv_next + 1.
ENDFORM.

FORM f_save_kontakt.
  IF gv_kmod = 'N'.
    PERFORM f_next_knr.
    zknr = gv_next.
  ENDIF.
  gs_kontakt-knr = zknr.
  gs_kontakt-kunnr = gv_kunnr.
  gs_kontakt-vorname = zvorname.
  gs_kontakt-nachname = znachname.
  gs_kontakt-email = zemail.
  gs_kontakt-telefon = ztelefon.
  gv_ok = ' '.
  IF gv_kmod = 'L'.
    UPDATE zkontakt FROM gs_kontakt.
    IF sy-subrc = 0.
      gv_ok = 'X'.
    ELSE.
      zinfo = 'Kontakt ist nicht mehr vorhanden.'.
    ENDIF.
  ELSE.
    INSERT zkontakt FROM gs_kontakt.
    IF sy-subrc = 0.
      gv_ok = 'X'.
    ELSE.
      zinfo = 'Kontaktnummer ist bereits vergeben.'.
    ENDIF.
  ENDIF.
  IF gv_ok = 'X'.
    COMMIT WORK.
    PERFORM f_load_kontakte.
    zinfo = 'Gespeichert.'.
  ENDIF.
ENDFORM.

FORM f_delete_kontakt.
  DELETE FROM zkontakt WHERE knr = @zknr.
  COMMIT WORK.
  zinfo = 'Kontakt gelöscht.'.
  CLEAR zknr.
  PERFORM f_load_kontakte.
ENDFORM.

* ---------------------------------------------------------------
* Verkaufschancen (Block unten in 0200)
* ---------------------------------------------------------------
FORM f_load_chancen.
  SELECT * FROM zchance INTO TABLE gt_chancen WHERE kunnr = @gv_kunnr ORDER BY cnr.
  DESCRIBE TABLE gt_chancen LINES gv_clen.
  gv_cidx = 0.
  gv_pos = 0.
  LOOP AT gt_chancen INTO gs_chance.
    gv_pos = gv_pos + 1.
    IF gs_chance-cnr = zcnr.
      gv_cidx = gv_pos.
      EXIT.
    ENDIF.
  ENDLOOP.
  IF gv_cidx = 0 AND gv_clen > 0.
    gv_cidx = 1.
  ENDIF.
  PERFORM f_show_chance.
ENDFORM.

FORM f_show_chance.
  IF gv_cidx > 0.
    READ TABLE gt_chancen ASSIGNING <ls_c> INDEX gv_cidx.
    gs_chance = <ls_c>.
    zcnr = gs_chance-cnr.
    ztitel = gs_chance-titel.
    zwert = gs_chance-wert.
    zphase = gs_chance-phase.
    zabschluss = gs_chance-abschluss.
    gv_phase_alt = gs_chance-phase.
    zcpos = |{ gv_cidx } von { gv_clen }|.
    gv_con = 'X'.
    gv_cmod = 'L'.
  ELSE.
    zcnr = 0.
    CLEAR ztitel.
    zwert = 0.
    CLEAR zphase.
    CLEAR zabschluss.
    gv_phase_alt = 'offen'.
    zcpos = 'keine Chancen'.
    gv_con = ' '.
    gv_cmod = 'N'.
  ENDIF.
ENDFORM.

FORM f_move_chance.
  gv_pos = gv_cidx + gv_step.
  IF gv_pos >= 1 AND gv_pos <= gv_clen.
    gv_cidx = gv_pos.
    PERFORM f_show_chance.
    CLEAR zinfo.
  ELSE.
    zinfo = 'Keine weitere Verkaufschance in dieser Richtung.'.
  ENDIF.
ENDFORM.

FORM f_new_chance.
  gv_con = 'X'.
  gv_cmod = 'N'.
  zcnr = 0.
  CLEAR ztitel.
  zwert = 0.
  zphase = 'offen'.
  CLEAR zabschluss.
  gv_phase_alt = 'offen'.
  zcpos = 'neu'.
  CLEAR zinfo.
ENDFORM.

FORM f_next_cnr.
  SELECT * FROM zchance INTO TABLE gt_alle_c ORDER BY cnr.
  gv_next = 0.
  LOOP AT gt_alle_c INTO gs_chance.
    IF gs_chance-cnr > gv_next.
      gv_next = gs_chance-cnr.
    ENDIF.
  ENDLOOP.
  gv_next = gv_next + 1.
ENDFORM.

FORM f_save_chance.
  IF gv_cmod = 'N'.
    PERFORM f_next_cnr.
    zcnr = gv_next.
  ENDIF.
  gs_chance-cnr = zcnr.
  gs_chance-kunnr = gv_kunnr.
  gs_chance-titel = ztitel.
  gs_chance-wert = zwert.
  gs_chance-phase = zphase.
  gs_chance-abschluss = zabschluss.
  gv_ok = ' '.
  IF gv_cmod = 'L'.
    UPDATE zchance FROM gs_chance.
    IF sy-subrc = 0.
      gv_ok = 'X'.
    ELSE.
      zinfo = 'Verkaufschance ist nicht mehr vorhanden.'.
    ENDIF.
  ELSE.
    INSERT zchance FROM gs_chance.
    IF sy-subrc = 0.
      gv_ok = 'X'.
    ELSE.
      zinfo = 'Chancennummer ist bereits vergeben.'.
    ENDIF.
  ENDIF.
  IF gv_ok = 'X'.
    COMMIT WORK.
    PERFORM f_load_chancen.
    zinfo = 'Gespeichert.'.
  ENDIF.
ENDFORM.

FORM f_delete_chance.
  DELETE FROM zchance WHERE cnr = @zcnr.
  COMMIT WORK.
  zinfo = 'Verkaufschance gelöscht.'.
  CLEAR zcnr.
  PERFORM f_load_chancen.
ENDFORM.

* ---------------------------------------------------------------
* Bericht (WRITE-Liste nach dem Dynpro-Ablauf)
* ---------------------------------------------------------------
FORM f_report.
  PERFORM f_load_kunden.
  SELECT * FROM zchance INTO TABLE gt_alle_c ORDER BY cnr.

  WRITE / 'CRM-Bericht'.
  WRITE / 'Offene Verkaufschancen'.
  gv_sum = 0.
  gv_anz = 0.
  LOOP AT gt_alle_c INTO gs_chance.
    IF gs_chance-phase = 'offen'.
      gv_sum = gv_sum + gs_chance-wert.
      gv_anz = gv_anz + 1.
      gv_line = |  Chance { gs_chance-cnr }, Kunde { gs_chance-kunnr }: { gs_chance-titel } { gs_chance-wert } EUR|.
      WRITE / gv_line.
    ENDIF.
  ENDLOOP.
  gv_line = |  Anzahl { gv_anz }, Summe { gv_sum } EUR|.
  WRITE / gv_line.

  WRITE / 'Gewonnene Verkaufschancen'.
  gv_sum_won = 0.
  gv_anz = 0.
  LOOP AT gt_alle_c INTO gs_chance.
    IF gs_chance-phase = 'gewonnen'.
      gv_sum_won = gv_sum_won + gs_chance-wert.
      gv_anz = gv_anz + 1.
      gv_line = |  Chance { gs_chance-cnr }, Kunde { gs_chance-kunnr }: { gs_chance-titel } { gs_chance-wert } EUR|.
      WRITE / gv_line.
    ENDIF.
  ENDLOOP.
  gv_line = |  Anzahl { gv_anz }, Summe { gv_sum_won } EUR|.
  WRITE / gv_line.

  WRITE / 'Umsatzanteil je Kunde (gewonnene Chancen)'.
  LOOP AT gt_kunden INTO gs_kunde.
    gv_kum = 0.
    LOOP AT gt_alle_c INTO gs_chance.
      IF gs_chance-kunnr = gs_kunde-kunnr AND gs_chance-phase = 'gewonnen'.
        gv_kum = gv_kum + gs_chance-wert.
      ENDIF.
    ENDLOOP.
    gv_pct = 0.
    IF gv_sum_won > 0.
      gv_pct = ( gv_kum * 100 ) DIV gv_sum_won.
    ENDIF.
    gv_line = |  Kunde { gs_kunde-kunnr } { gs_kunde-name }: { gv_kum } EUR, { gv_pct } %|.
    WRITE / gv_line.
  ENDLOOP.
ENDFORM.
