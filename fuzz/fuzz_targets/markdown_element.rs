//! Parser fuzz for the markdown element: arbitrary UTF-8 through
//! pulldown-cmark and the layout pass must not panic at any width, and
//! `height` must honor its contract — deterministic (the RefCell cache and
//! a fresh parse agree) and exactly what `render` is sized with.

#![no_main]

use eye_declare::Element;
use eye_declare::markdown::markdown;
use libfuzzer_sys::fuzz_target;
use ratatui_core::{buffer::Buffer, layout::Rect};

fuzz_target!(|input: (u8, &str)| {
    let (width_raw, src) = input;
    let width = 1 + (width_raw % 80) as u16; // 1..=80

    let element = markdown(src);
    let height = element.height(width);

    // Cached and recomputed answers must agree, for this and other widths.
    assert_eq!(height, element.height(width), "height not deterministic");
    let other = 1 + width / 2;
    let fresh = markdown(src);
    assert_eq!(
        element.height(other),
        fresh.height(other),
        "cache poisoned by earlier width",
    );

    // Render at the exact claimed size (capped to keep memory sane —
    // pathological inputs can legitimately claim thousands of rows).
    let height = height.min(500);
    if height > 0 {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        element.render(area, &mut buf);
    }
});
