//! Headless-Backend für Tests (Abschnitt 9.1, 9.3). Die Eingabe kommt aus einem Skript, die Abzüge
//! (`SNAPSHOT`) werden als Zeilen geschrieben, jeweils gefolgt von `----`.
//!
//! Skriptformat: eine Anweisung je Zeile. `'` beginnt einen Kommentar (außerhalb von Zeichenketten).
//! `TYPE '<text>'` schreibt Zeichen (Überschreibmodus), `''` in der Zeichenkette ist ein Hochkomma.
//! `FIELD <name>` setzt den Cursor auf das Feld. `KEY <taste>` sendet eine Taste. `SNAPSHOT` schreibt das Gitter.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;

use super::grid::{Grid, ROWS};
use super::terminal::{Event, Key, Terminal};

/// Anweisung des Eingabeskripts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Key(Key),
    Type(String),
    Field(String),
    Snapshot,
}

/// Terminal aus einem Skript. Der Abzug wird beim `SNAPSHOT` aus dem zuletzt gezeichneten Gitter geschrieben.
pub struct Headless {
    commands: Vec<(usize, Command)>,
    next: usize,
    queued: VecDeque<Key>,
    line: Option<usize>,
    last: Grid,
    out: Box<dyn Write + Send>,
}

impl Headless {
    /// Öffnet das Skript. Abzüge gehen in `out_path`, ohne Angabe nach stderr.
    pub fn open(script: &Path, out_path: Option<&Path>) -> Result<Headless, String> {
        let text = std::fs::read_to_string(script)
            .map_err(|e| format!("cannot read screen script {}: {e}", script.display()))?;
        let out: Box<dyn Write + Send> = match out_path {
            Some(path) => Box::new(
                std::fs::File::create(path)
                    .map_err(|e| format!("cannot create screen output {}: {e}", path.display()))?,
            ),
            None => Box::new(std::io::stderr()),
        };
        Headless::from_script(&text, out)
    }

    /// Terminal aus einem Skripttext und einer Ausgabe.
    pub fn from_script(text: &str, out: Box<dyn Write + Send>) -> Result<Headless, String> {
        Ok(Headless {
            commands: parse(text)?,
            next: 0,
            queued: VecDeque::new(),
            line: None,
            last: Grid::new(),
            out,
        })
    }

    fn snapshot(&mut self) -> Result<(), String> {
        let mut text = String::new();
        for row in 0..ROWS {
            text.push_str(&self.last.line(row));
            text.push('\n');
        }
        text.push_str("----\n");
        self.out
            .write_all(text.as_bytes())
            .and_then(|_| self.out.flush())
            .map_err(|e| format!("cannot write screen snapshot: {e}"))
    }
}

impl Terminal for Headless {
    fn draw(&mut self, grid: &Grid, _cursor: (usize, usize)) -> Result<(), String> {
        self.last = grid.clone();
        Ok(())
    }

    fn next_event(&mut self) -> Result<Event, String> {
        loop {
            if let Some(key) = self.queued.pop_front() {
                return Ok(Event::Key(key));
            }
            let Some((line, command)) = self.commands.get(self.next).cloned() else {
                return Ok(Event::Eof);
            };
            self.next += 1;
            self.line = Some(line);
            match command {
                Command::Key(key) => return Ok(Event::Key(key)),
                Command::Type(text) => self.queued.extend(text.chars().map(Key::Char)),
                Command::Field(name) => return Ok(Event::Field(name)),
                Command::Snapshot => self.snapshot()?,
            }
        }
    }

    fn location(&self) -> Option<usize> {
        self.line
    }
}

/// Liest das Skript. Fehler nennen die Zeile.
pub fn parse(text: &str) -> Result<Vec<(usize, Command)>, String> {
    let mut commands = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let mut lexer = Lexer::new(raw);
        if lexer.at_comment_or_end() {
            continue;
        }
        let command = parse_line(&mut lexer).map_err(|e| format!("line {line}: {e}"))?;
        commands.push((line, command));
    }
    Ok(commands)
}

