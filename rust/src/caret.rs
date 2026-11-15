// Caret ("> caret"): its color, a glow, and little effects when typing, deleting and moving.
// Cards like the theme picker, one section per setting; every card plays its effect. The
// selected card is also tried out on the real caret. Same as cpp/caret.inl.

use super::*;

pub const CS_COLOR: usize = 0;
pub const CS_GLOW: usize = 1;
pub const CS_TYPING: usize = 2;
pub const CS_DELETING: usize = 3;
pub const CS_MOVING: usize = 4;
const CS_COUNT: usize = 5;
const SEC_TITLE: [&str; CS_COUNT] = ["Color", "Glow", "When typing", "When deleting", "When moving"];
const SEC_REG: [&str; CS_COUNT] = ["CaretColor", "CaretGlow", "CaretTyping", "CaretDeleting", "CaretMoving"];
// rgb: -1 the theme's accent, -2 the text color, -3 rainbow
const COLORS: [(&str, i32); 12] = [
    ("Theme", -1),
    ("Text", -2),
    ("Ice", 0x9AD8FF),
    ("Blue", 0x3B82F6),
    ("Violet", 0x8B5CF6),
    ("Pink", 0xEC4899),
    ("Red", 0xEF4444),
    ("Orange", 0xF97316),
    ("Gold", 0xFACC15),
    ("Lime", 0x84CC16),
    ("Mint", 0x2DD4BF),
    ("Rainbow", -3),
];
const GLOWS: [&str; 4] = ["Off", "Soft", "Strong", "Pulse"];
const TYPINGS: [&str; 4] = ["None", "Sparkle", "Confetti", "Bubbles"];
const DELETINGS: [&str; 4] = ["None", "Explosion", "Shatter", "Dust"];
const MOVINGS: [&str; 4] = ["Glide", "Trail", "Comet", "Plain"];
const OPT_N: [usize; CS_COUNT] = [COLORS.len(), 4, 4, 4, 4];

fn opt_name(sec: usize, v: usize) -> &'static str {
    match sec {
        CS_COLOR => COLORS[v].0,
        CS_GLOW => GLOWS[v],
        CS_TYPING => TYPINGS[v],
        CS_DELETING => DELETINGS[v],
        _ => MOVINGS[v],
    }
}

// Every burst is a pure function of its age, so the cards can play them on a loop too.
const BK_NONE: i32 = 0;
const BK_SPARKLE: i32 = 1;
const BK_CONFETTI: i32 = 2;
const BK_BUBBLES: i32 = 3;
const BK_EXPLOSION: i32 = 4;
const BK_SHATTER: i32 = 5;
const BK_DUST: i32 = 6;
const BK_COMET: i32 = 7;
const BK_GHOST: i32 = 8;

#[derive(Clone, Copy)]
struct Burst {
    kind: i32,
    x: f32,
    y: f32,
    t: f64, // when it started
    seed: u32,
    w: f32, // width it spreads over (deleted text)
    col: D2D1_COLOR_F,
    dir: f32, // comet: which way the tail goes
}

// UI-thread state (State::cfx)
pub struct CaretUi {
    pub set: [usize; CS_COUNT],    // what's chosen (registry DWORDs)
    pub items: Vec<(usize, usize)>, // the cards shown: section, value
    pub grid: grid::CardGrid,
    bursts: Vec<Burst>,
    events: Vec<(bool, usize, usize)>, // typed?, index, count: turned into bursts when the text is drawn
    seed: u32,
    fx_last_x: f32, // where the last trail / comet bit was dropped
    star: Option<ID2D1PathGeometry1>,
    shard: Option<ID2D1PathGeometry1>,
}

impl Default for CaretUi {
    fn default() -> Self {
        CaretUi {
            set: [0; CS_COUNT],
            items: Vec::new(),
            grid: grid::CardGrid::new(4, 56.0, 24.0),
            bursts: Vec::new(),
            events: Vec::new(),
            seed: 1,
            fx_last_x: -1e9,
            star: None,
            shard: None,
        }
    }
}

fn cu() -> &'static mut CaretUi {
    &mut st().cfx
}

pub fn load_settings() {
    for s in 0..CS_COUNT {
        cu().set[s] = (read_dword(SEC_REG[s], 0) as usize).min(OPT_N[s] - 1);
    }
}

// The setting in effect: the selected card is tried out on the real caret while "> caret" is open.
fn opt_eff(sec: usize) -> usize {
    let s = st();
    let u = cu();
    if s.mode == Mode::Caret && s.sel >= 0 && (s.sel as usize) < u.items.len() && u.items[s.sel as usize].0 == sec {
        return u.items[s.sel as usize].1;
    }
    u.set[sec]
}

