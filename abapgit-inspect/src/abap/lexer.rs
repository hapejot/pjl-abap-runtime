//! Tokenizer für ABAP-Quelltext.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Bezeichner, Schlüsselwörter, Zahlen, Feldsymbole wie `<fs_x>`
    Word,
    /// `'text'` oder `` `text` ``
    Literal,
    /// String-Template `|text {  expr }|`
    Template,
    /// Operatoren und Satzzeichen
    Punct,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn is_word(&self, kw: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(kw)
    }

    pub fn is_punct(&self, p: &str) -> bool {
        self.kind == Kind::Punct && self.text == p
    }
}

const MULTI_PUNCT: [&str; 8] = ["->", "=>", "<>", "<=", ">=", "**", "?=", "&&"];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '%' | '$' | '/')
}

/// Ein Wort beginnt mit Buchstabe/Ziffer/_/%/$ oder `/` gefolgt von alnum (Namespace).
fn starts_word(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if c == '/' {
        return i + 1 < chars.len() && chars[i + 1].is_alphanumeric();
    }
    c.is_alphanumeric() || matches!(c, '_' | '%' | '$')
}

/// Prüft ob ab `i` ein Feldsymbol `<name>` ohne Leerzeichen steht; liefert Index von `>`.
fn field_symbol_end(chars: &[char], i: usize) -> Option<usize> {
    let n = chars.len();
    if chars[i] != '<' || i + 1 >= n || !(chars[i + 1].is_alphabetic() || chars[i + 1] == '_') {
        return None;
    }
    let mut j = i + 1;
    while j < n && is_word_char(chars[j]) {
        j += 1;
    }
    if j < n && chars[j] == '>' {
        Some(j)
    } else {
        None
    }
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut toks: Vec<Token> = Vec::new();
    let (mut i, mut line, mut line_start) = (0usize, 1usize, 0usize);

    while i < n {
        let c = chars[i];

        if c == '\n' {
            line += 1;
            i += 1;
            line_start = i;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Kommentarzeile (`*` in Spalte 1) oder Kommentar bis Zeilenende (`"`)
        if (c == '*' && i == line_start) || c == '"' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        let col = i - line_start + 1;
        let tok_line = line;
        let start = i;
        let kind;

        if c == '\'' || c == '`' {
            kind = Kind::Literal;
            i += 1;
            while i < n {
                let d = chars[i];
                i += 1;
                if d == '\n' {
                    line += 1;
                    line_start = i;
                }
                if d == c {
                    if i < n && chars[i] == c {
                        i += 1; // verdoppeltes Anführungszeichen
                    } else {
                        break;
                    }
                }
            }
        } else if c == '|' {
            kind = Kind::Template;
            i += 1;
            while i < n && chars[i] != '|' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if i < n && chars[i] == '\n' {
                    line += 1;
                    line_start = i + 1;
                }
                i += 1;
            }
            i += 1;
        } else if let Some(end) = field_symbol_end(&chars, i) {
            kind = Kind::Word;
            i = end + 1;
            // Komponente am Feldsymbol: <fs>-feld
            while i + 1 < n && chars[i] == '-' && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_') {
                i += 1;
                while i < n && is_word_char(chars[i]) {
                    i += 1;
                }
            }
        } else if starts_word(&chars, i) {
            kind = Kind::Word;
            i += 1;
            while i < n {
                let d = chars[i];
                if is_word_char(d) {
                    i += 1;
                } else if d == '-' && i + 1 < n && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_') {
                    // Komponenten- und Klassenbezeichner: sy-subrc, CLASS-DATA
                    i += 1;
                } else {
                    break;
                }
            }
        } else {
            kind = Kind::Punct;
            let two: String = chars[i..(i + 2).min(n)].iter().collect();
            i += if MULTI_PUNCT.contains(&two.as_str()) { 2 } else { 1 };
        }

        let end = i.min(n);
        let text: String = chars[start..end].iter().collect();
        toks.push(Token { kind, text, line: tok_line, col });
    }

    toks
}
