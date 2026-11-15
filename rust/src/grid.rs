// Card grids ("> font", "> caret"): cards in rows under a title for each category, scrolled
// inside the panel. Also the panel's scroll bar, which can be dragged. Same as cpp/grid.inl.

use super::*;

pub const TOP_H: f32 = GRID_PAD + GRID_HEADER_H - 4.0; // the fixed header above the cards

#[derive(Default)]
pub struct CardGrid {
    pub cols: usize,
    pub prev_h: f32,
    pub name_h: f32,
    pub row_gap: f32,
    pub head_h: f32,
    pub cells: Vec<(usize, f32, f32)>, // position in the grid's list, x in bar space, y from the top of the scrolling area
    pub headers: Vec<(String, f32)>,   // category title, y
    pub cell_sel: Vec<f32>,
    pub content_h: f32,
    pub off: f32,
    pub off_target: f32,
}

impl CardGrid {
    pub fn new(cols: usize, prev_h: f32, name_h: f32) -> Self {
        CardGrid { cols, prev_h, name_h, row_gap: 8.0, head_h: 28.0, ..Default::default() }
    }
    pub fn card_w(&self) -> f32 {
        (bar_w() - 2.0 * GRID_PAD - (self.cols as f32 - 1.0) * CARD_GAP) / self.cols as f32
    }
    pub fn row_h(&self) -> f32 {
        self.prev_h + self.name_h + self.row_gap
    }
    pub fn panel_h(&self) -> f32 {
        MAX_PANEL_H.min(TOP_H + self.content_h + 6.0)
    }
    pub fn view_h(&self) -> f32 {
        self.panel_h() - TOP_H
    }
    pub fn max_off(&self) -> f32 {
        (self.content_h + 6.0 - self.view_h()).max(0.0)
    }
    pub fn scroll_to(&mut self, o: f32, instant: bool) {
        self.off_target = o.clamp(0.0, self.max_off());
        if instant {
            self.off = self.off_target;
        }
    }
    pub fn animate(&mut self, dt: f32) {
        self.off = damp(self.off, self.off_target, 18.0, dt);
        if (self.off - self.off_target).abs() < 0.3 {
            self.off = self.off_target;
        }
    }

    // One category per item, in order; each new category starts a row under its title.
    pub fn layout(&mut self, cats: &[String]) {
        self.cells.clear();
        self.headers.clear();
        let (row_h, card_w) = (self.row_h(), self.card_w());
        let mut y = 0.0;
        let mut col = 0usize;
        for k in 0..cats.len() {
            if k == 0 || cats[k] != cats[k - 1] {
                if col > 0 {
                    y += row_h;
                    col = 0;
                }
                self.headers.push((cats[k].clone(), y));
                y += self.head_h;
            }
            self.cells.push((k, MARGIN + GRID_PAD + col as f32 * (card_w + CARD_GAP), y));
            col += 1;
            if col == self.cols {
                col = 0;
                y += row_h;
            }
        }
        if col > 0 {
            y += row_h;
        }
        self.content_h = if cats.is_empty() { 44.0 } else { y };
        self.cell_sel = vec![0.0; self.cells.len()];
    }

    // Keeps a card (and its category title when it's the first row) in view.
    pub fn ensure_visible(&mut self, k: i32, instant: bool) {
        if k < 0 || k as usize >= self.cells.len() {
            return;
        }
        let y = self.cells[k as usize].2;
        let view = self.view_h();
        let (top, bottom) = (y - self.head_h - 4.0, y + self.row_h());
        if top < self.off_target {
            self.scroll_to(top, instant);
        } else if bottom > self.off_target + view {
            self.scroll_to(bottom - view, instant);
        }
    }

