//! The terminal-sync state machine: owns cursor tracking, row accounting,
//! and the scrollback boundary.

use ratatui_core::buffer::Buffer;

use crate::escape::CursorState;
use crate::frame::Frame;

/// Synchronizes rendered frames with a real terminal.
///
/// The engine knows nothing about components or element trees: callers hand
/// it complete [`Frame`]s via [`present`](Engine::present) and it returns
/// ANSI escape bytes that bring the terminal in line — claiming new rows
/// with newlines as content grows, streaming rows that are about to become
/// unreachable into scrollback as plain output, diffing everything still
/// addressable, and positioning the hardware cursor.
///
/// Invariant: `cursor`, `emitted_rows`, and `terminal_height` must stay in
/// sync with the real terminal. All output the terminal receives for the
/// managed region must come from this engine.
pub struct Engine {
    width: u16,
    cursor: CursorState,
    prev_frame: Option<Frame>,
    /// Total rows we've "claimed" in the terminal so far.
    emitted_rows: u16,
    /// Terminal height, used to avoid writing to rows in scrollback.
    terminal_height: u16,
    /// Whether the last present parked the hidden cursor on the region's
    /// top row (no cursor hint). See
    /// [`position_cursor`](Engine::position_cursor): a parked cursor
    /// makes a resize-time position report the region top directly.
    parked: bool,
    /// Set by the resize resets: the screen row the erased region starts
    /// at (`None` when unknown). The next present is a repaint of
    /// content that was *already on screen* — it must not stream
    /// overflow rows into scrollback again (a resize drag would dump a
    /// screenful of duplicates per event), and an overflowing tail
    /// takes the visible screen instead of scrolling for rows.
    resumed_at: Option<u16>,
}

impl Engine {
    /// Create an engine for the given content width and terminal height.
    ///
    /// Pass `u16::MAX` as `terminal_height` to disable scrollback
    /// filtering (e.g. when no terminal is attached).
    pub fn new(width: u16, terminal_height: u16) -> Self {
        Self {
            width,
            cursor: CursorState::new(),
            prev_frame: None,
            emitted_rows: 0,
            terminal_height,
            parked: false,
            resumed_at: None,
        }
    }

    /// The present after a resize reset: the region was just erased from
    /// screen row `resumed_at` down and the frame replaces content that
    /// was already visible.
    ///
    /// Unlike a true first render, nothing here belongs in scrollback —
    /// the frame's overflow (if it is taller than the screen) was
    /// already emitted in some earlier form, so re-streaming it would
    /// duplicate a screenful per resize event. An overflowing frame
    /// instead claims the whole screen and paints only its visible
    /// window; rows above the window become unreachable, like any
    /// scrolled content.
    fn repaint_after_reset(
        &mut self,
        new_frame: Frame,
        cursor_hint: Option<(u16, u16)>,
        park_hidden: bool,
    ) -> Vec<u8> {
        let start = self.resumed_at.take().unwrap_or(0);
        let new_height = new_frame.area().height;
        let mut output = Vec::new();

        let rows_below = self.terminal_height.saturating_sub(start);
        let start = if new_height > rows_below && start > 0 {
            // The frame needs more rows than remain below the erase
            // point: take the whole screen — it would end up covered
            // anyway, and claiming rows by scrolling would shove the
            // content above into scrollback line by line.
            output.extend_from_slice(b"\x1b[H\x1b[J");
            0
        } else {
            start
        };

        if new_height <= self.terminal_height.saturating_sub(start) {
            // Fits below the erase point: claim rows without scrolling.
            output.resize(output.len() + new_height as usize - 1, b'\n');
            self.emitted_rows = new_height;
            self.cursor.row = new_height - 1;
        } else {
            // Taller than the screen: fill it, no scrolling. Rows above
            // the visible window are never physically claimed — they're
            // unreachable regardless, exactly as if they had scrolled.
            // (Saturating: a resize can report height zero.)
            output.resize(
                output.len() + self.terminal_height.saturating_sub(1) as usize,
                b'\n',
            );
            self.emitted_rows = new_height;
            self.cursor.row = new_height - 1;
        }
        self.cursor.col = 0;

        let empty = Frame::new(ratatui_core::buffer::Buffer::empty(
            ratatui_core::layout::Rect::new(0, 0, self.width, 0),
        ));
        let scrolled_past = self.emitted_rows.saturating_sub(self.terminal_height);
        let mut diff = new_frame.diff_from(&empty, scrolled_past);
        diff.retain_visible(scrolled_past);
        output.extend_from_slice(&diff.to_escape_sequences(&mut self.cursor));

        self.position_cursor(&mut output, cursor_hint, park_hidden);
        self.prev_frame = Some(new_frame);
        output
    }