fn hue(h: f64) -> D2D1_COLOR_F {
    let h = h - h.floor();
    let r = clamp01((h * 6.0 - 3.0).abs() - 1.0);
    let g = clamp01(2.0 - (h * 6.0 - 2.0).abs());
    let b = clamp01(2.0 - (h * 6.0 - 4.0).abs());
    D2D1_COLOR_F { r: 0.25 + 0.75 * r, g: 0.25 + 0.75 * g, b: 0.25 + 0.75 * b, a: 1.0 }
}
fn color_of(v: usize, t_now: f64) -> D2D1_COLOR_F {
    match COLORS[v.min(COLORS.len() - 1)].1 {
        -1 => st().th.accent,
        -2 => st().th.text,
        -3 => hue(t_now * 0.22),
        rgb => c(rgb as u32, 1.0),
    }
}
fn color_now(t_now: f64) -> D2D1_COLOR_F {
    color_of(opt_eff(CS_COLOR), t_now)
}
fn lerp_white(col: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    lerp_c(col, c(0xFFFFFF, 1.0), t)
}

// ---- the list ---------------------------------------------------------------------
pub fn build_list(filter: &str) {
    let words: Vec<&str> = filter.split(' ').filter(|w| !w.is_empty()).collect();
    let u = cu();
    u.items.clear();
    let mut cats = Vec::new();
    for s in 0..CS_COUNT {
        for v in 0..OPT_N[s] {
            let hay = format!("{} {}", opt_name(s, v), SEC_TITLE[s]).to_lowercase();
            if words.iter().all(|w| hay.contains(w)) {
                u.items.push((s, v));
                cats.push(SEC_TITLE[s].to_string());
            }
        }
    }
    u.grid.layout(&cats);
}

pub fn count() -> i32 {
    cu().items.len() as i32
}
pub fn panel_h() -> f32 {
    cu().grid.panel_h()
}

pub fn enter() {
    let s = st();
    let u = cu();
    s.sel = u.items.iter().position(|&(sec, v)| sec == CS_COLOR && v == u.set[CS_COLOR]).unwrap_or(0) as i32;
    u.grid.scroll_to(0.0, true);
    u.grid.ensure_visible(s.sel, true);
}

fn apply(k: i32) {
    let u = cu();
    if k < 0 || k as usize >= u.items.len() {
        return;
    }
    let (sec, v) = u.items[k as usize];
    u.set[sec] = v;
    write_dword(SEC_REG[sec], v as u32);
}
pub fn apply_sel() {
    apply(st().sel);
}

// "Pink · Sparkle" (or "Pink · 3 effects") for the commands list.
pub fn summary() -> String {
    let u = cu();
    let mut extra = Vec::new();
    if u.set[CS_GLOW] != 0 {
        extra.push(format!("{} glow", GLOWS[u.set[CS_GLOW]]));
    }
    for sec in [CS_TYPING, CS_DELETING, CS_MOVING] {
        if u.set[sec] != 0 {
            extra.push(opt_name(sec, u.set[sec]).to_string());
        }
    }
    let col = COLORS[u.set[CS_COLOR]].0;
    match extra.len() {
        0 => col.to_string(),
        1 => format!("{} · {}", col, extra[0]),
        n => format!("{} · {} effects", col, n),
    }
}

// ---- effects ----------------------------------------------------------------------------
pub fn typed(index: usize, count: usize) {
    cu().events.push((true, index, count));
}
pub fn deleted(index: usize, count: usize) {
    cu().events.push((false, index, count));
}
pub fn clear_fx() {
    let u = cu();
    u.bursts.clear();
    u.events.clear();
    u.fx_last_x = -1e9;
}

fn burst_life(kind: i32) -> f32 {
    match kind {
        BK_SPARKLE => 0.65,
        BK_CONFETTI => 0.95,
        BK_BUBBLES => 1.0,
        BK_EXPLOSION => 0.6,
        BK_SHATTER => 0.85,
        BK_DUST => 0.75,
        BK_COMET => 0.4,
        BK_GHOST => 0.22,
        _ => 0.0,
    }
}

fn rnd(seed: u32, i: i32, k: i32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E3779B1) ^ (i as u32).wrapping_mul(0x85EBCA77) ^ (k as u32).wrapping_mul(0xC2B2AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A2D39);
    h ^= h >> 15;
    (h & 0xFFFFFF) as f32 / 16777216.0
}