fn parse_line(lexer: &mut Lexer) -> Result<Command, String> {
    let keyword = lexer
        .word()
        .ok_or_else(|| "expected a command".to_string())?;
    let command = match keyword.to_ascii_uppercase().as_str() {
        "SNAPSHOT" => Command::Snapshot,
        "FIELD" => {
            let name = lexer
                .word()
                .ok_or_else(|| "FIELD needs a field name".to_string())?;
            Command::Field(name.to_ascii_uppercase())
        }
        "KEY" => {
            let name = lexer
                .word()
                .ok_or_else(|| "KEY needs a key name".to_string())?;
            Command::Key(parse_key(&name).ok_or_else(|| format!("unknown key '{name}'"))?)
        }
        "TYPE" => {
            let text = lexer
                .string()?
                .ok_or_else(|| "TYPE needs a text in single quotes".to_string())?;
            Command::Type(text)
        }
        other => return Err(format!("unknown command '{other}'")),
    };
    if !lexer.at_comment_or_end() {
        return Err(format!("unexpected text after {keyword}"));
    }
    Ok(command)
}

/// Taste aus dem Skript (Abschnitt 9.3), ohne Unterschied bei Groß- und Kleinschreibung.
fn parse_key(name: &str) -> Option<Key> {
    let upper = name.to_ascii_uppercase();
    if let Some(number) = upper.strip_prefix('F') {
        let n: u8 = number.parse().ok()?;
        return (1..=12).contains(&n).then_some(Key::F(n));
    }
    Some(match upper.as_str() {
        "ENTER" => Key::Enter,
        "TAB" => Key::Tab,
        "BTAB" => Key::BackTab,
        "ESC" => Key::Esc,
        "BACKSPACE" => Key::Backspace,
        "DELETE" => Key::Delete,
        "UP" => Key::Up,
        "DOWN" => Key::Down,
        "LEFT" => Key::Left,
        "RIGHT" => Key::Right,
        _ => return None,
    })
}

/// Zerlegung einer Skriptzeile in Wörter und Zeichenketten.
struct Lexer<'a> {
    rest: &'a str,
}

