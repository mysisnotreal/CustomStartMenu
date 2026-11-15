// Panels: the "> commands" added in 1.5 (live widgets, tools, settings). Each one is a
// PanelDef. A panel is either a list of rows (PState::rows, drawn by draw_rows) or draws
// itself. Same as cpp/panels.inl.

use super::*;

#[derive(Clone, Default)]
pub struct PRow {
    pub glyph: String,
    pub title: String,
    pub detail: String,
    pub icon: String,   // a file / app whose icon replaces the glyph
    pub action: String, // right-hand text on the selected row ("Copy  ↵")
    pub tag: i32,
    pub on: bool,                      // detail in the accent color
    pub swatch: Option<D2D1_COLOR_F>, // a color dot instead of the glyph
}

pub struct PanelDef {
    pub key: &'static str,
    pub title: &'static str,
    pub glyph: &'static str,
    pub aliases: Vec<&'static str>,  // other names that open it ("> music" for "> media")
    pub arg_only: Vec<&'static str>, // names that only count with something after them ("> f cat")
    pub listed: bool,                // shown in the "> " list
    pub takes_arg: bool,             // picking it from the list types "> key " (with a space)
    pub tag: i32,                    // for panels that share callbacks (the power ones)
    pub fill: Option<fn(bool)>,      // (re)builds the rows from the argument
    pub detail: Option<fn() -> String>,
    pub enter: Option<fn(i32, bool)>,
    pub del: Option<fn(i32) -> bool>, // Delete with the caret at the end of the text
    pub height: Option<fn() -> f32>,  // set for panels that draw themselves
    pub draw: Option<fn(f32, f32, f32, f64)>,
    pub keys: Option<fn(VIRTUAL_KEY, bool) -> bool>,
    pub click: Option<fn(f32, f32) -> bool>,
    pub wheel: Option<fn(i32)>,
    pub scroll_frac: Option<fn(f32)>, // the scroll bar was dragged (self-drawn panels)
    pub count: Option<fn() -> i32>,
    pub leave: Option<fn()>,
    pub empty: &'static str,
    pub refresh: f64, // seconds between fill(false) calls while it's open (live lists)
}

impl PanelDef {
    pub fn new(key: &'static str, title: &'static str, glyph: &'static str) -> Self {
        PanelDef {
            key,
            title,
            glyph,
            aliases: Vec::new(),
            arg_only: Vec::new(),
            listed: true,
            takes_arg: false,
            tag: 0,
            fill: None,
            detail: None,
            enter: None,
            del: None,
            height: None,
            draw: None,
            keys: None,
            click: None,
            wheel: None,
            scroll_frac: None,
            count: None,
            leave: None,
            empty: "Nothing here",
            refresh: 0.0,
        }
    }
}

pub struct UserCmd {
    pub name: String,
    pub lower: String,
    pub path: String,
}

#[derive(Default)]
pub struct PState {
    pub rows: Vec<PRow>,
    pub panel: i32,
    pub arg: String, // what's typed after the panel's name
    pub user_cmd: i32,
    pub run_panel: i32,
    pub flash: String, // a short message where the hint goes ("Copied")
    pub flash_time: f64,
    pub lay_anim: i32, // "> layout": 0 rise, 1 pop, 2 fade, 3 drop
    pub inline_copy: String,
    pub inline_url: String,
    pub fill_at: f64,
    pub confirm_key: String,
    pub confirm_at: f64,
    pub user_cmds: Vec<UserCmd>,
}

static mut PS: Option<PState> = None;
pub fn ps() -> &'static mut PState {
    unsafe {
        (*std::ptr::addr_of_mut!(PS)).get_or_insert_with(|| PState {
            panel: -1,
            user_cmd: -1,
            run_panel: -1,
            flash_time: -10.0,
            confirm_at: -10.0,
            ..Default::default()
        })
    }
}
static mut PANELS: Vec<PanelDef> = Vec::new();
pub fn panels() -> &'static mut Vec<PanelDef> {
    unsafe { &mut *std::ptr::addr_of_mut!(PANELS) }
}
pub fn add_panel(p: PanelDef) -> usize {
    panels().push(p);
    panels().len() - 1
}
pub fn cur() -> Option<&'static PanelDef> {
    let i = ps().panel;
    if i >= 0 {
        panels().get(i as usize)
    } else {
        None
    }
}
pub fn rows() -> &'static mut Vec<PRow> {
    &mut ps().rows
}