    /// The content width frames are expected to be rendered at.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// How many rows have been emitted to the terminal.
    pub fn emitted_rows(&self) -> u16 {
        self.emitted_rows
    }

    /// Update the known terminal height.
    pub fn set_terminal_height(&mut self, height: u16) {
        self.terminal_height = height;
    }

    /// Present a new frame: diff against the previous one and return the
    /// bytes that bring the terminal up to date.
    ///
    /// Handles height growth (emitting newlines to claim rows, streaming
    /// rows that scroll out during the burst), filters cells that are
    /// unreachable in scrollback, and finishes by positioning the hardware
    /// cursor at `cursor_hint` (`(col, row)`, shown) or hiding it (`None`).
    ///
    /// Returns an empty Vec if nothing changed.
    pub fn present(&mut self, new_frame: Frame, cursor_hint: Option<(u16, u16)>) -> Vec<u8> {
        self.present_inner(new_frame, cursor_hint, true)
    }

    /// [`present`](Engine::present), with control over parking the
    /// hidden cursor. [`commit`](Engine::commit) presents the block as
    /// an intermediate frame and immediately claims the row below it —
    /// parking there would be movement churn undone a byte later.
    fn present_inner(
        &mut self,
        new_frame: Frame,
        cursor_hint: Option<(u16, u16)>,
        park_hidden: bool,
    ) -> Vec<u8> {
        let new_height = new_frame.area().height;

        // First render
        if self.prev_frame.is_none() {
            if new_height == 0 {
                self.prev_frame = Some(new_frame);
                return Vec::new();
            }

            if self.resumed_at.is_some() {
                return self.repaint_after_reset(new_frame, cursor_hint, park_hidden);
            }

            // For the first render, we need to claim space and write everything.
            // Create an empty "previous" frame so diff produces all cells.
            let empty = Frame::new(ratatui_core::buffer::Buffer::empty(
                ratatui_core::layout::Rect::new(0, 0, self.width, 0),
            ));
            let stream_until = new_height.saturating_sub(self.terminal_height);
            let mut diff = new_frame.diff_from(&empty, stream_until);

            let mut output = Vec::new();
            self.stream_rows_into_scrollback(&new_frame, 0, stream_until, &mut output);

            // Emit newlines to claim rows (minus 1 because the cursor
            // is already on the first row)
            let new_rows_needed = new_height.saturating_sub(self.emitted_rows);
            if new_rows_needed > 0 {
                let newline_count = if self.emitted_rows == 0 {
                    new_rows_needed.saturating_sub(1)
                } else {
                    new_rows_needed
                } as usize;
                if self.emitted_rows > 0 && newline_count > 0 {
                    output.push(b'\r');
                    self.cursor.col = 0;
                }
                output.resize(output.len() + newline_count, b'\n');
                self.emitted_rows = new_height;
                self.cursor.row = new_height.saturating_sub(1);
                self.cursor.col = 0;
            }

            // Filter out cells in scrollback (unreachable by cursor)
            let scrolled_past = self.emitted_rows.saturating_sub(self.terminal_height);
            diff.retain_visible(scrolled_past);

            let escape_bytes = diff.to_escape_sequences(&mut self.cursor);
            output.extend_from_slice(&escape_bytes);

            self.position_cursor(&mut output, cursor_hint, park_hidden);
            self.prev_frame = Some(new_frame);
            return output;
        }

        // Subsequent renders. Rows already past the scrollback boundary
        // are immutable; skip them at diff time instead of filtering
        // afterward.
        let prev = self.prev_frame.as_ref().unwrap();
        let already_scrolled = self.emitted_rows.saturating_sub(self.terminal_height);
        let mut diff = new_frame.diff_from(prev, already_scrolled);

        if diff.is_empty() && !diff.grew() {
            // Even if content didn't change, cursor position might have
            // (e.g., cursor moved within an input field)
            let mut output = Vec::new();
            self.position_cursor(&mut output, cursor_hint, park_hidden);
            self.prev_frame = Some(new_frame);
            return output;
        }

        let mut output = Vec::new();
        let old_scrolled_past = self.emitted_rows.saturating_sub(self.terminal_height);
        let new_scrolled_past = new_height.saturating_sub(self.terminal_height);
        self.stream_rows_into_scrollback(
            &new_frame,
            old_scrolled_past,
            new_scrolled_past,
            &mut output,
        );

        // If the frame grew, we may need to claim more terminal rows.
        // Only emit newlines for rows beyond what we've already claimed —
        // if the frame previously shrank, some emitted rows are unused
        // and can absorb part (or all) of the growth without new newlines.
        let new_rows_needed = new_height.saturating_sub(self.emitted_rows);
        if new_rows_needed > 0 && self.emitted_rows == 0 {
            // Growing out of an empty region: the cursor already sits on
            // the row that becomes row 0, so claim with n-1 newlines —
            // the same adjustment the first-render path makes. Claiming n
            // would leave cursor.row == emitted_rows (one past the
            // bottom), and the next growth's move-to-bottom would snap
            // tracking up a row without emitting any movement, painting
            // everything one row below where the engine believes it is.
            output.push(b'\r');
            self.cursor.col = 0;
            output.resize(output.len() + new_rows_needed as usize - 1, b'\n');
            self.emitted_rows = new_height;
            self.cursor.row = new_height - 1;
        } else if new_rows_needed > 0 {
            // Move cursor to the bottom of our current region first
            // (it might be somewhere in the middle from the last write)
            let current_bottom = self.emitted_rows.saturating_sub(1);
            debug_assert!(
                self.cursor.row <= current_bottom,
                "cursor tracked below the region bottom"
            );
            if self.cursor.row < current_bottom {
                let down = current_bottom - self.cursor.row;
                output.extend_from_slice(format!("\x1b[{}B", down).as_bytes());
            }
            self.cursor.row = current_bottom;

            // Carriage return to column 0 before emitting newlines.
            // \x1b[nB (CUD) and \n (LF) only move vertically — neither
            // resets the column. Without this, cursor.col = 0 would
            // diverge from the terminal's actual column, causing the
            // first diff cell on the new row to be written at the wrong
            // position (wherever the cursor was left after the previous
            // render's escape sequences).
            output.push(b'\r');
            self.cursor.col = 0;

            // Emit newlines to claim new rows
            output.resize(output.len() + new_rows_needed as usize, b'\n');
            self.emitted_rows += new_rows_needed;
            self.cursor.row += new_rows_needed;
        }

        // Filter out cells in scrollback (unreachable by cursor)
        let scrolled_past = self.emitted_rows.saturating_sub(self.terminal_height);
        diff.retain_visible(scrolled_past);

        let escape_bytes = diff.to_escape_sequences(&mut self.cursor);
        output.extend_from_slice(&escape_bytes);

        self.position_cursor(&mut output, cursor_hint, park_hidden);
        self.prev_frame = Some(new_frame);
        output
    }