fn make_poly(p: &[D2D_POINT_2F]) -> Option<ID2D1PathGeometry1> {
    unsafe {
        let geo = st().d2d.CreatePathGeometry().ok()?;
        let sink = geo.Open().ok()?;
        sink.BeginFigure(p[0], D2D1_FIGURE_BEGIN_FILLED);
        sink.AddLines(&p[1..]);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close().ok()?;
        Some(geo)
    }
}
fn ensure_geometry() {
    let u = cu();
    if u.star.is_none() {
        // a four-pointed twinkle
        let p: Vec<D2D_POINT_2F> = (0..8)
            .map(|j| {
                let a = j as f32 * 0.785398 - 1.570796;
                let r = if j % 2 == 1 { 0.3 } else { 1.0 };
                pt(r * a.cos(), r * a.sin())
            })
            .collect();
        u.star = make_poly(&p);
    }
    if u.shard.is_none() {
        u.shard = make_poly(&[pt(0.0, -1.0), pt(0.9, 0.7), pt(-0.8, 0.55)]);
    }
}

fn m_rotate(deg: f32) -> Matrix3x2 {
    let (s, c) = deg.to_radians().sin_cos();
    Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: 0.0, M32: 0.0 }
}
fn fill_shape(g: &Option<ID2D1PathGeometry1>, x: f32, y: f32, size: f32, deg: f32, col: D2D1_COLOR_F, base: &Matrix3x2) {
    let Some(g) = g else { return };
    if size <= 0.05 {
        return;
    }
    let m = m_mul(&m_mul(&m_mul(&m_scale(size, size, 0.0, 0.0), &m_rotate(deg)), &m_translate(x, y)), base);
    set_transform(&m);
    set_color(col);
    unsafe { rt().FillGeometry(g, solid(), None::<&ID2D1Brush>) };
    set_transform(base);
}
fn dot(x: f32, y: f32, r: f32, col: D2D1_COLOR_F) {
    if r <= 0.05 || col.a <= 0.003 {
        return;
    }
    set_color(col);
    unsafe { rt().FillEllipse(&D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r }, solid()) };
}

const CONFETTI: [u32; 5] = [0xFF5E7E, 0xFFD166, 0x4ADE80, 0x60A5FA, 0xC084FC];
const FIRE: [u32; 4] = [0xFFF1A8, 0xFFD166, 0xFF8C42, 0xFF4D4D];

