//! The emulator is the oracle for every headless test in the workspace,
//! so it gets direct behavior tests of its own (mutation testing showed
//! several of its CSI arms could be deleted unnoticed). Each test feeds
//! raw escape bytes and asserts on screen and cursor state.

use eye_declare_engine::test_terminal::TestTerminal;

#[test]
fn plain_text_and_wrap() {
    let mut term = TestTerminal::new(4, 3);
    term.feed(b"abcdef");
    assert_eq!(term.viewport_lines()[0], "abcd");
    assert_eq!(term.viewport_lines()[1], "ef");
    assert_eq!(term.cursor(), (1, 2));
}

#[test]
fn linefeed_at_bottom_scrolls_into_scrollback() {
    let mut term = TestTerminal::new(4, 2);
    term.feed(b"a\r\nb\r\nc");
    assert_eq!(term.scrollback_lines(), vec!["a"]);
    assert_eq!(term.viewport_lines(), vec!["b", "c"]);
}

#[test]
fn cursor_movement_csi_arms() {
    let mut term = TestTerminal::new(10, 5);
    term.feed(b"aaaa\r\nbbbb\r\ncccc");
    // CUU (A) up 2 to (0,4), then CUF (C) forward 1, write at col 5.
    term.feed(b"\x1b[2A\x1b[1Cx");
    assert_eq!(term.viewport_lines()[0], "aaaa x");
    // CUD (B) down 1 to (1,6), CUB (D) back 3, write at col 3.
    term.feed(b"\x1b[1B\x1b[3Dy");
    assert_eq!(term.viewport_lines()[1], "bbby");
    // CNL (E) next line to column 0.
    term.feed(b"\x1b[1Ez");
    assert_eq!(term.viewport_lines()[2], "zccc");
    // CPL (F) previous line to column 0.
    term.feed(b"\x1b[2Fw");
    assert_eq!(term.viewport_lines()[0], "waaa x");
    // Home (H) then erase below (J).
    term.feed(b"\x1b[H\x1b[J");
    assert_eq!(term.viewport_lines(), vec!["", "", "", "", ""]);
}

#[test]
fn erase_below_from_mid_row() {
    let mut term = TestTerminal::new(6, 3);
    term.feed(b"aaaaaa\r\nbbbbbb\r\ncccccc");
    term.feed(b"\x1b[2A"); // to row 0 (col stays past 'a's — clamped at 5)
    term.feed(b"\x1b[3D"); // back to col 2
    term.feed(b"\x1b[J");
    assert_eq!(term.viewport_lines(), vec!["aa", "", ""]);
}

#[test]
fn backspace_moves_left_only() {
    let mut term = TestTerminal::new(4, 2);
    term.feed(b"ab\x08x");
    assert_eq!(term.viewport_lines()[0], "ax");
}

#[test]
fn wide_glyphs_occupy_two_columns() {
    let mut term = TestTerminal::new(6, 2);
    term.feed("日本x".as_bytes());
    assert_eq!(term.viewport_lines()[0], "日本x");
    assert_eq!(term.cursor(), (0, 5));
}

#[test]
fn wide_glyph_at_right_edge_wraps_whole() {
    let mut term = TestTerminal::new(3, 2);
    term.feed("ab日".as_bytes());
    // No room for both columns at the edge: the glyph wraps whole.
    assert_eq!(term.viewport_lines()[0], "ab");
    assert_eq!(term.viewport_lines()[1], "日");
}

#[test]
fn overwriting_half_a_wide_glyph_orphans_the_rest() {
    let mut term = TestTerminal::new(6, 2);
    term.feed("日x".as_bytes());
    // Overwrite the continuation cell (col 1).
    term.feed(b"\r\x1b[1Cy");
    assert_eq!(term.viewport_lines()[0], " yx");
}

#[test]
fn resize_pads_and_clips_width() {
    let mut term = TestTerminal::new(6, 2);
    term.feed(b"abcdef");
    term.resize(4, 2);
    assert_eq!(term.viewport_lines()[0], "abcd");
    term.resize(8, 2);
    assert_eq!(term.viewport_lines()[0], "abcd");
    // New columns are writable after growing.
    term.feed(b"\x1b[H\x1b[7Cz");
    assert_eq!(term.viewport_lines()[0], "abcd   z");
}

