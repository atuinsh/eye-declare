//! Differential fuzz: drive a whole app headlessly with arbitrary
//! push/tail sequences and check the VTE emulator's screen against a naive
//! transcript model after every step.
//!
//! The invariant is the library's core promise: the terminal always shows
//! every committed block, in push order, exactly once, with the current
//! tail below them and nothing else. The emulator models chars as width 1
//! and never resizes, so content is short width-1 ASCII lines and geometry
//! is fixed per run — wide-char and resize coverage needs TestTerminal
//! support first.

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use eye_declare::{App, Ctx, Element, Runtime, col, text};
use eye_declare_engine::test_terminal::TestTerminal;
use libfuzzer_sys::fuzz_target;

/// A line short enough (<= 6 cols) to never wrap at the narrowest
/// generated terminal (8 cols), from printable ASCII only.
#[derive(Debug, Clone)]
struct Line(String);

impl<'a> Arbitrary<'a> for Line {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let len = u.int_in_range(0..=6)?;
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            s.push(char::from(u.int_in_range(32u8..=126)?));
        }
        Ok(Line(s))
    }
}

#[derive(Debug, Arbitrary)]
enum Op {
    /// Replace the live tail (0..=4 lines: always shorter than the
    /// terminal, so the region never outgrows the viewport on its own).
    SetTail(Line, Option<(Line, Option<(Line, Option<Line>)>)>),
    /// Commit a block of 0..=3 lines into scrollback.
    Push(Vec<Line>),
    /// Re-present without a model change (animation tick path).
    Present,
}

#[derive(Debug, Arbitrary)]
struct Session {
    width_raw: u8,
    height_raw: u8,
    ops: Vec<Op>,
}

struct FuzzApp {
    tail: Vec<String>,
}

#[derive(Clone)]
enum Msg {
    SetTail(Vec<String>),
    Push(Vec<String>),
}

impl App for FuzzApp {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::SetTail(lines) => self.tail = lines,
            Msg::Push(lines) => {
                ctx.push(col().children(lines.iter().map(|l| text(l.as_str()))));
            }
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col().children(self.tail.iter().map(|l| text(l.as_str())))
    }
}

fn tail_lines(op: &Op) -> Option<Vec<String>> {
    match op {
        Op::SetTail(a, rest) => {
            let mut lines = vec![a.0.clone()];
            if let Some((b, rest)) = rest {
                lines.push(b.0.clone());
                if let Some((c, rest)) = rest {
                    lines.push(c.0.clone());
                    if let Some(d) = rest {
                        lines.push(d.0.clone());
                    }
                }
            }
            Some(lines)
        }
        _ => None,
    }
}

fuzz_target!(|session: Session| {
    let width = 8 + (session.width_raw % 25) as u16; // 8..=32
    let height = 8 + (session.height_raw % 13) as u16; // 8..=20

    let mut rt = Runtime::new(FuzzApp { tail: Vec::new() }, width, height);
    let mut term = TestTerminal::new(width as usize, height as usize);

    let (bytes, _) = rt.startup();
    term.feed(&bytes);

    // The naive model the terminal must always agree with.
    let mut committed: Vec<String> = Vec::new();
    let mut tail: Vec<String> = Vec::new();

    for op in &session.ops {
        match op {
            Op::SetTail(..) => {
                let lines = tail_lines(op).unwrap();
                tail = lines.clone();
                let (bytes, _) = rt.process(Msg::SetTail(lines));
                term.feed(&bytes);
            }
            Op::Push(lines) => {
                let lines: Vec<String> = lines.iter().take(3).map(|l| l.0.clone()).collect();
                committed.extend(lines.iter().map(|l| l.trim_end().to_string()));
                let (bytes, _) = rt.process(Msg::Push(lines));
                term.feed(&bytes);
            }
            Op::Present => {
                let bytes = rt.present();
                term.feed(&bytes);
            }
        }

        let actual: Vec<String> = term
            .scrollback_lines()
            .into_iter()
            .chain(term.viewport_lines())
            .collect();
        let expected: Vec<&str> = committed
            .iter()
            .map(|l| l.as_str())
            .chain(tail.iter().map(|l| l.trim_end()))
            .collect();

        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                actual.get(i).map(|s| s.as_str()),
                Some(*want),
                "row {i} diverged after {op:?}\nexpected: {expected:?}\nterminal: {actual:?}",
            );
        }
        for (i, line) in actual.iter().enumerate().skip(expected.len()) {
            assert!(
                line.is_empty(),
                "row {i} should be blank after {op:?}, got {line:?}\nterminal: {actual:?}",
            );
        }
    }
});
