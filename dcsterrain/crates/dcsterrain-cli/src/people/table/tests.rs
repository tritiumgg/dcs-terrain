use super::*;

fn wide(n: usize) -> Table {
    let mut t = Table::new().col("#", Align::Right).cut("Name", 6);
    for i in 0..n {
        t = t.optional(&format!("Extra {i}"), Align::Right, i as u8);
    }
    let mut row = vec!["1".to_string(), "A rather long name for a place".into()];
    row.extend((0..n).map(|i| format!("{:>12}", i)));
    t.row(row);
    t
}

#[test]
fn numbers_align_right_and_words_left() {
    let mut t = Table::new()
        .col("Name", Align::Left)
        .col("Length (m)", Align::Right);
    t.row(vec!["a".into(), "5".into()]);
    t.row(vec!["bb".into(), "1250".into()]);
    let mut p = Page::new(false);
    p.table(&t);
    assert_eq!(
        p.text,
        "Name  Length (m)\na              5\nbb          1250\n"
    );
}

#[test]
fn a_page_too_narrow_cuts_then_leaves_out_and_says_which() {
    let mut p = Page::new(false);
    p.table(&wide(12));
    for line in p.text.lines() {
        assert!(chars(line) <= WIDTH, "{line}");
    }
    // The lowest kept go first, and the note names them.
    assert!(
        p.text
            .contains("left out to fit 100 characters: Extra 0, Extra 1")
    );
    assert!(p.text.contains("Extra 11"));
    // The name is cut only by what the columns left still need: whole, or
    // to the page's full width.
    let row = p.text.lines().nth(1).unwrap();
    assert!(
        row.contains("A rather long name for a place") || chars(row) == WIDTH,
        "{row}"
    );
}

#[test]
fn long_lines_wrap_at_a_space() {
    let mut p = Page::new(false);
    p.line(&"word ".repeat(40));
    assert!(p.text.lines().count() > 1);
    assert!(p.text.lines().all(|l| chars(l) <= WIDTH));
}

#[test]
fn numbers_round_with_no_negative_zero() {
    assert_eq!(fixed(Some(-0.004), 2), "0.00");
    assert_eq!(fixed(Some(-0.4), 0), "0");
    assert_eq!(fixed(Some(-1.5), 1), "-1.5");
    assert_eq!(fixed(Some(f64::NAN), 1), "—");
    assert_eq!(fixed(None, 1), "—");
}

#[test]
fn color_is_escapes_only_when_asked() {
    let mut t = Table::new().col("A", Align::Left);
    t.row(vec!["1".into()]);
    for color in [false, true] {
        let mut p = Page::new(color);
        p.table(&t);
        p.note("A note.");
        let text = p.finish();
        assert_eq!(text.contains('\x1b'), color, "{text}");
    }
}

#[test]
fn a_cut_cell_ends_in_an_ellipsis() {
    let mut t = Table::new().cut("Name", 8).col("Wide", Align::Left);
    t.row(vec!["x".repeat(40), "y".repeat(80)]);
    let mut p = Page::new(false);
    p.table(&t);
    let row = p.text.lines().nth(1).unwrap();
    assert!(row.starts_with(&format!("{}…  y", "x".repeat(17))), "{row}");
    assert_eq!(chars(row), WIDTH);
}