    /// Reset only the managed region for a width change: erase from the
    /// region's reachable top downward, then drop tracking state. Content
    /// above the region — committed blocks, in v2 usage — is untouched,
    /// matching the committed-is-immutable semantics (spec O3): like any
    /// printed output, it keeps whatever wrapping the terminal's own
    /// reflow gave it.
    ///
    /// Best-effort by nature: the terminal reflowed existing content when
    /// the width changed, so pre-reflow row arithmetic may be off by the
    /// reflow delta; the next [`present`](Engine::present) repaints the
    /// whole region, which bounds any artifact to one frame.
    pub fn reset_region(&mut self, new_width: u16) -> Vec<u8> {
        let mut output = Vec::new();
        // The reachable top of the region (rows above this are in
        // scrollback and behave as committed regardless).
        let top = self.emitted_rows.saturating_sub(self.terminal_height);
        crate::escape::write_relative_move(&mut output, &mut self.cursor, top, 0);
        output.extend_from_slice(b"\x1b[J");

        self.width = new_width;
        self.cursor = CursorState::new();
        self.prev_frame = None;
        self.emitted_rows = 0;
        self.resumed_at = Some(0);
        output
    }

    /// [`reset_region`](Engine::reset_region) with ground truth: the
    /// cursor's absolute screen position `(col, row)` (0-based) as
    /// reported by the terminal *after* it reflowed for the new width.
    ///
    /// A stale-arithmetic erase after a reflow either starts too low
    /// (leaving unrepaintable fragments above the region) or too high
    /// (destroying committed rows). The report re-anchors us:
    ///
    /// - A parked cursor (hidden — presents without a cursor hint park
    ///   it) sits on the region's top row, so the report *is* the erase
    ///   target — exact on every terminal, reflowing or truncating.
    /// - A visible cursor sits at the app's hint, and the distance up
    ///   to the region top is recomputed from the previous frame: each
    ///   region row is a hard line the terminal re-wraps at the new
    ///   width, occupying `ceil(content/width)` physical rows, trailing
    ///   blanks trimmed (terminals drop them when re-wrapping).
    ///   Trimming errs toward starting the erase *lower*: a model
    ///   mismatch leaves a stale fragment rather than destroying
    ///   committed output. (Truncating terminals — xterm, urxvt,
    ///   screen — don't re-wrap, so this path over-erases there; every
    ///   terminal Atuin targets re-wraps, and the parked path is exact
    ///   everywhere.)
    ///
    /// The erase then targets the region top absolutely, which also
    /// stops the erase point from drifting across a resize drag.
    pub fn reset_region_anchored(&mut self, new_width: u16, cursor: (u16, u16)) -> Vec<u8> {
        let (_, cpr_row) = cursor;
        let distance = if self.parked {
            0
        } else {
            self.reflow_distance(new_width)
        };

        let start = (cpr_row as i32 - distance as i32).max(0);

        let mut output = Vec::new();
        output.extend_from_slice(format!("\x1b[{};1H", start + 1).as_bytes());
        output.extend_from_slice(b"\x1b[J");

        self.width = new_width;
        self.cursor = CursorState::new();
        self.prev_frame = None;
        self.emitted_rows = 0;
        self.resumed_at = Some(start.min(u16::MAX as i32) as u16);
        output
    }

