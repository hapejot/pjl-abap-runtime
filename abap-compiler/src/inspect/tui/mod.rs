//! Terminal-Oberfläche mit crossterm: Objektliste oben, Quelltext unten.

pub mod app;

use std::io::{self, Stdout, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyEventKind},
    execute, queue,
    style::{Attribute, Color, ContentStyle, PrintStyledContent, StyledContent},
    terminal::{self, ClearType},
};

use app::{App, Object, Row, Sources};

// Farbschema
const HEAD_FG: Color = Color::White;
const HEAD_BG: Color = Color::DarkBlue;
const BAR_FG: Color = Color::White;
const BAR_BG: Color = Color::DarkGrey;
const SEL_FG: Color = Color::Black;
const SEL_BG_LIST: Color = Color::Cyan;
const SEL_BG_SRC: Color = Color::DarkGrey;
const SRC_TITLE_FG: Color = Color::Black;
const SRC_TITLE_BG: Color = Color::Green;
const FILE_HEADER: Color = Color::Yellow;
const COMMENT: Color = Color::DarkGreen;
const DECL: Color = Color::Magenta;
const STATUS_FG: Color = Color::Black;
const STATUS_BG: Color = Color::Grey;
const FILTER_BG: Color = Color::Yellow;

/// Farbe eines Eintrags: Klassen magenta, Methoden cyan, Routinen gelb.
fn entry_color(label: &str) -> Color {
    let kind = label.split(' ').next().unwrap_or("");
    match kind {
        "CLASS" | "INTERFACE" => Color::Magenta,
        "METHODS" | "CLASS-METHODS" => Color::Cyan,
        _ => Color::Yellow,
    }
}

/// Stile für einen Objekttyp in der Liste.
fn type_color(t: &str) -> Color {
    match t {
        "CLAS" => Color::Cyan,
        "PROG" => Color::Green,
        "FUGR" => Color::Yellow,
        "INTF" => Color::Magenta,
        "TABL" | "DTEL" | "DOMA" | "TTYP" => Color::Blue,
        _ => Color::White,
    }
}

fn style(fg: Color, bg: Option<Color>, bold: bool) -> ContentStyle {
    let mut s = ContentStyle::new();
    s.foreground_color = Some(fg);
    s.background_color = bg;
    if bold {
        s.attributes.set(Attribute::Bold);
    }
    s
}

/// Farbe einer Quelltextzeile: Dateikopf, Kommentar, Deklarationskopf oder normal.
fn source_style(text: &str) -> ContentStyle {
    let t = text.trim_start();
    if t.starts_with("----") {
        style(FILE_HEADER, None, true)
    } else if t.starts_with('*') || t.starts_with('"') {
        style(COMMENT, None, false)
    } else if [
        "CLASS ",
        "METHOD ",
        "FORM ",
        "FUNCTION ",
        "MODULE ",
        "INTERFACE ",
    ]
    .iter()
    .any(|k| t.to_uppercase().starts_with(k))
    {
        style(DECL, None, true)
    } else {
        style(Color::Reset, None, false)
    }
}

/// Stellt das Terminal beim Verlassen wieder her, auch bei Fehlern.
struct TermGuard;

impl TermGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(TermGuard)
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