// k: size (1 on the bar, a little less on the cards); alpha: fades the whole burst.
fn draw_burst(b: &Burst, t_now: f64, k: f32, alpha: f32) {
    let life = burst_life(b.kind);
    let age = (t_now - b.t) as f32;
    if age < 0.0 || age >= life {
        return;
    }
    let u = age / life;
    let e = 1.0 - (1.0 - u) * (1.0 - u) * (1.0 - u); // eased progress
    let base = get_transform();
    ensure_geometry();
    let (star, shard) = (&cu().star, &cu().shard);
    let r = |i: i32, kk: i32| rnd(b.seed, i, kk);
    match b.kind {
        BK_SPARKLE => {
            if u < 0.3 {
                dot(b.x, b.y, (3.0 + u * 12.0) * k, with_a(lerp_white(b.col, 0.6), alpha * (1.0 - u / 0.3) * 0.45));
            }
            for i in 0..7 {
                let ang = r(i, 0) * std::f32::consts::TAU;
                let d = (10.0 + r(i, 1) * 24.0) * e * k;
                let tw = 0.75 + 0.25 * (age * 30.0 + i as f32 * 1.7).sin();
                let sz = (2.4 + r(i, 2) * 2.8) * k * (1.0 - u) * tw;
                fill_shape(star, b.x + ang.cos() * d, b.y + ang.sin() * d * 0.85, sz, r(i, 3) * 90.0 + age * 160.0, with_a(lerp_white(b.col, 0.45), alpha * (1.0 - u * u)), &base);
            }
        }
        BK_CONFETTI => {
            for i in 0..10 {
                let vx = (r(i, 0) - 0.5) * 130.0;
                let vy = -(60.0 + r(i, 1) * 90.0);
                let px = b.x + vx * age * k;
                let py = b.y + (vy * age + 0.5 * 330.0 * age * age) * k;
                let rot = r(i, 2) * 360.0 + age * (r(i, 3) - 0.5) * 900.0;
                let flip = 0.3 + 0.7 * (age * 9.0 + i as f32).cos().abs();
                let a = alpha * if u < 0.7 { 1.0 } else { (1.0 - u) / 0.3 };
                set_transform(&m_mul(&m_mul(&m_mul(&m_scale(k, k * flip, 0.0, 0.0), &m_rotate(rot)), &m_translate(px, py)), &base));
                fill_rect(rect(-1.3, -2.4, 1.3, 2.4), c(CONFETTI[(i as u32).wrapping_add(b.seed) as usize % 5], a));
                set_transform(&base);
            }
        }
        BK_BUBBLES => {
            for i in 0..5 {
                let uu = clamp01(((age - r(i, 0) * 0.25) / 0.75) as f64);
                if uu <= 0.0 || uu >= 1.0 {
                    continue;
                }
                let eu = 1.0 - (1.0 - uu) * (1.0 - uu);
                let px = b.x + ((r(i, 2) - 0.5) * 14.0 + (uu * 6.0 + i as f32).sin() * 3.0) * k;
                let py = b.y - (6.0 + r(i, 1) * 30.0) * eu * k;
                let rad = (1.6 + r(i, 3) * 3.4) * k * (0.6 + 0.4 * uu);
                set_color(with_a(lerp_white(b.col, 0.3), alpha * (1.0 - uu) * 0.9));
                unsafe { rt().DrawEllipse(&D2D1_ELLIPSE { point: pt(px, py), radiusX: rad, radiusY: rad }, solid(), 1.0, None::<&ID2D1StrokeStyle>) };
                dot(px - rad * 0.35, py - rad * 0.35, (rad * 0.22).max(0.5), with_a(c(0xFFFFFF, 1.0), alpha * (1.0 - uu) * 0.8));
            }
        }
        BK_EXPLOSION => {
            let sc = 1.0 + 0.08 * (b.w / 7.0).min(8.0);
            if u < 0.2 {
                dot(b.x, b.y, (5.0 + 22.0 * u) * k * sc, c(0xFFF1A8, alpha * (1.0 - u / 0.2) * 0.85));
            }
            set_color(c(0xFF8C42, alpha * (1.0 - u) * 0.9));
            let rr = (4.0 + 24.0 * e) * k * sc;
            unsafe {
                rt().DrawEllipse(&D2D1_ELLIPSE { point: pt(b.x, b.y), radiusX: rr, radiusY: rr * 0.8 }, solid(), (3.0 * (1.0 - u) + 0.4) * k, None::<&ID2D1StrokeStyle>)
            };
            for i in 0..14 {
                let ang = r(i, 0) * std::f32::consts::TAU;
                let d = (8.0 + r(i, 1) * 34.0) * e * k * sc;
                let ox = (r(i, 5) - 0.5) * b.w;
                let rad = (1.4 + r(i, 2) * 2.4) * (1.0 - u) * k;
                dot(b.x + ox + ang.cos() * d, b.y + ang.sin() * d * 0.8, rad, c(FIRE[i as usize % 4], alpha * (1.0 - u)));
            }
        }
        BK_SHATTER => {
            for i in 0..9 {
                let vx = (r(i, 0) - 0.5) * 140.0;
                let vy = -(30.0 + r(i, 1) * 80.0);
                let px = b.x + (r(i, 5) - 0.5) * b.w + vx * age * k;
                let py = b.y + (r(i, 6) - 0.5) * 10.0 * k + (vy * age + 0.5 * 380.0 * age * age) * k;
                fill_shape(shard, px, py, (2.4 + r(i, 4) * 3.4) * k, r(i, 2) * 360.0 + age * (r(i, 3) - 0.5) * 720.0, with_a(b.col, alpha * (1.0 - u * u)), &base);
            }
        }
        BK_DUST => {
            for i in 0..16 {
                let px = b.x + ((r(i, 0) - 0.5) * (b.w + 6.0) + (r(i, 2) - 0.5) * 30.0 * age) * k;
                let py = b.y + ((r(i, 1) - 0.5) * 14.0 - (6.0 + r(i, 3) * 22.0) * age) * k;
                dot(px, py, (0.8 + r(i, 4) * 1.6) * k * (1.0 - u * 0.5), with_a(b.col, alpha * (1.0 - u) * 0.85));
            }
        }
        BK_COMET => {
            let px = b.x + b.dir * (4.0 + r(0, 0) * 14.0) * e * k;
            let py = b.y + (r(0, 1) - 0.5) * 12.0 * e * k;
            dot(px, py, (1.0 + r(0, 2) * 1.8) * (1.0 - u) * k, with_a(lerp_white(b.col, 0.3), alpha * (1.0 - u)));
        }
        BK_GHOST => {
            let h = CARET_H * k;
            fill_rr(rect(b.x - CARET_W / 2.0, b.y - h / 2.0, b.x + CARET_W / 2.0, b.y + h / 2.0), 1.0, with_a(b.col, alpha * 0.45 * (1.0 - u)));
        }
        _ => {}
    }
}