#[test]
fn resize_clipping_half_a_wide_glyph_drops_it() {
    let mut term = TestTerminal::new(4, 1);
    term.feed("a日".as_bytes());
    term.resize(2, 1);
    assert_eq!(term.viewport_lines()[0], "a");
}

#[test]
fn resize_height_shrink_scrolls_top_out_to_keep_cursor() {
    let mut term = TestTerminal::new(4, 4);
    term.feed(b"a\r\nb\r\nc"); // cursor on row 2
    term.resize(4, 2);
    assert_eq!(term.scrollback_lines(), vec!["a"]);
    assert_eq!(term.viewport_lines(), vec!["b", "c"]);
    assert_eq!(term.cursor(), (1, 1));
}

#[test]
fn resize_height_grow_adds_blank_rows_below() {
    let mut term = TestTerminal::new(4, 2);
    term.feed(b"a\r\nb");
    term.resize(4, 4);
    assert_eq!(term.viewport_lines(), vec!["a", "b", "", ""]);
    assert_eq!(term.cursor(), (1, 1));
}

#[test]
fn reflow_narrow_splits_hard_line() {
    let mut term = TestTerminal::new(8, 4);
    term.feed(b"abcdefgh\r\nxy");
    term.resize_reflow(4, 4);
    // The content end keeps its screen row (tmux-verified): the growth
    // scrolls out the top even though blank rows remain below.
    assert_eq!(term.scrollback_lines(), vec!["abcd"]);
    assert_eq!(term.viewport_lines(), vec!["efgh", "xy", "", ""]);
    assert_eq!(term.cursor(), (1, 2));
}

#[test]
fn reflow_widen_rejoins_soft_wrapped_line() {
    let mut term = TestTerminal::new(4, 4);
    term.feed(b"abcdef");
    assert_eq!(term.cursor(), (1, 2));
    term.resize_reflow(8, 4);
    assert_eq!(term.viewport_lines(), vec!["abcdef", "", "", ""]);
    // Offset 6 within the rejoined logical line.
    assert_eq!(term.cursor(), (0, 6));
}

#[test]
fn reflow_widen_does_not_rejoin_hard_lines() {
    let mut term = TestTerminal::new(4, 4);
    term.feed(b"abcd\r\nef");
    term.resize_reflow(8, 4);
    assert_eq!(term.viewport_lines(), vec!["abcd", "ef", "", ""]);
    assert_eq!(term.cursor(), (1, 2));
}

#[test]
fn reflow_narrow_overflow_scrolls_top_into_scrollback() {
    let mut term = TestTerminal::new(6, 3);
    term.feed(b"aaaaaa\r\nbbb\r\nccc");
    term.resize_reflow(3, 3);
    assert_eq!(term.scrollback_lines(), vec!["aaa"]);
    assert_eq!(term.viewport_lines(), vec!["aaa", "bbb", "ccc"]);
    assert_eq!(term.cursor(), (2, 2));
}

#[test]
fn reflow_blank_rows_stay_single_at_any_width() {
    let mut term = TestTerminal::new(8, 5);
    term.feed(b"top\r\n\r\nbottom");
    term.resize_reflow(4, 5);
    // Blank line stays one row; content-end pinning scrolls "top" out.
    assert_eq!(term.scrollback_lines(), vec!["top"]);
    assert_eq!(term.viewport_lines(), vec!["", "bott", "om", "", ""]);
}

#[test]
fn reflow_widen_pulls_rows_back_from_scrollback() {
    let mut term = TestTerminal::new(8, 4);
    term.feed(b"abcdefgh\r\nxy");
    term.resize_reflow(4, 4); // "abcd" scrolls out (content-end pinned)
    term.resize_reflow(8, 4); // rejoins and comes back down
    assert_eq!(term.scrollback_lines(), Vec::<String>::new());
    assert_eq!(term.viewport_lines(), vec!["abcdefgh", "xy", "", ""]);
    assert_eq!(term.cursor(), (1, 2));
}

#[test]
fn reflow_erase_breaks_soft_link() {
    let mut term = TestTerminal::new(4, 4);
    term.feed(b"abcdef");
    // Move to the start of the soft-wrapped first row and erase below:
    // the wrap link must not survive into the next reflow.
    term.feed(b"\x1b[H\x1b[J");
    term.feed(b"xy");
    term.resize_reflow(8, 4);
    assert_eq!(term.viewport_lines(), vec!["xy", "", "", ""]);
}
