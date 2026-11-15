// "> emoji": every emoji in a grid (res/emoji.txt, built into the exe). Type to search by
// name; Enter copies. The ones used lately come first. Same as cpp/emoji.inl.

use super::panels::*;
use super::*;

struct EmojiDef {
    ch: String,
    name: String,
    group: usize,
}
#[derive(Default)]
struct EmojiUi {
    all: Vec<EmojiDef>,
    groups: Vec<String>,
    items: Vec<usize>, // what the grid shows: index into all
    cats: Vec<String>,
    grid: grid::CardGrid,
    recent: Vec<String>,
}
static mut EM: Option<EmojiUi> = None;
fn em() -> &'static mut EmojiUi {
    unsafe {
        (*std::ptr::addr_of_mut!(EM)).get_or_insert_with(|| EmojiUi { grid: grid::CardGrid::new(10, 42.0, 0.0), ..Default::default() })
    }
}

fn load() {
    let e = em();
    if !e.all.is_empty() {
        return;
    }
    for line in include_str!("../../res/emoji.txt").split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if let Some(g) = line.strip_prefix('@') {
            e.groups.push(g.to_string());
            continue;
        }
        let Some(tab) = line.find('\t') else { continue };
        e.all.push(EmojiDef { ch: line[..tab].to_string(), name: line[tab + 1..].to_string(), group: e.groups.len().saturating_sub(1) });
    }
    if let Some(rec) = read_string(SETTINGS_KEY, w!("RecentEmoji")) {
        e.recent = rec.split(' ').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    }
}

pub fn fill(first: bool) {
    load();
    let e = em();
    e.items.clear();
    e.cats.clear();
    let a = ps().arg.to_lowercase();
    let words: Vec<&str> = a.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        for r in &e.recent {
            if let Some(i) = e.all.iter().position(|d| &d.ch == r) {
                e.items.push(i);
                e.cats.push("Recently used".into());
            }
        }
    }
    for (i, d) in e.all.iter().enumerate() {
        let g = e.groups.get(d.group).map(|g| g.to_lowercase()).unwrap_or_default();
        if words.iter().all(|w| d.name.contains(w) || g.contains(w)) {
            e.items.push(i);
            e.cats.push(if words.is_empty() { e.groups.get(d.group).cloned().unwrap_or_default() } else { "Results".into() });
        }
    }
    let cats = e.cats.clone();
    e.grid.layout(&cats);
    if first {
        st().sel = 0;
        e.grid.scroll_to(0.0, true);
    }
}
pub fn count() -> i32 {
    em().items.len() as i32
}
pub fn height() -> f32 {
    if em().items.is_empty() {
        120.0
    } else {
        em().grid.panel_h()
    }
}

pub fn draw(top: f32, dt: f32, alpha: f32, _t: f64) {
    let s = st();
    let th = s.th;
    let e = em();
    let g = &mut e.grid;
    let (cw, ch) = (g.card_w(), g.prev_h);
    let sel_name = if s.sel >= 0 && (s.sel as usize) < e.items.len() { e.all[e.items[s.sel as usize]].name.clone() } else { String::new() };
    panel_header("Emoji", &if sel_name.is_empty() { String::new() } else { format!("{}   ↵ copy", sel_name) }, top, alpha);
    let c_top = top + grid::TOP_H;
    let c_bottom = top + s.panel_h;
    g.animate(dt);
    unsafe { rt().PushAxisAlignedClip(&rect(MARGIN, c_top, MARGIN + bar_w(), c_bottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    if e.items.is_empty() {
        dl("No emoji with that name", &s.fmt_row, MARGIN + GRID_PAD + 2.0, c_top, 300.0, 40.0, with_a(th.dim, alpha));
    }
    for (title, hy) in &g.headers {
        let y = c_top + hy - g.off;
        if y > c_bottom || y + g.head_h < c_top {
            continue;
        }
        dl(title, &s.fmt_header, MARGIN + GRID_PAD + 2.0, y, 400.0, g.head_h - 2.0, with_a(th.dim, alpha * 0.9));
    }
    for k in 0..g.cells.len() {
        let (item, x, cy) = g.cells[k];
        let y = c_top + cy - g.off;
        g.cell_sel[k] = damp(g.cell_sel[k], if k as i32 == s.sel { 1.0 } else { 0.0 }, 18.0, dt);
        if y > c_bottom || y + g.row_h() < c_top {
            continue;
        }
        let sel = g.cell_sel[k];
        let hit = rect(x, y.max(c_top), x + cw, (y + ch).min(c_bottom));
        if hit.bottom > hit.top {
            s.hits.push((hit, k as i32));
        }
        if sel > 0.01 {
            fill_rr(rect(x, y, x + cw, y + ch), 9.0, with_a(th.highlight, alpha * sel * 2.0));
            set_color(with_a(th.accent, alpha * sel));
            unsafe {
                rt().DrawRoundedRectangle(&rrect(rect(x + 0.75, y + 0.75, x + cw - 0.75, y + ch - 0.75), 8.5), solid(), 1.5, None::<&ID2D1StrokeStyle>)
            };
        }
        let em_ch = &e.all[e.items[item]].ch;
        if let Some(l) = make_layout(&ws(em_ch), &s.fmt_emoji, cw, ch) {
            let sk = 1.0 + 0.12 * sel;
            let old = get_transform();
            set_transform(&m_mul(&m_scale(sk, sk, x + cw / 2.0, y + ch / 2.0), &old));
            set_color(with_a(th.text, alpha));
            unsafe { rt().DrawTextLayout(pt(x, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
            set_transform(&old);
        }
    }
    unsafe { rt().PopAxisAlignedClip() };
    let view = c_bottom - c_top;
    if g.max_off() > 0.5 {
        grid::draw_scroll_bar(c_top + 4.0, view - 8.0, g.off / g.max_off(), view / (g.content_h + 6.0), alpha, dt);
    }
}

pub fn copy(k: i32) {
    let e = em();
    if k < 0 || k as usize >= e.items.len() {
        return;
    }
    let ch = e.all[e.items[k as usize]].ch.clone();
    if !set_clipboard_text(&ch) {
        return;
    }
    flash(&format!("Copied {}", ch));
    e.recent.retain(|r| *r != ch);
    e.recent.insert(0, ch);
    e.recent.truncate(20);
    write_string("RecentEmoji", &e.recent.join(" "));
}
pub fn enter(sel: i32, _: bool) {
    copy(sel);
}
pub fn key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    let Some(k) = em().grid.key(vk, shift, st().sel) else { return false };
    select(k);
    let sel = st().sel;
    em().grid.ensure_visible(sel, false);
    true
}
pub fn click(x: f32, y: f32) -> bool {
    let k = hit_item_at(x, y);
    if k >= 0 {
        st().sel = k;
        copy(k);
    }
    true
}
pub fn wheel(steps: i32) {
    let g = &mut em().grid;
    let t = g.off_target + steps as f32 * 60.0;
    g.scroll_to(t, false);
}
pub fn scroll_frac(f: f32) {
    let g = &mut em().grid;
    let m = g.max_off();
    g.scroll_to(f * m, true);
}