// A soft light around the caret.
fn draw_glow(x: f32, cy: f32, h: f32, col: D2D1_COLOR_F, op: f32, glow: usize, t_now: f64, k: f32) {
    if glow == 0 || op <= 0.01 {
        return;
    }
    let a = match glow {
        1 => 0.32,
        2 => 0.6,
        _ => 0.2 + 0.4 * (0.5 + 0.5 * (t_now * std::f64::consts::TAU / 1.4).sin() as f32),
    };
    let rx = if glow == 2 { 16.0 } else { 11.0 } * k;
    let ry = h * if glow == 2 { 0.95 } else { 0.8 };
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: with_a(col, a * op) },
        D2D1_GRADIENT_STOP { position: 0.45, color: with_a(col, a * op * 0.35) },
        D2D1_GRADIENT_STOP { position: 1.0, color: with_a(col, 0.0) },
    ];
    unsafe {
        let Ok(coll) = rt().CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: pt(x, cy), gradientOriginOffset: pt(0.0, 0.0), radiusX: rx, radiusY: ry };
        if let Ok(br) = rt().CreateRadialGradientBrush(&props, None, &coll) {
            rt().FillEllipse(&D2D1_ELLIPSE { point: pt(x, cy), radiusX: rx, radiusY: ry }, &br);
        }
    }
}

fn typing_kind(v: usize) -> i32 {
    match v {
        1 => BK_SPARKLE,
        2 => BK_CONFETTI,
        3 => BK_BUBBLES,
        _ => BK_NONE,
    }
}
fn deleting_kind(v: usize) -> i32 {
    match v {
        1 => BK_EXPLOSION,
        2 => BK_SHATTER,
        3 => BK_DUST,
        _ => BK_NONE,
    }
}
fn delete_color(kind: i32, caret: D2D1_COLOR_F) -> D2D1_COLOR_F {
    match kind {
        BK_SHATTER => st().th.text,
        BK_DUST => st().th.dim,
        _ => caret,
    }
}

// The real caret, called by draw_text_area once the text is drawn (outside its clip, so glows
// and bits can spill over the edges). tx: where the text starts on screen; op: blink; h: height.
pub fn draw_caret_and_fx(lay: &IDWriteTextLayout, full_len: usize, tx: f32, cy: f32, op: f32, h: f32, t_now: f64) {
    let s = st();
    let u = cu();
    let col = color_now(t_now);
    let events = std::mem::take(&mut u.events);
    for (typed, index, count) in events {
        let idx = index.min(full_len);
        if typed {
            let kind = typing_kind(opt_eff(CS_TYPING));
            if kind != BK_NONE {
                let x = tx + caret_x_for(lay, (idx + count).min(full_len), full_len);
                u.bursts.push(Burst { kind, x, y: cy, t: t_now, seed: u.seed, w: 0.0, col, dir: 0.0 });
                u.seed = u.seed.wrapping_add(1);
            }
        } else {
            let kind = deleting_kind(opt_eff(CS_DELETING));
            let w = count.min(12) as f32 * 7.0; // the deleted text was about this wide, right of idx
            if kind != BK_NONE {
                let x = tx + caret_x_for(lay, idx, full_len) + w / 2.0;
                u.bursts.push(Burst { kind, x, y: cy, t: t_now, seed: u.seed, w, col: delete_color(kind, col), dir: 0.0 });
                u.seed = u.seed.wrapping_add(1);
            }
        }
    }

    let cx = tx + s.caret_x;
    let moving = opt_eff(CS_MOVING);
    let is_moving = (s.caret_x - s.trail_x).abs() > 1.5;
    if !is_moving {
        u.fx_last_x = cx;
    } else if (cx - u.fx_last_x).abs() > 3.0 {
        if moving == 1 {
            u.bursts.push(Burst { kind: BK_GHOST, x: u.fx_last_x, y: cy, t: t_now, seed: u.seed, w: 0.0, col, dir: 0.0 });
            u.seed = u.seed.wrapping_add(1);
        }
        if moving == 2 {
            for _ in 0..2 {
                let dir = if s.trail_x > s.caret_x { 1.0 } else { -1.0 };
                u.bursts.push(Burst { kind: BK_COMET, x: cx, y: cy, t: t_now, seed: u.seed, w: 0.0, col, dir });
                u.seed = u.seed.wrapping_add(1);
            }
        }
        u.fx_last_x = cx;
    }

    if op > 0.003 {
        draw_glow(cx, cy, h, col, op, opt_eff(CS_GLOW), t_now, 1.0);
        let x0 = tx + s.caret_x.min(s.trail_x) - CARET_W / 2.0;
        let x1 = tx + s.caret_x.max(s.trail_x) + CARET_W / 2.0;
        if (moving == 0 || moving == 2) && x1 - x0 > CARET_W + 0.6 {
            // the soft smear behind a gliding caret
            fill_rr(rect(x0, cy - h * 0.38, x1, cy + h * 0.38), 1.0, with_a(col, op * 0.28));
        }
        fill_rr(rect(cx - CARET_W / 2.0, cy - h / 2.0, cx + CARET_W / 2.0, cy + h / 2.0), 1.0, with_a(col, op));
    }

    if u.bursts.len() > 400 {
        let extra = u.bursts.len() - 400;
        u.bursts.drain(..extra);
    }
    let bursts = std::mem::take(&mut u.bursts);
    for b in &bursts {
        draw_burst(b, t_now, 1.0, 1.0);
    }
    cu().bursts = bursts.into_iter().filter(|b| ((t_now - b.t) as f32) < burst_life(b.kind)).collect();
}

