//! Zustand der TUI: Objektliste mit Einträgen (Methoden, Funktionen), Filter,
//! Auswahl und Quelltext-Scrollen. Bewusst ohne Terminal-Zugriff, damit die Logik testbar ist.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::inspect::objects::Entry;

/// (Objekttyp, Objektname)
pub type Key = (String, String);
/// Objekt -> [(Dateiname, Quelltext)]
pub type Sources = BTreeMap<Key, Vec<(String, String)>>;

/// Ein Objekt mit seinen navigierbaren Einträgen.
#[derive(Debug, Clone)]
pub struct Object {
    pub key: Key,
    pub entries: Vec<Entry>,
}

/// Eine sichtbare Zeile der Liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Objekt (Index in `objects`)
    Object(usize),
    /// Eintrag (Objekt-Index, Eintrag-Index)
    Entry(usize, usize),
}

pub struct App {
    pub objects: Vec<Object>,
    sources: Sources,
    pub filter: String,
    pub filter_mode: bool,
    /// Sichtbare Zeilen nach Filter
    pub rows: Vec<Row>,
    /// Auswahl als Index in `rows`
    pub sel: usize,
    pub list_off: usize,
    /// Quelltext des geladenen Objekts als Zeilen
    pub lines: Vec<String>,
    /// Zeilenindex der Dateikopfzeile je Datei in `lines`
    header_pos: BTreeMap<String, usize>,
    /// Objekt-Index, dessen Quelltext in `lines` steht
    loaded: Option<usize>,
    pub src_off: usize,
    pub focus_src: bool,
    pub quit: bool,
}

impl App {
    pub fn new(objects: Vec<Object>, sources: Sources) -> Self {
        let mut app = App {
            objects,
            sources,
            filter: String::new(),
            filter_mode: false,
            rows: Vec::new(),
            sel: 0,
            list_off: 0,
            lines: Vec::new(),
            header_pos: BTreeMap::new(),
            loaded: None,
            src_off: 0,
            focus_src: false,
            quit: false,
        };
        app.refilter();
        app
    }

    /// Filter neu anwenden; Auswahl springt auf die erste Zeile.
    pub fn refilter(&mut self) {
        let f = self.filter.to_lowercase();
        let matches = |s: &str| f.is_empty() || s.to_lowercase().contains(&f);

        let mut rows = Vec::new();
        for (oi, o) in self.objects.iter().enumerate() {
            let obj_match = matches(&o.key.0) || matches(&o.key.1);
            let shown: Vec<usize> = (0..o.entries.len())
                .filter(|&ei| obj_match || matches(&o.entries[ei].label))
                .collect();
            if obj_match || !shown.is_empty() {
                rows.push(Row::Object(oi));
                rows.extend(shown.into_iter().map(|ei| Row::Entry(oi, ei)));
            }
        }
        self.rows = rows;
        self.sel = 0;
        self.list_off = 0;
        self.sync_source(true);
    }

    /// Objekt-Index der aktuellen Zeile.
    pub fn current_object(&self) -> Option<usize> {
        match self.rows.get(self.sel)? {
            Row::Object(oi) | Row::Entry(oi, _) => Some(*oi),
        }
    }

    /// Schlüssel des Objekts der aktuellen Zeile.
    pub fn current(&self) -> Option<&Key> {
        self.current_object().map(|oi| &self.objects[oi].key)
    }

    /// Eintrag der aktuellen Zeile, falls es ein Eintrag ist.
    pub fn current_entry(&self) -> Option<&Entry> {
        match self.rows.get(self.sel)? {
            Row::Entry(oi, ei) => Some(&self.objects[*oi].entries[*ei]),
            Row::Object(_) => None,
        }
    }

    /// Lädt den Quelltext bei Objektwechsel und positioniert ihn bei Einträgen.
    /// `force` lädt auch bei gleichem Objekt neu.
    fn sync_source(&mut self, force: bool) {
        let Some(oi) = self.current_object() else {
            self.lines.clear();
            self.header_pos.clear();
            self.loaded = None;
            self.src_off = 0;
            return;
        };
        if force || self.loaded != Some(oi) {
            self.load_source(oi);
        }
        if let Some(Row::Entry(_, ei)) = self.rows.get(self.sel).copied() {
            let e = &self.objects[oi].entries[ei];
            if let Some(&start) = self.header_pos.get(&e.file) {
                // Zeile n (1-basiert) liegt direkt n Zeilen nach der Kopfzeile
                self.src_off = (start + e.target_line()).saturating_sub(2);
            }
        }
        self.clamp_src();
    }

    fn load_source(&mut self, oi: usize) {
        self.lines.clear();
        self.header_pos.clear();
        self.src_off = 0;
        self.loaded = Some(oi);
        let key = self.objects[oi].key.clone();
        if let Some(files) = self.sources.get(&key) {
            for (file, src) in files {
                self.header_pos.insert(file.clone(), self.lines.len());
                self.lines.push(format!("---- {file} ----"));
                self.lines
                    .extend(src.lines().map(|l| l.replace('\t', "    ")));
                self.lines.push(String::new());
            }
        }
    }

    fn clamp_src(&mut self) {
        let max = self.lines.len().saturating_sub(1);
        self.src_off = self.src_off.min(max);
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let max = (self.rows.len() - 1) as isize;
        let new = (self.sel as isize + delta).clamp(0, max) as usize;
        if new != self.sel {
            self.sel = new;
            self.sync_source(false);
        }
    }

    pub fn scroll_src(&mut self, delta: isize) {
        self.src_off = (self.src_off as isize + delta).max(0) as usize;
        self.clamp_src();
    }

    fn move_by(&mut self, delta: isize) {
        if self.focus_src {
            self.scroll_src(delta);
        } else {
            self.move_sel(delta);
        }
    }

    fn jump(&mut self, to_end: bool) {
        if self.focus_src {
            self.src_off = if to_end {
                self.lines.len().saturating_sub(1)
            } else {
                0
            };
        } else if !self.rows.is_empty() {
            let target = if to_end { self.rows.len() - 1 } else { 0 };
            let delta = target as isize - self.sel as isize;
            self.move_sel(delta);
        }
    }

    /// Verarbeitet eine Taste. `page` ist die Seitengröße für PgUp/PgDn.
    pub fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers, page: usize) {
        let page = page.max(1) as isize;

        if self.filter_mode {
            match code {
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.refilter();
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.refilter();
                }
                KeyCode::Enter => self.filter_mode = false,
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filter_mode = false;
                    self.refilter();
                }
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/') => self.filter_mode = true,
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.refilter();
                }
            }
            KeyCode::Tab => self.focus_src = !self.focus_src,
            // Enter auf einem Eintrag springt in den Quelltext
            KeyCode::Enter => {
                if self.current_entry().is_some() {
                    self.focus_src = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-page),
            KeyCode::PageDown => self.move_by(page),
            KeyCode::Home | KeyCode::Char('g') => self.jump(false),
            KeyCode::End | KeyCode::Char('G') => self.jump(true),
            _ => {}
        }
    }
}