// ---- flash messages and "press Enter again" ------------------------------------------
pub fn flash(s: &str) {
    let p = ps();
    p.flash = s.to_string();
    p.flash_time = now();
}
// The first Enter arms, a second one within 3 seconds goes ahead.
pub fn confirmed(what: &str) -> bool {
    let t = now();
    let p = ps();
    if p.confirm_key == what && t - p.confirm_at < 3.0 {
        p.confirm_key.clear();
        return true;
    }
    p.confirm_key = what.to_string();
    p.confirm_at = t;
    false
}
pub fn armed(what: &str) -> bool {
    let p = ps();
    p.confirm_key == what && now() - p.confirm_at < 3.0
}

pub fn leave() {
    if let Some(f) = cur().and_then(|p| p.leave) {
        f();
    }
    ps().confirm_key.clear();
}
pub fn cur_empty() -> &'static str {
    cur().map(|p| p.empty).unwrap_or("")
}

pub fn fill(first: bool) {
    let Some(p) = cur() else { return };
    ps().fill_at = now();
    if first {
        rows().clear();
    }
    if let Some(f) = p.fill {
        f(first);
    }
    let n = item_count();
    let s = st();
    s.sel = if n > 0 { s.sel.clamp(0, n - 1) } else { 0 };
}

// Live panels refresh themselves while they're open.
pub fn tick(t_now: f64) {
    let Some(p) = cur() else { return };
    if st().mode != Mode::Panel || p.refresh <= 0.0 {
        return;
    }
    if t_now - ps().fill_at >= p.refresh {
        fill(false);
    }
}

pub fn has_rows() -> bool {
    cur().map(|p| p.draw.is_none()).unwrap_or(false)
}
pub fn item_count() -> i32 {
    let Some(p) = cur() else { return 0 };
    if let Some(c) = p.count {
        return c();
    }
    if p.draw.is_some() {
        0
    } else {
        rows().len() as i32
    }
}
pub fn height() -> f32 {
    let Some(p) = cur() else { return 0.0 };
    if let Some(h) = p.height {
        return MAX_PANEL_H.min(h());
    }
    PANEL_PAD * 2.0 + ROW_H * rows().len().clamp(1, MAX_ROWS) as f32
}

// Which panel the text after ">" opens: the longest name (or alias) that is the whole text
// or is followed by a space. Returns it and the rest, as typed.
pub fn panel_match(ql: &str, q: &str) -> Option<(usize, String)> {
    let mut best: Option<usize> = None;
    let mut best_len = 0usize;
    for (i, p) in panels().iter().enumerate() {
        let mut test = |k: &str| {
            if k.len() <= best_len {
                return;
            }
            if ql == k || (ql.len() > k.len() && ql.starts_with(k) && ql.as_bytes()[k.len()] == b' ') {
                best = Some(i);
                best_len = k.len();
            }
        };
        test(p.key);
        for a in &p.aliases {
            test(a);
        }
        for a in &p.arg_only {
            if ql.len() > a.len() + 1 {
                test(a);
            }
        }
    }
    best.map(|i| (i, q.get(best_len.min(q.len())..).unwrap_or("").trim().to_string()))
}

pub fn activate(shift: bool) {
    if let Some(f) = cur().and_then(|p| p.enter) {
        f(st().sel, shift);
    }
}

pub fn key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    let Some(p) = cur() else { return false };
    if let Some(k) = p.keys {
        if k(vk, shift) {
            return true;
        }
    }
    let s = st();
    if vk == VK_DELETE && s.caret == s.text.len() && s.anchor == s.caret {
        if let Some(d) = p.del {
            return d(s.sel);
        }
    }
    false
}

// ---- the "> " list: built-in commands, then panels, then the user's own commands -------
pub const CMDID_PANEL: usize = 1000;
pub const CMDID_USER: usize = 5000;