    /// Physical rows between the region top and the cursor after the
    /// terminal re-wrapped our rows (each one a hard line, created by
    /// its own linefeed) at `new_width`.
    fn reflow_distance(&self, new_width: u16) -> u32 {
        let new_width = new_width.max(1) as u32;
        if new_width >= self.width as u32 {
            // Hard lines never rejoin on widen; one physical row each.
            return self.cursor.row as u32;
        }
        let prev_height = self
            .prev_frame
            .as_ref()
            .map(|f| f.area().height)
            .unwrap_or(0);
        let mut distance: u32 = 0;
        for row in 0..self.cursor.row {
            let content = if row < prev_height {
                self.prev_frame
                    .as_ref()
                    .map(|f| f.content_width_of_row(row) as u32)
                    .unwrap_or(0)
            } else {
                // Claimed rows below the frame are blank.
                0
            };
            distance += content.div_ceil(new_width).max(1);
        }
        // The cursor's own row: the fragment its column lands on.
        distance + self.cursor.col as u32 / new_width
    }

    /// Reset for a width change: clear the visible screen (scrollback is
    /// preserved), home the cursor, and drop all tracking state. The caller
    /// should follow up with a fresh [`present`](Engine::present).
    ///
    /// After a width change the terminal has already reflowed existing
    /// content, making cursor tracking invalid; clear-and-redraw is the
    /// reliable fallback. (v1 semantics — it repaints its whole retained
    /// tree afterward. v2 callers use [`reset_region`](Engine::reset_region)
    /// instead, because committed content can never be repainted.)
    pub fn reset(&mut self, new_width: u16) -> Vec<u8> {
        // \x1b[2J = clear entire screen
        // \x1b[H  = cursor to row 1, col 1 (home)
        // This does NOT clear scrollback (\x1b[3J would do that).
        self.width = new_width;
        self.cursor = CursorState::new();
        self.prev_frame = None;
        self.emitted_rows = 0;
        self.resumed_at = Some(0);
        b"\x1b[2J\x1b[H".to_vec()
    }