/// Schreibt eine Zeile aus gestylten Teilen, abgeschnitten auf die Breite.
fn line(
    out: &mut Stdout,
    row: usize,
    cols: usize,
    parts: Vec<(String, ContentStyle)>,
) -> io::Result<()> {
    queue!(
        out,
        cursor::MoveTo(0, row as u16),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    let mut used = 0;
    for (text, st) in parts {
        if used >= cols {
            break;
        }
        let clipped: String = text.chars().take(cols - used).collect();
        used += clipped.chars().count();
        queue!(out, PrintStyledContent(StyledContent::new(st, clipped)))?;
    }
    Ok(())
}

/// Füllt Text bis zur vollen Breite, damit Balken und Auswahl durchgehend gefärbt sind.
fn full(text: &str, cols: usize) -> String {
    let len = text.chars().count();
    if len >= cols {
        text.chars().take(cols).collect()
    } else {
        format!("{text}{}", " ".repeat(cols - len))
    }
}

fn draw(out: &mut Stdout, app: &mut App) -> io::Result<()> {
    let (cols, rows) = terminal::size()?;
    let (cols, rows) = (cols as usize, rows as usize);

    // Layout: Zeile 0 Kopf, Liste bis list_h-1, list_h Quelltext-Kopf,
    // darunter Quelltext, letzte Zeile Statusleiste.
    let list_h = (rows / 2).max(3);
    let list_rows = list_h - 1;
    let src_h = rows.saturating_sub(list_h + 2);

    if app.sel < app.list_off {
        app.list_off = app.sel;
    }
    if app.sel >= app.list_off + list_rows {
        app.list_off = app.sel + 1 - list_rows;
    }
    let max_src = app.lines.len().saturating_sub(src_h);
    app.src_off = app.src_off.min(max_src);

    // Kopfzeile
    let shown_objects = app
        .rows
        .iter()
        .filter(|r| matches!(r, Row::Object(_)))
        .count();
    let head = format!(
        " Objects {}/{}   Filter: {}",
        shown_objects,
        app.objects.len(),
        app.filter
    );
    let head_style = if app.focus_src {
        style(HEAD_FG, Some(HEAD_BG), false)
    } else {
        style(SEL_FG, Some(SEL_BG_LIST), true)
    };
    line(out, 0, cols, vec![(full(&head, cols), head_style)])?;

    // Objekte und Einträge (Methoden, Funktionen, Formen ...)
    for r in 0..list_rows {
        let idx = app.list_off + r;
        let row = 1 + r;
        let Some(&item) = app.rows.get(idx) else {
            line(
                out,
                row,
                cols,
                vec![(" ".repeat(cols), style(Color::Reset, None, false))],
            )?;
            continue;
        };
        let selected = idx == app.sel;
        let parts = match item {
            Row::Object(oi) => {
                let (t, n) = &app.objects[oi].key;
                if selected {
                    let bg = if app.focus_src {
                        SEL_BG_SRC
                    } else {
                        SEL_BG_LIST
                    };
                    let fg = if app.focus_src { BAR_FG } else { SEL_FG };
                    vec![(
                        full(&format!(" {:<8} {}", t, n), cols),
                        style(fg, Some(bg), true),
                    )]
                } else {
                    vec![
                        (format!(" {:<8} ", t), style(type_color(t), None, true)),
                        (n.clone(), style(Color::Reset, None, false)),
                    ]
                }
            }
            Row::Entry(oi, ei) => {
                let e = &app.objects[oi].entries[ei];
                let indent = format!("     {}", "  ".repeat(e.depth));
                // Signatur: Deklaration ohne Schlüsselwort und Namen
                let rest = e
                    .detail
                    .split_whitespace()
                    .skip(2)
                    .collect::<Vec<_>>()
                    .join(" ");
                if selected {
                    let bg = if app.focus_src {
                        SEL_BG_SRC
                    } else {
                        SEL_BG_LIST
                    };
                    let fg = if app.focus_src { BAR_FG } else { SEL_FG };
                    let text = if rest.is_empty() {
                        format!("{indent}{}", e.label)
                    } else {
                        format!("{indent}{}  {rest}", e.label)
                    };
                    vec![(full(&text, cols), style(fg, Some(bg), true))]
                } else {
                    let mut parts = vec![
                        (indent, style(Color::Reset, None, false)),
                        (e.label.clone(), style(entry_color(&e.label), None, false)),
                    ];
                    if !rest.is_empty() {
                        parts.push((format!("  {rest}"), style(BAR_BG, None, false)));
                    }
                    parts
                }
            }
        };
        line(out, row, cols, parts)?;
    }

    // Titelbalken des Quelltextbereichs
    let focus = if app.focus_src { "Source" } else { "List" };
    let title = match app.current() {
        Some((t, n)) => format!(" {t} {n}   [Focus: {focus}] "),
        None => " no object selected ".to_string(),
    };
    let title_style = if app.focus_src {
        style(SRC_TITLE_FG, Some(SRC_TITLE_BG), true)
    } else {
        style(BAR_FG, Some(BAR_BG), true)
    };
    line(out, list_h, cols, vec![(full(&title, cols), title_style)])?;

    // Quelltext
    for r in 0..src_h {
        let text = app
            .lines
            .get(app.src_off + r)
            .map(String::as_str)
            .unwrap_or("");
        line(
            out,
            list_h + 1 + r,
            cols,
            vec![(text.to_string(), source_style(text))],
        )?;
    }

    // Statusleiste
    let (status, st) = if app.filter_mode {
        (
            format!(" Filter: {}_    Enter: done   Esc: clear", app.filter),
            style(STATUS_FG, Some(FILTER_BG), true),
        )
    } else {
        (
            " / Filter  Tab Focus  Up/Down  PgUp/PgDn  g/G start/end  q quit".to_string(),
            style(STATUS_FG, Some(STATUS_BG), false),
        )
    };
    line(
        out,
        rows.saturating_sub(1),
        cols,
        vec![(full(&status, cols), st)],
    )?;

    out.flush()
}

/// Startet die Oberfläche. Blockiert bis `q` oder Ctrl+C.
pub fn run(objects: Vec<Object>, sources: Sources) -> io::Result<()> {
    let _guard = TermGuard::enter()?;
    let mut out = io::stdout();
    let mut app = App::new(objects, sources);
    loop {
        draw(&mut out, &mut app)?;
        if let Event::Key(k) = event::read()? {
            if k.kind == KeyEventKind::Press {
                let (_, rows) = terminal::size()?;
                app.handle_key(k.code, k.modifiers, rows as usize / 2);
                if app.quit {
                    break;
                }
            }
        }
    }
    Ok(())
}
