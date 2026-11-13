use abapgit_inspect::abap::{tokenize, Kind};

fn texts(src: &str) -> Vec<String> {
    tokenize(src).into_iter().map(|t| t.text).collect()
}

#[test]
fn skips_comment_line_in_column_one() {
    assert_eq!(texts("* ein Kommentar\nDATA a."), vec!["DATA", "a", "."]);
}

#[test]
fn skips_inline_quote_comment() {
    assert_eq!(texts("DATA a. \" rest\nDATA b."), vec!["DATA", "a", ".", "DATA", "b", "."]);
}

#[test]
fn star_not_in_column_one_is_punct() {
    let toks = tokenize("  * x");
    assert_eq!(toks[0].text, "*");
    assert_eq!(toks[0].kind, Kind::Punct);
}

#[test]
fn string_literal_with_doubled_quote() {
    let toks = tokenize("x = 'it''s'.");
    assert_eq!(toks[2].kind, Kind::Literal);
    assert_eq!(toks[2].text, "'it''s'");
}

#[test]
fn backtick_literal() {
    let toks = tokenize("x = `a b`.");
    assert_eq!(toks[2].kind, Kind::Literal);
    assert_eq!(toks[2].text, "`a b`");
}

#[test]
fn string_template_is_one_token() {
    let toks = tokenize("x = |a { b } c|.");
    assert_eq!(toks[2].kind, Kind::Template);
    assert_eq!(toks[2].text, "|a { b } c|");
}

#[test]
fn component_selector_stays_one_word() {
    assert_eq!(texts("IF sy-subrc = 0."), vec!["IF", "sy-subrc", "=", "0", "."]);
}

#[test]
fn class_static_access_is_one_word() {
    assert_eq!(texts("CLASS-DATA x."), vec!["CLASS-DATA", "x", "."]);
}

#[test]
fn object_arrow_is_punct() {
    let toks = tokenize("me->run( ).");
    assert_eq!(toks[1].text, "->");
    assert_eq!(toks[1].kind, Kind::Punct);
}

#[test]
fn static_arrow_is_punct() {
    let toks = tokenize("zcl=>run( ).");
    assert_eq!(toks[1].text, "=>");
}

#[test]
fn field_symbol_is_word() {
    let toks = tokenize("ASSIGN <fs_x> TO <y>.");
    assert_eq!(toks[1].text, "<fs_x>");
    assert_eq!(toks[1].kind, Kind::Word);
    assert_eq!(toks[3].text, "<y>");
}

#[test]
fn less_than_with_spaces_is_punct() {
    let toks = tokenize("IF a < b.");
    assert_eq!(toks[2].text, "<");
    assert_eq!(toks[2].kind, Kind::Punct);
}

#[test]
fn positions_are_reported() {
    let toks = tokenize("DATA a.\n  WRITE b.");
    let write = toks.iter().find(|t| t.text == "WRITE").unwrap();
    assert_eq!(write.line, 2);
    assert_eq!(write.col, 3);
}

#[test]
fn word_matching_is_case_insensitive() {
    let toks = tokenize("data a.");
    assert!(toks[0].is_word("DATA"));
}