    // Up / Down: the card in the next row nearest to this one's column.
    pub fn vertical(&self, k: i32, dir: i32) -> i32 {
        if k < 0 || k as usize >= self.cells.len() {
            return k;
        }
        let (_, x, y) = self.cells[k as usize];
        let mut best = k;
        let mut best_row = if dir > 0 { 1e9f32 } else { -1e9f32 };
        let mut best_dx = 1e9f32;
        for (i, &(_, cx, cy)) in self.cells.iter().enumerate() {
            if if dir > 0 { cy <= y + 1.0 } else { cy >= y - 1.0 } {
                continue;
            }
            let closer = if dir > 0 { cy < best_row - 1.0 } else { cy > best_row + 1.0 };
            let same = (cy - best_row).abs() <= 1.0;
            let dx = (cx - x).abs();
            if closer || (same && dx < best_dx) {
                best = i as i32;
                best_row = cy;
                best_dx = dx;
            }
        }
        best
    }

    // Arrow keys / Page Up / Page Down: the new selection, or None when the key isn't ours.
    pub fn key(&self, vk: VIRTUAL_KEY, shift: bool, sel: i32) -> Option<i32> {
        let n = self.cells.len() as i32;
        if n == 0 {
            return None;
        }
        let k = match vk {
            VK_UP => self.vertical(sel, -1),
            VK_DOWN => self.vertical(sel, 1),
            VK_LEFT if !shift => sel - 1,
            VK_RIGHT if !shift => sel + 1,
            VK_PRIOR => (0..3).fold(sel, |k, _| self.vertical(k, -1)),
            VK_NEXT => (0..3).fold(sel, |k, _| self.vertical(k, 1)),
            _ => return None,
        };
        Some(k.clamp(0, n - 1))
    }
}

// ---- scroll bar ------------------------------------------------------------------------
#[derive(Default, Clone, Copy)]
pub struct ScrollBar {
    pub shown: bool, // as last drawn (bar space)
    pub track_top: f32,
    pub track_h: f32,
    pub thumb_y: f32,
    pub thumb_h: f32,
    pub drag: bool, // the thumb is being dragged
    pub grab: f32,  // where on the thumb it was grabbed
    pub hot: bool,  // under the mouse
    pub w: f32,     // drawn width (grows under the mouse)
}

fn sb() -> &'static mut ScrollBar {
    &mut st().sb
}

// frac: how far down the content is scrolled (0..1); visible: how much of it fits (0..1).
pub fn draw_scroll_bar(track_top: f32, track_h: f32, frac: f32, visible: f32, alpha: f32, dt: f32) {
    let th = st().th;
    let b = sb();
    let thumb_h = (track_h * visible.clamp(0.0, 1.0)).max(24.0);
    let thumb_y = track_top + (track_h - thumb_h) * frac.clamp(0.0, 1.0);
    b.shown = true;
    b.track_top = track_top;
    b.track_h = track_h;
    b.thumb_y = thumb_y;
    b.thumb_h = thumb_h;
    let wide = b.hot || b.drag;
    if b.w == 0.0 {
        b.w = 3.0;
    }
    b.w = damp(b.w, if wide { 6.0 } else { 3.0 }, 18.0, dt);
    let x1 = MARGIN + bar_w() - 3.0;
    let x0 = x1 - b.w;
    if wide {
        // a faint track so it's clear where it can go
        fill_rr(rect(x0, track_top, x1, track_top + track_h), b.w / 2.0, with_a(th.dim, alpha * 0.12));
    }
    let a = if b.drag { 0.85 } else if wide { 0.7 } else { 0.45 };
    fill_rr(rect(x0, thumb_y, x1, thumb_y + thumb_h), b.w / 2.0, with_a(th.dim, alpha * a));
}

pub fn hit(x: f32, y: f32) -> bool {
    let b = sb();
    b.shown && x >= MARGIN + bar_w() - 16.0 && x <= MARGIN + bar_w() + 2.0 && y >= b.track_top - 4.0 && y <= b.track_top + b.track_h + 4.0
}
pub fn on_thumb(y: f32) -> bool {
    let b = sb();
    y >= b.thumb_y && y <= b.thumb_y + b.thumb_h
}
// Where the thumb's top goes for the mouse at y, as 0..1 of the track.
pub fn frac(y: f32) -> f32 {
    let b = sb();
    let room = (b.track_h - b.thumb_h).max(1.0);
    ((y - b.grab - b.track_top) / room).clamp(0.0, 1.0)
}