impl<'a> Lexer<'a> {
    fn new(line: &'a str) -> Lexer<'a> {
        Lexer { rest: line }
    }

    fn skip_blanks(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// Leere Zeile oder Kommentar bis zum Ende.
    fn at_comment_or_end(&mut self) -> bool {
        self.skip_blanks();
        self.rest.is_empty() || self.rest.starts_with('\'')
    }

    /// Nächstes Wort ohne Anführungszeichen. `None`, wenn keins folgt.
    fn word(&mut self) -> Option<String> {
        self.skip_blanks();
        if self.rest.is_empty() || self.rest.starts_with('\'') {
            return None;
        }
        let end = self
            .rest
            .find(char::is_whitespace)
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(word.to_string())
    }

    /// Zeichenkette in einfachen Hochkommas. `''` ist ein Hochkomma. `None`, wenn keine folgt.
    fn string(&mut self) -> Result<Option<String>, String> {
        self.skip_blanks();
        let Some(body) = self.rest.strip_prefix('\'') else {
            return Ok(None);
        };
        let mut text = String::new();
        let mut chars = body.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            if ch == '\'' {
                if matches!(chars.peek(), Some((_, '\''))) {
                    chars.next();
                    text.push('\'');
                    continue;
                }
                self.rest = &body[index + 1..];
                return Ok(Some(text));
            }
            text.push(ch);
        }
        Err("unterminated text, missing closing single quote".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Ausgabe, die der Test danach lesen kann.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn commands(text: &str) -> Vec<Command> {
        parse(text).unwrap().into_iter().map(|(_, c)| c).collect()
    }

    #[test]
    fn parses_all_commands_case_insensitively() {
        assert_eq!(
            commands("field zkunnr\nTYPE '4711'\nKEY tab\nKEY F12\nkey btab\nsnapshot\n"),
            vec![
                Command::Field("ZKUNNR".into()),
                Command::Type("4711".into()),
                Command::Key(Key::Tab),
                Command::Key(Key::F(12)),
                Command::Key(Key::BackTab),
                Command::Snapshot,
            ]
        );
    }

    #[test]
    fn all_key_names_are_known() {
        for name in [
            "ENTER",
            "TAB",
            "BTAB",
            "F1",
            "F12",
            "ESC",
            "BACKSPACE",
            "DELETE",
            "UP",
            "DOWN",
            "LEFT",
            "RIGHT",
        ] {
            assert!(parse_key(name).is_some(), "{name}");
        }
        assert_eq!(parse_key("F13"), None);
        assert_eq!(parse_key("SHIFTF1"), None);
    }

    #[test]
    fn quote_starts_a_comment_at_command_position_and_after_arguments() {
        assert_eq!(
            commands("' Kommentar nur\nFIELD ZKUNNR ' Cursor auf das Feld\nKEY TAB ' weiter\nTYPE 'a' ' Ende\n"),
            vec![
                Command::Field("ZKUNNR".into()),
                Command::Key(Key::Tab),
                Command::Type("a".into()),
            ]
        );
    }

    #[test]
    fn doubled_quote_inside_text_is_one_quote() {
        assert_eq!(commands("TYPE 'It''s'"), vec![Command::Type("It's".into())]);
    }

    #[test]
    fn blank_lines_are_skipped_and_line_numbers_are_kept() {
        let parsed = parse("\n  \nKEY ENTER\n").unwrap();
        assert_eq!(parsed, vec![(3, Command::Key(Key::Enter))]);
    }

    #[test]
    fn unknown_command_reports_its_line() {
        let err = parse("KEY ENTER\nCLICK 1\n").unwrap_err();
        assert_eq!(err, "line 2: unknown command 'CLICK'");
    }

    #[test]
    fn unknown_key_reports_its_line() {
        let err = parse("\nKEY F13\n").unwrap_err();
        assert_eq!(err, "line 2: unknown key 'F13'");
    }

    #[test]
    fn type_without_quotes_and_unterminated_text_are_errors() {
        assert!(parse("TYPE abc")
            .unwrap_err()
            .starts_with("line 1: TYPE needs"));
        assert!(parse("TYPE 'abc").unwrap_err().contains("unterminated"));
    }

    #[test]
    fn extra_words_after_a_command_are_errors() {
        assert_eq!(
            parse("KEY ENTER ENTER").unwrap_err(),
            "line 1: unexpected text after KEY"
        );
    }

    #[test]
    fn type_expands_to_character_keys_and_snapshot_writes_grid() {
        let out = Shared::default();
        let mut term =
            Headless::from_script("TYPE 'ab'\nSNAPSHOT\nKEY ESC\n", Box::new(out.clone())).unwrap();
        let grid = Grid::new();
        term.draw(&grid, (0, 0)).unwrap();
        assert_eq!(term.next_event().unwrap(), Event::Key(Key::Char('a')));
        assert_eq!(term.next_event().unwrap(), Event::Key(Key::Char('b')));
        assert_eq!(term.location(), Some(1));
        assert_eq!(term.next_event().unwrap(), Event::Key(Key::Esc));
        assert_eq!(term.next_event().unwrap(), Event::Eof);

        let written = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), ROWS + 1);
        assert_eq!(lines[0].len(), 80);
        assert_eq!(lines[ROWS], "----");
    }

    #[test]
    fn field_event_is_returned_with_name() {
        let mut term = Headless::from_script("FIELD zfeld\n", Box::new(Shared::default())).unwrap();
        assert_eq!(term.next_event().unwrap(), Event::Field("ZFELD".into()));
    }

    #[test]
    fn open_reads_script_file_and_writes_snapshots_to_output_file() {
        let dir = std::env::temp_dir().join(format!("abaprt_headless_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("input.keys");
        let output = dir.join("screen.txt");
        std::fs::write(&script, "SNAPSHOT\nKEY ENTER\n").unwrap();

        let mut term = Headless::open(&script, Some(&output)).unwrap();
        term.draw(&Grid::new(), (0, 0)).unwrap();
        assert_eq!(term.next_event().unwrap(), Event::Key(Key::Enter));
        assert_eq!(term.next_event().unwrap(), Event::Eof);
        drop(term);

        let written = std::fs::read_to_string(&output).unwrap();
        assert_eq!(written.matches("----\n").count(), 1);
        assert_eq!(written.lines().count(), ROWS + 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_reports_missing_script_file() {
        let missing = std::env::temp_dir().join("abaprt_no_such_script_file.keys");
        let err = Headless::open(&missing, None)
            .err()
            .expect("error expected");
        assert!(err.starts_with("cannot read screen script"), "{err}");
    }
}