pub fn cmd_key(id: usize) -> String {
    if id >= CMDID_USER {
        return ps().user_cmds[id - CMDID_USER].lower.clone();
    }
    if id >= CMDID_PANEL {
        return panels()[id - CMDID_PANEL].key.to_string();
    }
    CMDS[id].key.to_string()
}
pub fn cmd_title(id: usize) -> String {
    if id >= CMDID_USER {
        return ps().user_cmds[id - CMDID_USER].name.clone();
    }
    if id >= CMDID_PANEL {
        return panels()[id - CMDID_PANEL].title.to_string();
    }
    CMDS[id].title.to_string()
}
pub fn cmd_glyph(id: usize) -> &'static str {
    if id >= CMDID_USER {
        return "\u{E756}"; // command prompt
    }
    if id >= CMDID_PANEL {
        return panels()[id - CMDID_PANEL].glyph;
    }
    CMDS[id].glyph
}
pub fn cmd_detail(id: usize) -> String {
    if id >= CMDID_USER {
        let path = &ps().user_cmds[id - CMDID_USER].path;
        return match path.rfind('.') {
            Some(d) => format!("Your command · {}", path[d + 1..].to_lowercase()),
            None => "Your command".into(),
        };
    }
    if id >= CMDID_PANEL {
        return panels()[id - CMDID_PANEL].detail.map(|f| f()).unwrap_or_default();
    }
    command_detail(id)
}
pub fn cmd_run(id: usize) {
    let k = cmd_key(id);
    let arg = id >= CMDID_USER || (id >= CMDID_PANEL && panels()[id - CMDID_PANEL].takes_arg);
    set_text(&ws(&format!("> {}{}", k, if arg { " " } else { "" })));
}

pub fn cmd_list_fill(q: &str) {
    tools::scan_user_commands();
    let m = |key: &str, title: &str, al: &[&str]| {
        q.is_empty() || key.starts_with(q) || title.to_lowercase().contains(q) || al.iter().any(|a| a.starts_with(q))
    };
    let s = st();
    for i in 0..=CMD_FONTFOLDER {
        if m(CMDS[i].key, CMDS[i].title, &[]) {
            s.cmd_list.push(i);
        }
    }
    for (i, p) in panels().iter().enumerate() {
        if p.listed && m(p.key, p.title, &p.aliases) {
            s.cmd_list.push(CMDID_PANEL + i);
        }
    }
    for i in CMD_FONTFOLDER + 1..CMDS.len() {
        if m(CMDS[i].key, CMDS[i].title, &[]) {
            s.cmd_list.push(i);
        }
    }
    for (i, u) in ps().user_cmds.iter().enumerate() {
        if m(&u.lower, &u.name, &[]) {
            s.cmd_list.push(CMDID_USER + i);
        }
    }
}