// ---- the cards ---------------------------------------------------------------------------
fn text_width(s: &str) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let Some(l) = make_layout(&ws(s), &st().fmt_text, 1000.0, 40.0) else { return 0.0 };
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe {
        let _ = l.GetMetrics(&mut m);
    }
    m.widthIncludingTrailingWhitespace
}

fn mini_caret(x: f32, cy: f32, col: D2D1_COLOR_F, a: f32) {
    fill_rr(rect(x - 1.0, cy - 9.0, x + 1.0, cy + 9.0), 1.0, with_a(col, a));
}

// What a card shows: a few letters and a caret playing that option.
fn draw_demo(sec: usize, val: usize, x: f32, y: f32, w: f32, h: f32, t_now: f64, k: usize) {
    let th = st().th;
    let tx = x + 14.0;
    let cy = y + h / 2.0;
    let kk = 0.85;
    const WORD: &str = "Hello";
    let text = |s: &str, a: f32| {
        if !s.is_empty() {
            draw_layout(&ws(s), &st().fmt_text, tx, cy - 28.0, w - 20.0, 56.0, with_a(th.text, a));
        }
    };
    let set = cu().set;
    let color_v = if sec == CS_COLOR { val } else { set[CS_COLOR] };
    let col = color_of(color_v, t_now + if sec == CS_COLOR { 0.0 } else { k as f64 * 0.05 });
    let glow = if sec == CS_GLOW { val } else if sec == CS_COLOR { set[CS_GLOW] } else { 0 };
    let blink = 0.55 + 0.45 * (((t_now + k as f64 * 0.13) % 1.1) / 1.1 * std::f64::consts::TAU).cos() as f32;
    let seed = |j: usize| (j * 7 + k * 31 + 1) as u32;
    match sec {
        CS_COLOR | CS_GLOW => {
            let cx = tx + text_width("Abc") + 1.0;
            text("Abc", 0.9);
            draw_glow(cx, cy, 18.0, col, 1.0, glow, t_now, kk);
            mini_caret(cx, cy, col, if glow != 0 { 1.0 } else { blink.max(0.35) });
        }
        CS_TYPING => {
            let ph = (t_now + k as f64 * 0.37) % 2.6;
            let shown = (0..5).filter(|&j| ph >= 0.15 + j as f64 * 0.3).count();
            text(&WORD[..shown], 0.9);
            let kind = typing_kind(val);
            if kind != BK_NONE {
                for j in 0..shown {
                    let age = ph - (0.15 + j as f64 * 0.3);
                    let b = Burst { kind, x: tx + text_width(&WORD[..j + 1]) + 1.0, y: cy, t: t_now - age, seed: seed(j), w: 0.0, col, dir: 0.0 };
                    draw_burst(&b, t_now, kk, 1.0);
                }
            }
            mini_caret(tx + text_width(&WORD[..shown]) + 1.0, cy, col, 1.0);
        }
        CS_DELETING => {
            let ph = (t_now + k as f64 * 0.37) % 2.6;
            let gone = (0..5).filter(|&j| ph >= 0.5 + j as f64 * 0.3).count();
            text(&WORD[..5 - gone], 0.9 * clamp01(ph / 0.2));
            let kind = deleting_kind(val);
            if kind != BK_NONE {
                for j in 0..gone {
                    let age = ph - (0.5 + j as f64 * 0.3);
                    let x0 = text_width(&WORD[..4 - j]);
                    let x1 = text_width(&WORD[..5 - j]);
                    let b = Burst { kind, x: tx + (x0 + x1) / 2.0 + 1.0, y: cy, t: t_now - age, seed: seed(j), w: x1 - x0, col: delete_color(kind, col), dir: 0.0 };
                    draw_burst(&b, t_now, kk, 1.0);
                }
            }
            mini_caret(tx + text_width(&WORD[..5 - gone]) + 1.0, cy, col, 1.0);
        }
        _ => {
            // moving: the caret jumps between the two ends of the word and glides there
            text(WORD, 0.9);
            let w_end = text_width(WORD) + 1.0;
            let tt = t_now + k as f64 * 0.29;
            let jump = tt.floor() as i64;
            let tau = (tt - jump as f64) as f32;
            let (a, b) = if jump % 2 != 0 { (1.0, w_end) } else { (w_end, 1.0) };
            let at = |t: f32, speed: f32| a + (b - a) * (1.0 - (-speed * t.max(0.0)).exp());
            let cx = tx + at(tau, 32.0);
            let trail = tx + at(tau, 13.0);
            if val == 0 || val == 2 {
                let (x0, x1) = (cx.min(trail) - 1.0, cx.max(trail) + 1.0);
                if x1 - x0 > 2.6 {
                    fill_rr(rect(x0, cy - 7.0, x1, cy + 7.0), 1.0, with_a(col, 0.28));
                }
            }
            if val == 1 {
                for i in 1..=7 {
                    let te = tau - i as f32 * 0.03;
                    if te < 0.0 || (at(te, 32.0) - at(te + 0.03, 32.0)).abs() < 0.5 {
                        continue;
                    }
                    let g = Burst { kind: BK_GHOST, x: tx + at(te, 32.0), y: cy, t: t_now - i as f64 * 0.03, seed: 0, w: 0.0, col, dir: 0.0 };
                    draw_burst(&g, t_now, kk, 1.0);
                }
            }
            if val == 2 {
                for i in 0..16 {
                    let te = tau - i as f32 * 0.025;
                    if te < 0.0 || (b - a).abs() * 32.0 * (-32.0 * te).exp() < 40.0 {
                        continue;
                    }
                    for p in 0..2 {
                        let s = (jump * 97 + i * 2 + p) as u32;
                        let g = Burst { kind: BK_COMET, x: tx + at(te, 32.0), y: cy, t: t_now - i as f64 * 0.025, seed: s, w: 0.0, col, dir: if b > a { -1.0 } else { 1.0 } };
                        draw_burst(&g, t_now, kk, 1.0);
                    }
                }
            }
            mini_caret(cx, cy, col, 1.0);
        }
    }
}