    /// Commit finished rows above the live tail (the v2 timeline surface).
    ///
    /// Presents `rows` as the whole frame — replacing the tail, not
    /// stacking above it — then shifts them out of tracking via
    /// [`commit_scrolled`](Engine::commit_scrolled). Presenting the block
    /// alone is what makes sealing cheap and correct: in the common case
    /// the block *was* the live tail a moment ago, so identical rows diff
    /// to nothing and rows already burst-streamed into scrollback are
    /// never re-emitted. (The earlier stack-above-the-tail formulation
    /// re-streamed that overlap whenever block + tail exceeded the
    /// terminal height: the transcript duplicated into scrollback and the
    /// screen jumped by the block height.)
    ///
    /// The region is left empty; callers present the tail again afterward
    /// (the runtime does so in the same flush). From the next present on,
    /// the committed rows behave like any earlier terminal output:
    /// unaddressable, immutable, drifting into scrollback naturally.
    pub fn commit(&mut self, rows: &Buffer) -> Vec<u8> {
        let committed_height = rows.area.height;
        if committed_height == 0 {
            return Vec::new();
        }

        let mut output = self.present_inner(Frame::new(rows.clone()), None, false);

        // The next region origin is the row below the block. Make it
        // physically exist with the cursor on it before the shift: an LF
        // at the screen bottom scrolls one row, elsewhere it only moves
        // down. This claims exactly the genuinely-new rows.
        if self.cursor.row < committed_height {
            output.push(b'\r');
            self.cursor.col = 0;
            let down = committed_height - self.cursor.row;
            output.resize(output.len() + down as usize, b'\n');
            self.cursor.row = committed_height;
            self.emitted_rows = self.emitted_rows.max(committed_height + 1);
        }

        output.extend_from_slice(&self.commit_scrolled(committed_height));
        output
    }

    /// Drop the top `committed_height` rows from tracking: a pure origin
    /// shift — those rows become unaddressable and will never be repainted.
    /// Subsequent diffs only cover the remaining active region.
    ///
    /// Invariant: the physical cursor must sit at or below the new origin
    /// when the shift happens, or region coordinates would desync from the
    /// terminal. If the cursor is parked above (e.g. the presenting frame's
    /// cursor hint pointed into the committed rows), this emits a relative
    /// move down to the new origin first — hence the returned bytes, which
    /// are empty in the common case.
    #[must_use]
    pub fn commit_scrolled(&mut self, committed_height: u16) -> Vec<u8> {
        if committed_height == 0 {
            return Vec::new();
        }

        let mut output = Vec::new();
        if self.cursor.row < committed_height {
            crate::escape::write_relative_move(&mut output, &mut self.cursor, committed_height, 0);
        }

        if let Some(ref prev) = self.prev_frame {
            self.prev_frame = Some(prev.slice_top_rows(committed_height));
        }
        self.emitted_rows = self.emitted_rows.saturating_sub(committed_height);
        self.cursor.row = self.cursor.row.saturating_sub(committed_height);
        output
    }

    /// Park the cursor for shell handoff: column 0 on the row after the
    /// last content row, trailing blank rows erased.
    ///
    /// Call after a final [`present`](Engine::present). When the tail
    /// shrank before exit (e.g., an input box cleared) the vacated rows
    /// are reclaimed so the shell prompt appears immediately after the
    /// content; when the tail still has content, a fresh line is opened
    /// below it so whatever the process prints next (`println!`, the
    /// shell prompt) never lands on top of the tail. Idempotent.
    pub fn finalize(&mut self) -> Vec<u8> {
        if self.emitted_rows == 0 {
            return Vec::new();
        }
        let current_height = self
            .prev_frame
            .as_ref()
            .map(|f| f.area().height)
            .unwrap_or(0);

        // Respect the scrollback boundary: rows above `scrolled_past` are
        // in terminal scrollback and unreachable by cursor movement.  If we
        // tried to move there the terminal would clamp us, desyncing our
        // cursor tracking.  Only erase rows we can actually reach.
        let scrolled_past = self.emitted_rows.saturating_sub(self.terminal_height);
        let target_row = current_height.max(scrolled_past);

        let mut output = Vec::new();

        if target_row < self.emitted_rows {
            // Trailing blank rows below the content: erasing them leaves
            // the cursor exactly at the handoff position.
            //
            // Use CR first to clear any pending-wrap state, then CPL
            // (Cursor Previous Line) which moves up N lines and to
            // column 0 atomically — more reliable than CUU + CR for
            // terminals with edge-case wrap behavior.
            output.extend_from_slice(b"\r");
            if self.cursor.row > target_row {
                let up = self.cursor.row - target_row;
                output.extend_from_slice(format!("\x1b[{}F", up).as_bytes());
            } else if self.cursor.row < target_row {
                let down = target_row - self.cursor.row;
                output.extend_from_slice(format!("\x1b[{}E", down).as_bytes());
            }

            // Erase from cursor to end of screen
            output.extend_from_slice(b"\x1b[J");

            self.emitted_rows = target_row;
        } else if self.cursor.row < self.emitted_rows {
            // Content fills the region: open a fresh line below it. LF —
            // not CNL, which clamps instead of scrolling at the bottom
            // margin, where the new line may not physically exist yet.
            output.extend_from_slice(b"\r");
            let bottom = self.emitted_rows - 1;
            if self.cursor.row < bottom {
                let down = bottom - self.cursor.row;
                output.extend_from_slice(format!("\x1b[{}E", down).as_bytes());
            }
            output.extend_from_slice(b"\n");
        } else {
            // Already parked below the content (a repeated finalize).
            return Vec::new();
        }

        self.cursor.row = self.emitted_rows;
        self.cursor.col = 0;
        output
    }