// ---- small shared helpers ---------------------------------------------------------------
// The install folder (Programs\CustomStartMenu), where Notes.txt, Commands\ and the caches live.
pub fn app_dir() -> String {
    let mut d = games::known_folder_path(&windows::Win32::UI::Shell::FOLDERID_UserProgramFiles);
    if d.is_empty() {
        d = games::expand_env("%LOCALAPPDATA%\\Programs");
    }
    d.push_str("\\CustomStartMenu");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn set_clipboard_text(s: &str) -> bool {
    let mut v = ws(s);
    v.push(0);
    unsafe {
        if OpenClipboard(hwnd()).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, v.len() * 2) {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
                let _ = GlobalUnlock(h);
                ok = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(h.0)).is_ok();
                if !ok {
                    let _ = GlobalFree(h);
                }
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

// "3 min ago", "2 h ago", "yesterday"...
pub fn ago(seconds: f64) -> String {
    if seconds < 60.0 {
        return "just now".into();
    }
    if seconds < 3600.0 {
        return format!("{} min ago", (seconds / 60.0) as i32);
    }
    if seconds < 86400.0 {
        return format!("{} h ago", (seconds / 3600.0) as i32);
    }
    let d = (seconds / 86400.0) as i32;
    if d == 1 {
        "yesterday".into()
    } else {
        format!("{} days ago", d)
    }
}

pub fn one_line(s: &str, max: usize) -> String {
    let mut o = String::new();
    let mut n = 0;
    let mut sp = false;
    for c in s.chars() {
        if c == '\r' || c == '\n' || c == '\t' || c == ' ' {
            sp = !o.is_empty();
            continue;
        }
        if sp {
            o.push(' ');
            n += 1;
        }
        sp = false;
        o.push(c);
        n += c.len_utf16();
        if n >= max {
            break;
        }
    }
    o
}

pub fn format_bytes(mut b: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut i = 0;
    while b >= 1024.0 && i < 4 {
        b /= 1024.0;
        i += 1;
    }
    if i == 0 || b >= 100.0 {
        format!("{:.0} {}", b, U[i])
    } else {
        format!("{:.1} {}", b, U[i])
    }
}

// Settings panels ("> sounds", "> layout"...): one row per setting; Left / Right / Enter
// change the selected one.
pub struct SettingDef {
    pub name: &'static str,
    pub glyph: &'static str,
    pub options: Vec<String>,
    pub get: Box<dyn Fn() -> i32>,
    pub set: Box<dyn Fn(i32)>,
}
pub fn settings_fill(defs: &[SettingDef]) {
    let r = rows();
    r.clear();
    for d in defs {
        let v = (d.get)().clamp(0, d.options.len() as i32 - 1) as usize;
        r.push(PRow {
            glyph: d.glyph.into(),
            title: d.name.into(),
            detail: d.options[v].clone(),
            action: format!("‹  {}  ›", d.options[v]),
            on: true,
            ..Default::default()
        });
    }
}
pub fn settings_key(defs: &[SettingDef], vk: VIRTUAL_KEY, shift: bool) -> bool {
    if shift || (vk != VK_LEFT && vk != VK_RIGHT) {
        return false;
    }
    let sel = st().sel;
    if sel < 0 || sel as usize >= defs.len() {
        return false;
    }
    let d = &defs[sel as usize];
    let n = d.options.len() as i32;
    (d.set)(((d.get)() + if vk == VK_RIGHT { 1 } else { -1 }).rem_euclid(n));
    fill(false);
    true
}
pub fn settings_enter(defs: &[SettingDef], sel: i32) {
    if sel < 0 || sel as usize >= defs.len() {
        return;
    }
    let d = &defs[sel as usize];
    (d.set)(((d.get)() + 1) % d.options.len() as i32);
    fill(false);
}

// ---- drawing ----------------------------------------------------------------------------
pub fn dl(s: &str, fmt: &IDWriteTextFormat, x: f32, y: f32, w: f32, h: f32, col: D2D1_COLOR_F) {
    if !s.is_empty() {
        draw_layout(&ws(s), fmt, x, y, w, h, col);
    }
}

pub fn text_width(s: &str, f: &IDWriteTextFormat) -> f32 {
    let Some(l) = make_layout(&ws(s), f, 4000.0, 100.0) else { return 0.0 };
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe {
        let _ = l.GetMetrics(&mut m);
    }
    m.widthIncludingTrailingWhitespace
}

pub fn fill_circle(x: f32, y: f32, r: f32, col: D2D1_COLOR_F) {
    set_color(col);
    unsafe { rt().FillEllipse(&D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r }, solid()) };
}

// A row in a panel list.
pub fn draw_panel_row(r: &PRow, y: f32, is_sel: bool, row_a: f32, t_now: f64) {
    let s = st();
    let th = s.th;
    let (ix, iy) = (MARGIN + 20.0, y + (ROW_H - 24.0) / 2.0);
    if !r.icon.is_empty() {
        let icon = get_icon(&ws(&r.icon), 0);
        let io = match &icon {
            Some((_, shown)) => ease_out_cubic(clamp01((t_now - shown) / 0.18) as f64) as f32,
            None => 0.0,
        };
        if io < 1.0 {
            let g = if r.glyph.is_empty() { "\u{E7C3}" } else { r.glyph.as_str() };
            dl(g, &s.fmt_glyph, ix, y, 24.0, ROW_H, with_a(th.dim, row_a * (1.0 - io)));
        }
        if let Some((bmp, _)) = &icon {
            unsafe { rt().DrawBitmap(bmp, Some(&rect(ix, iy, ix + 24.0, iy + 24.0)), row_a * io, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
        }
    } else if let Some(sw) = r.swatch {
        fill_circle(ix + 12.0, y + ROW_H / 2.0, 8.0, with_a(sw, row_a));
    } else {
        dl(&r.glyph, &s.fmt_glyph, ix, y, 24.0, ROW_H, with_a(if is_sel { th.accent } else { th.dim }, row_a));
    }
    let right = if is_sel && !r.action.is_empty() { &r.action } else { &r.detail };
    let mut rw = 0.0f32;
    let max_w = 300.0f32.min((bar_w() - 76.0) * 0.48); // the title keeps at least half
    if !right.is_empty() {
        if let Some(l) = make_layout(&ws(right), &s.fmt_row_right, max_w, ROW_H) {
            unsafe {
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                if let Ok(sign) = s.dw.CreateEllipsisTrimmingSign(&l) {
                    let _ = l.SetTrimming(&trim, &sign);
                }
                let mut tm = DWRITE_TEXT_METRICS::default();
                let _ = l.GetMetrics(&mut tm);
                rw = max_w.min(tm.width);
                let accent = r.on || (is_sel && !r.action.is_empty());
                set_color(with_a(if accent { th.accent } else { th.dim }, row_a * if is_sel { 1.0 } else { 0.85 }));
                rt().DrawTextLayout(pt(MARGIN + bar_w() - 20.0 - max_w, y), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
    }
    dl(
        &r.title,
        &s.fmt_row,
        MARGIN + 56.0,
        y,
        bar_w() - 56.0 - 36.0 - rw,
        ROW_H,
        with_a(if is_sel { th.text } else { lerp_c(th.dim, th.text, 0.55) }, row_a),
    );
}

// Shared look for the self-drawn panels: a section title on the left, key hints on the right.
pub fn panel_header(title: &str, hints: &str, top: f32, alpha: f32) {
    let s = st();
    dl(title, &s.fmt_header, MARGIN + GRID_PAD + 2.0, top + GRID_PAD - 4.0, 300.0, GRID_HEADER_H, with_a(s.th.dim, alpha));
    if !hints.is_empty() {
        dl(
            hints,
            &s.fmt_row_right,
            MARGIN + bar_w() - GRID_PAD - 330.0,
            top + GRID_PAD - 4.0,
            328.0,
            GRID_HEADER_H,
            with_a(s.th.dim, alpha * 0.85),
        );
    }
}

// A filled round-cornered card in the panel's material (like the theme cards).
pub fn draw_card(r: D2D_RECT_F, alpha: f32, radius: f32) {
    let th = st().th;
    fill_rr(r, radius, with_a(th.highlight, alpha * 1.1));
    set_color(with_a(th.divider, alpha));
    unsafe {
        rt().DrawRoundedRectangle(&rrect(rect(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5), radius), solid(), 1.0, None::<&ID2D1StrokeStyle>)
    };
}

// A line graph of values 0..1 (oldest first), filled underneath.
pub fn draw_graph(v: &[f32], r: D2D_RECT_F, col: D2D1_COLOR_F, alpha: f32, max_v: f32) {
    if v.len() < 2 || max_v <= 0.0 {
        return;
    }
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let p = |i: usize| pt(r.left + w * i as f32 / (v.len() - 1) as f32, r.bottom - h * (v[i] / max_v).clamp(0.0, 1.0));
    unsafe {
        let d2d = &st().d2d;
        let (Ok(line), Ok(fillg)) = (d2d.CreatePathGeometry(), d2d.CreatePathGeometry()) else { return };
        if let Ok(sink) = line.Open() {
            sink.BeginFigure(p(0), D2D1_FIGURE_BEGIN_HOLLOW);
            for i in 1..v.len() {
                sink.AddLine(p(i));
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
        }
        if let Ok(sink) = fillg.Open() {
            sink.BeginFigure(pt(r.left, r.bottom), D2D1_FIGURE_BEGIN_FILLED);
            for i in 0..v.len() {
                sink.AddLine(p(i));
            }
            sink.AddLine(pt(r.right, r.bottom));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
        }
        if let Some(b) = make_gradient(pt(0.0, r.top), pt(0.0, r.bottom), with_a(col, alpha * 0.30), with_a(col, 0.0)) {
            rt().FillGeometry(&fillg, &b, None::<&ID2D1Brush>);
        }
        set_color(with_a(col, alpha));
        rt().DrawGeometry(&line, solid(), 1.5, None::<&ID2D1StrokeStyle>);
    }
}