pub fn draw_grid(top: f32, dt: f32, alpha: f32, t_now: f64) {
    let s = st();
    let th = s.th;
    let u = cu();
    let cw = u.grid.card_w();
    let (prev_h, name_h) = (u.grid.prev_h, u.grid.name_h);
    draw_layout(&ws("Caret"), &s.fmt_header, MARGIN + GRID_PAD + 2.0, top + GRID_PAD - 4.0, 200.0, GRID_HEADER_H, with_a(th.dim, alpha));
    draw_layout(&ws("← → try it   ↵ apply"), &s.fmt_row_right, MARGIN + BAR_W - GRID_PAD - 260.0, top + GRID_PAD - 4.0, 258.0, GRID_HEADER_H, with_a(th.dim, alpha * 0.85));

    let c_top = top + grid::TOP_H;
    let c_bottom = top + s.panel_h;
    u.grid.animate(dt);
    unsafe { rt().PushAxisAlignedClip(&rect(MARGIN, c_top, MARGIN + BAR_W, c_bottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    if u.items.is_empty() {
        draw_layout(&ws("Nothing matches"), &s.fmt_row, MARGIN + GRID_PAD + 2.0, c_top, 300.0, 40.0, with_a(th.dim, alpha));
    }
    let head_h = u.grid.head_h;
    for (title, hy) in &u.grid.headers {
        let y = c_top + hy - u.grid.off;
        if y > c_bottom || y + head_h < c_top {
            continue;
        }
        draw_layout(&ws(title), &s.fmt_header, MARGIN + GRID_PAD + 2.0, y, 400.0, head_h - 2.0, with_a(th.dim, alpha * 0.9));
    }

    let base = get_transform();
    let row_h = u.grid.row_h();
    let cols = u.grid.cols;
    for k in 0..u.grid.cells.len() {
        let (pos, x, cy0) = u.grid.cells[k];
        let (sec, val) = u.items[pos];
        let y = c_top + cy0 - u.grid.off;
        u.grid.cell_sel[k] = damp(u.grid.cell_sel[k], if k as i32 == s.sel { 1.0 } else { 0.0 }, 16.0, dt);
        if y > c_bottom || y + row_h < c_top {
            continue;
        }
        let sel = u.grid.cell_sel[k];
        let card_a = alpha * clamp01(s.content_a as f64 * 1.3 - (k % cols) as f64 * 0.04);
        let card = rect(x, y, x + cw, y + prev_h);
        let hit = rect(x, y.max(c_top), x + cw, (y + prev_h + name_h).min(c_bottom));
        if hit.bottom > hit.top {
            s.hits.push((hit, k as i32));
        }

        let sk = 1.0 + 0.035 * sel;
        let m = m_mul(&m_scale(sk, sk, x + cw / 2.0, y + prev_h / 2.0), &base);
        set_transform(&m);
        unsafe {
            let lp = D2D1_LAYER_PARAMETERS {
                contentBounds: card,
                geometricMask: ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: m_identity(),
                opacity: card_a,
                opacityBrush: ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            rt().PushLayer(&lp, None::<&ID2D1Layer>);
        }
        fill_acrylic(&rrect(card, 9.0), &th, card.top, card.bottom);
        if let Some(b) = make_gradient(pt(0.0, card.top), pt(0.0, card.bottom), th.border_top, th.border_bottom) {
            unsafe { rt().DrawRoundedRectangle(&rrect(rect(x + 0.5, y + 0.5, x + cw - 0.5, y + prev_h - 0.5), 8.5), &b, 1.0, None::<&ID2D1StrokeStyle>) };
        }
        unsafe { rt().PushAxisAlignedClip(&card, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        draw_demo(sec, val, x, y, cw, prev_h, t_now, k);
        unsafe {
            rt().PopAxisAlignedClip();
            rt().PopLayer();
        }

        if sel > 0.01 {
            set_color(with_a(th.accent, card_a * sel));
            unsafe { rt().DrawRoundedRectangle(&rrect(rect(x - 3.0, y - 3.0, x + cw + 3.0, y + prev_h + 3.0), 12.0), solid(), 2.0, None::<&ID2D1StrokeStyle>) };
        }
        let using = u.set[sec] == val;
        if using {
            // the one in use
            let bc = pt(x + cw - 13.0, y + 13.0);
            set_color(with_a(th.accent, card_a));
            unsafe { rt().FillEllipse(&D2D1_ELLIPSE { point: bc, radiusX: 8.0, radiusY: 8.0 }, solid()) };
            draw_layout(&ws("\u{E73E}"), &s.fmt_glyph_sm, bc.x - 8.0, bc.y - 8.0, 16.0, 16.0, with_a(c(0xFFFFFF, 1.0), card_a));
        }
        set_transform(&base);

        let nc = lerp_c(th.dim, th.text, sel.max(if using { 0.7 } else { 0.0 }));
        draw_layout(&ws(opt_name(sec, val)), &s.fmt_card, x, y + prev_h + 2.0, cw, name_h - 4.0, with_a(nc, card_a));
    }
    unsafe { rt().PopAxisAlignedClip() };

    let view = c_bottom - c_top;
    let max_off = u.grid.max_off();
    if max_off > 0.5 {
        grid::draw_scroll_bar(c_top + 4.0, view - 8.0, u.grid.off / max_off, view / (u.grid.content_h + 6.0), alpha, dt);
    }
}

// ---- glue for main.rs ------------------------------------------------------------------
pub fn on_key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    let Some(k) = cu().grid.key(vk, shift, st().sel) else { return false };
    select(k);
    cu().grid.ensure_visible(st().sel, false);
    true
}
pub fn click(x: f32, y: f32) {
    let k = hit_item_at(x, y);
    if k >= 0 && k < cu().items.len() as i32 {
        st().sel = k;
        apply(k);
    }
}
pub fn wheel(steps: i32) {
    let g = &mut cu().grid;
    g.scroll_to(g.off_target + steps as f32 * 64.0, false);
}