    /// Stream rows that would be in terminal scrollback by the time the
    /// current frame is fully claimed.
    ///
    /// The normal growth path first emits blank newlines and then paints
    /// visible cells by cursor movement. Rows that scroll out during those
    /// newlines cannot be reached afterward, so burst appends would leave
    /// blank terminal scrollback. This method writes those rows as normal
    /// terminal output before claiming the rest of the frame. It starts at
    /// the old scrollback boundary, so insertions above a persistent footer
    /// overwrite the soon-to-scroll visible rows before advancing the terminal.
    fn stream_rows_into_scrollback(
        &mut self,
        frame: &Frame,
        start: u16,
        end: u16,
        output: &mut Vec<u8>,
    ) {
        let frame_height = frame.area().height;
        let end = end.min(frame_height);
        if start >= end {
            return;
        }

        crate::escape::write_relative_move(output, &mut self.cursor, start, 0);

        for row in start..end {
            frame.write_committed_row(row, output, &mut self.cursor);
            output.extend_from_slice(b"\r\n");
            self.cursor.row = row.saturating_add(1);
            self.cursor.col = 0;
            self.emitted_rows = self.emitted_rows.max(self.cursor.row.saturating_add(1));
        }
    }

    /// Append escape sequences to position (and show) the terminal cursor
    /// at `hint` (`(col, row)`), or hide it when `hint` is `None`.
    fn position_cursor(
        &mut self,
        output: &mut Vec<u8>,
        hint: Option<(u16, u16)>,
        park_hidden: bool,
    ) {
        if let Some((col, row)) = hint {
            crate::escape::write_relative_move(output, &mut self.cursor, row, col);
            // Show cursor at the component's cursor position
            output.extend_from_slice(b"\x1b[?25h");
            self.parked = false;
        } else if !park_hidden {
            output.extend_from_slice(b"\x1b[?25l");
            self.parked = false;
        } else {
            // No cursor hint: hide the cursor and park it on the
            // region's reachable top row. The position is invisible, but
            // it makes a later resize's position report *be* the region
            // top: the cursor rides its logical line through any reflow,
            // and stays put in a truncating terminal — either way no
            // arithmetic can drift. (Left at the last painted cell it
            // would sit on the content's final row, which reflowing
            // terminals pin to its screen row — a report from there is
            // blind to re-wrapping above it.)
            let top = self.emitted_rows.saturating_sub(self.terminal_height);
            crate::escape::write_relative_move(output, &mut self.cursor, top, 0);
            output.extend_from_slice(b"\x1b[?25l");
            self.parked = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::layout::Rect;

    fn buffer_with_lines(width: u16, lines: &[&str]) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, lines.len() as u16));
        for (y, line) in lines.iter().enumerate() {
            buf.set_stringn(
                0,
                y as u16,
                line,
                width as usize,
                ratatui_core::style::Style::default(),
            );
        }
        buf
    }

    #[test]
    fn commit_scrolled_is_silent_when_cursor_below_origin() {
        let mut engine = Engine::new(10, 24);
        // A visible cursor on row 1 sits at/below the new origin 1.
        let _ = engine.present(
            Frame::new(buffer_with_lines(10, &["aaa", "bbb"])),
            Some((0, 1)),
        );
        let bytes = engine.commit_scrolled(1);
        assert!(bytes.is_empty());
        assert_eq!(engine.emitted_rows(), 1);
    }

    #[test]
    fn commit_scrolled_moves_parked_cursor_to_origin() {
        let mut engine = Engine::new(10, 24);
        // No hint: the hidden cursor parks on the region top, above the
        // new origin — the shift must move it down first.
        let _ = engine.present(Frame::new(buffer_with_lines(10, &["aaa", "bbb"])), None);
        let bytes = engine.commit_scrolled(1);
        assert!(!bytes.is_empty());
        assert_eq!(engine.emitted_rows(), 1);
    }

    #[test]
    fn commit_scrolled_moves_cursor_parked_in_committed_rows() {
        let mut engine = Engine::new(10, 24);
        // Cursor hint parks the physical cursor at the very top row.
        let _ = engine.present(
            Frame::new(buffer_with_lines(10, &["aaa", "bbb"])),
            Some((0, 0)),
        );
        let bytes = engine.commit_scrolled(1);
        assert!(
            !bytes.is_empty(),
            "cursor above the new origin must be moved down before the shift"
        );
        assert_eq!(engine.cursor.row, 0, "cursor is at the new origin");
        assert_eq!(engine.emitted_rows(), 1);
    }

    #[test]
    fn commit_replaces_tail_and_leaves_an_empty_region() {
        let mut engine = Engine::new(10, 24);
        let _ = engine.present(Frame::new(buffer_with_lines(10, &["> tail"])), None);

        let _ = engine.commit(&buffer_with_lines(10, &["block one", "block two"]));

        // The block is gone from the engine's world; the region is the
        // single claimed origin row, empty until the caller re-presents
        // the tail (as the runtime does in the same flush).
        assert_eq!(engine.emitted_rows(), 1);
        let prev = engine.prev_frame.as_ref().unwrap();
        assert_eq!(prev.area().height, 0);
    }

    #[test]
    fn commit_of_the_presented_tail_emits_no_content_bytes() {
        // The seal case: the block IS the previous tail. Nothing needs
        // repainting — the only bytes claim the new origin row.
        let mut engine = Engine::new(10, 24);
        let content = buffer_with_lines(10, &["aaa", "bbb", "ccc"]);
        let _ = engine.present(Frame::new(content.clone()), None);

        let bytes = engine.commit(&content);
        let printable: String = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            !printable.contains("aaa") && !printable.contains("ccc"),
            "sealing identical content must not repaint it, got {printable:?}"
        );
        assert!(
            printable.ends_with('\n'),
            "seal should end by claiming the origin row, got {printable:?}"
        );
        assert_eq!(engine.emitted_rows(), 1);
    }

    #[test]
    fn zero_height_resize_does_not_panic() {
        // Terminals can report a zero-height size mid-drag; the repaint
        // must not underflow its row claim.
        let mut engine = Engine::new(10, 24);
        let _ = engine.present(Frame::new(buffer_with_lines(10, &["aaa", "bbb"])), None);
        engine.set_terminal_height(0);
        let _ = engine.reset_region_anchored(10, (0, 0));
        let bytes = engine.present(Frame::new(buffer_with_lines(10, &["aaa", "bbb"])), None);
        let _ = bytes;
    }

    #[test]
    fn commit_with_empty_rows_is_a_no_op() {
        let mut engine = Engine::new(10, 24);
        let _ = engine.present(Frame::new(buffer_with_lines(10, &["> tail"])), None);
        let bytes = engine.commit(&Buffer::empty(Rect::new(0, 0, 10, 0)));
        assert!(bytes.is_empty());
        assert_eq!(engine.emitted_rows(), 1);
    }

    #[test]
    fn commit_before_any_present_works() {
        let mut engine = Engine::new(10, 24);
        let _ = engine.commit(&buffer_with_lines(10, &["hello"]));
        // The claimed origin row below the block is the whole region.
        assert_eq!(engine.emitted_rows(), 1);
        let prev = engine.prev_frame.as_ref().unwrap();
        assert_eq!(prev.area().height, 0);
    }
}
