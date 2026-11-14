// Fonts ("> font"): a catalog of free fonts shown as preview cards, grouped by category.
// Nothing is downloaded until the card's Download button is clicked (or Enter is pressed on
// it); the pictures on the cards are small images from this project's repository.
// Same behavior as cpp/fonts.inl.

use super::*;
use std::collections::HashSet;
use std::sync::LazyLock;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Networking::WinHttp::*;

const PREVIEW_BASE: &str = "https://raw.githubusercontent.com/mysisnotreal/CustomStartMenu/main/fonts/previews/";
const SAMPLE: &str = "The quick brown fox jumps over the lazy dog";
const LOCAL_CAT: &str = "On this PC";
const CATALOG: &str = include_str!("../../fonts/catalog.tsv");

const COLS: usize = 3;
const PREV_H: f32 = 64.0;
const NAME_H: f32 = 26.0;
const ROW_GAP: f32 = 8.0;
const HEAD_H: f32 = 28.0;
const ROW_H_F: f32 = PREV_H + NAME_H + ROW_GAP;
const CARD_W: f32 = (BAR_W - 2.0 * GRID_PAD - (COLS as f32 - 1.0) * CARD_GAP) / COLS as f32;
const TOP_H: f32 = GRID_PAD + GRID_HEADER_H - 4.0; // the fixed header above the cards

#[derive(Clone, Default)]
pub struct FontItem {
    pub cat: String,
    pub name: String,
    pub lower: String,
    pub id: String,
    pub url: String,
    pub file: String, // its name in the Fonts folder
}

// UI-thread state (State::fonts)
#[derive(Default)]
pub struct FontUi {
    pub cat: Vec<FontItem>, // the catalog, then fonts that are only on this PC
    pub cat_n: usize,
    pub filter: String,
    pub list: Vec<usize>, // what the panel shows (indices into cat)
    pub local: HashSet<String>, // lower-case file names in the Fonts folder
    pub cells: Vec<(usize, f32, f32)>, // item, x in bar space, y from the top of the scrolling area
    pub headers: Vec<(String, f32)>,
    pub cell_sel: Vec<f32>,
    pub content_h: f32,
    pub off: f32,
    pub off_target: f32,
    pub btn_hits: Vec<(D2D_RECT_F, i32)>,
    pub default_hit: Option<D2D_RECT_F>,
    pub btn_hot: i32,
    pub default_hot: bool,
    pub bmps: HashMap<String, (ID2D1Bitmap, f64, bool)>, // id -> bitmap, first shown, is a mask
    pub local_fmt: HashMap<String, Option<IDWriteTextFormat>>,
}

// Shared with the download and picture threads.
struct Preview {
    ready: bool,
    failed: bool,
    px: Vec<u32>,
    w: u32,
    h: u32,
    mask: bool, // gray coverage (drawn in the theme's text color) rather than a color picture
}
#[derive(Default)]
struct Shared {
    progress: HashMap<String, f32>, // id -> 0..1 while downloading
    failed: HashSet<String>,
    previews: HashMap<String, Preview>,
    queue: Vec<String>,
    quit: bool,
}
static SHARED: LazyLock<Mutex<Shared>> = LazyLock::new(|| Mutex::new(Shared::default()));
static PREVIEW_CV: Condvar = Condvar::new();
static WORKERS: Mutex<Vec<std::thread::JoinHandle<()>>> = Mutex::new(Vec::new());

// The font in use: a file in the Fonts folder ("" = the Windows font).
pub static FONT_FILE: Mutex<String> = Mutex::new(String::new());
pub fn font_file() -> String {
    FONT_FILE.lock().unwrap().clone()
}

fn fu() -> &'static mut FontUi {
    &mut st().fonts
}

pub fn fonts_dir() -> String {
    format!("{}\\CustomStartMenu\\Fonts", games::known_folder_path(&FOLDERID_UserProgramFiles))
}
fn preview_dir() -> String {
    format!("{}\\CustomStartMenu\\Cache\\FontPreviews", games::known_folder_path(&FOLDERID_UserProgramFiles))
}

// ---- names ---------------------------------------------------------------------
// "SF-Pro-Display-BlackItalic.otf" -> "SF Pro Display Black Italic": drop the extension, split
// on - and _, split joined words (camelCase, acronyms, a trailing number) and spell out style
// abbreviations. The catalog's names were made the same way.
pub fn clean_font_name(file: &str) -> String {
    let mut s = file.to_string();
    let l = s.to_lowercase();
    if l.ends_with(".otf") || l.ends_with(".ttf") {
        s.truncate(s.len() - 4);
    }
    let c: Vec<char> = s.chars().collect();
    let mut c = c;
    if c.len() > 2 && c[c.len() - 2] == 'I' && c[c.len() - 1] == 't' && (c[c.len() - 3].is_lowercase() || c[c.len() - 3] == '-') {
        c.extend("alic".chars());
    }
    let mut r = String::new();
    for i in 0..c.len() {
        let ch = c[i];
        if ch == '-' || ch == '_' || ch == ' ' {
            if !r.is_empty() && !r.ends_with(' ') {
                r.push(' ');
            }
            continue;
        }
        if i > 0 && !r.is_empty() && !r.ends_with(' ') {
            let p = c[i - 1];
            let digits_to_end = ch.is_ascii_digit() && {
                let mut j = i;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                j == c.len() || matches!(c[j], '-' | '_' | ' ')
            };
            if (p.is_lowercase() && ch.is_uppercase())
                || (p.is_uppercase() && ch.is_uppercase() && i + 1 < c.len() && c[i + 1].is_lowercase())
                || (p.is_lowercase() && digits_to_end)
            {
                r.push(' ');
            }
        }
        r.push(ch);
    }
    let mut r = r.trim().to_string();
    for (a, b) in [
        ("Deja Vu", "DejaVu"),
        ("Math Jax", "MathJax"),
        ("Semibold", "Semi Bold"),
        ("Extralight", "Extra Light"),
        ("Ultralight", "Ultra Light"),
        ("Regular Italic", "Italic"),
    ] {
        r = r.replace(a, b);
    }
    r
}

// ---- catalog ---------------------------------------------------------------------
pub fn load_catalog() {
    let f = fu();
    for line in CATALOG.lines() {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() < 5 {
            continue;
        }
        f.cat.push(FontItem {
            cat: p[0].to_string(),
            name: p[1].to_string(),
            lower: format!("{} {}", p[1], p[0]).to_lowercase(),
            id: p[2].to_string(),
            url: p[3].to_string(),
            file: p[3].rsplit('/').next().unwrap_or("").to_string(),
        });
    }
    f.cat_n = f.cat.len();
}

fn is_font_file(n: &str) -> bool {
    let l = n.to_lowercase();
    l.ends_with(".otf") || l.ends_with(".ttf")
}

// Looks at the Fonts folder: which catalog fonts are downloaded, and which fonts were put
// there by hand (they get their own "On this PC" category).
fn scan_local() {
    let f = fu();
    f.local.clear();
    f.cat.truncate(f.cat_n);
    let mut files: Vec<String> = std::fs::read_dir(fonts_dir())
        .map(|d| d.flatten().filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false)).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    files.retain(|n| is_font_file(n));
    files.sort_by_key(|a| a.to_lowercase());
    let known: HashSet<String> = f.cat.iter().map(|i| i.file.to_lowercase()).collect();
    for file in files {
        let l = file.to_lowercase();
        f.local.insert(l.clone());
        if known.contains(&l) {
            continue;
        }
        let name = clean_font_name(&file);
        f.cat.push(FontItem {
            cat: LOCAL_CAT.to_string(),
            lower: format!("{} {}", name, LOCAL_CAT).to_lowercase(),
            name,
            id: format!("local:{}", l),
            url: String::new(),
            file,
        });
    }
}

fn is_local(it: &FontItem) -> bool {
    fu().local.contains(&it.file.to_lowercase())
}
fn in_use(it: &FontItem) -> bool {
    let cur = font_file();
    !cur.is_empty() && it.file.eq_ignore_ascii_case(&cur)
}

pub fn display_name(file: &str) -> String {
    if file.is_empty() {
        return "Default".to_string();
    }
    fu().cat.iter().find(|i| i.file.eq_ignore_ascii_case(file)).map(|i| i.name.clone()).unwrap_or_else(|| clean_font_name(file))
}

// Cards in rows of three under a title for each category.
fn layout_cells() {
    let f = fu();
    f.cells.clear();
    f.headers.clear();
    let mut y = 0.0;
    let mut col = 0usize;
    let mut cat: Option<String> = None;
    for &i in &f.list {
        let it = &f.cat[i];
        if cat.as_deref() != Some(it.cat.as_str()) {
            if col > 0 {
                y += ROW_H_F;
                col = 0;
            }
            f.headers.push((it.cat.clone(), y));
            y += HEAD_H;
            cat = Some(it.cat.clone());
        }
        f.cells.push((i, MARGIN + GRID_PAD + col as f32 * (CARD_W + CARD_GAP), y));
        col += 1;
        if col == COLS {
            col = 0;
            y += ROW_H_F;
        }
    }
    if col > 0 {
        y += ROW_H_F;
    }
    f.content_h = if f.list.is_empty() { 44.0 } else { y };
    f.cell_sel = vec![0.0; f.cells.len()];
}

// "> font NAME": every word has to appear in the font's name or category.
pub fn build_list(filter: &str) {
    fu().filter = filter.to_string();
    scan_local();
    let f = fu();
    let words: Vec<&str> = filter.split(' ').filter(|w| !w.is_empty()).collect();
    f.list = (0..f.cat.len()).filter(|&i| words.iter().all(|w| f.cat[i].lower.contains(w))).collect();
    layout_cells();
}

pub fn count() -> i32 {
    fu().list.len() as i32
}
pub fn panel_h() -> f32 {
    MAX_PANEL_H.min(TOP_H + fu().content_h + 6.0)
}
fn view_h() -> f32 {
    panel_h() - TOP_H
}
fn max_off() -> f32 {
    (fu().content_h + 6.0 - view_h()).max(0.0)
}
pub fn scroll_to(off: f32, instant: bool) {
    let f = fu();
    f.off_target = off.clamp(0.0, max_off());
    if instant {
        f.off = f.off_target;
    }
}

// Keeps the selected card (and its category title when it's the first row) in view.
fn ensure_visible(k: i32, instant: bool) {
    let f = fu();
    if k < 0 || k as usize >= f.cells.len() {
        return;
    }
    let y = f.cells[k as usize].2;
    let view = view_h();
    let (top, bottom) = (y - HEAD_H - 4.0, y + ROW_H_F);
    if top < f.off_target {
        scroll_to(top, instant);
    } else if bottom > f.off_target + view {
        scroll_to(bottom - view, instant);
    }
}

// Up / Down: the card in the next row nearest to this one's column.
fn vertical(k: i32, dir: i32) -> i32 {
    let f = fu();
    if k < 0 || k as usize >= f.cells.len() {
        return k;
    }
    let (_, x, y) = f.cells[k as usize];
    let mut best = k;
    let mut best_row = if dir > 0 { 1e9f32 } else { -1e9f32 };
    let mut best_dx = 1e9f32;
    for (i, &(_, cx, cy)) in f.cells.iter().enumerate() {
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

// ---- downloads -------------------------------------------------------------------
// GET over HTTPS. With a path, the body goes to that file (via a temporary file, so a
// broken download never leaves half a font behind); otherwise into `out`.
fn http_get(url: &str, mut out: Option<&mut Vec<u8>>, path: Option<&str>, progress: &mut dyn FnMut(f32)) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let (host, p) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let mut ok = false;
    unsafe {
        let s = WinHttpOpen(w!("CustomStart/1.3"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if s.is_null() {
            return false;
        }
        let c = WinHttpConnect(s, &HSTRING::from(host), 443, 0);
        let r = if c.is_null() {
            null_mut()
        } else {
            WinHttpOpenRequest(c, w!("GET"), &HSTRING::from(p), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE)
        };
        if !r.is_null() && WinHttpSendRequest(r, None, None, 0, 0, 0).is_ok() && WinHttpReceiveResponse(r, null_mut()).is_ok() {
            let mut status = 0u32;
            let mut sz = 4u32;
            let _ = WinHttpQueryHeaders(r, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut c_void), &mut sz, null_mut());
            let mut total = 0u64;
            let mut lenbuf = [0u16; 32];
            let mut lsz = 64u32;
            if WinHttpQueryHeaders(r, WINHTTP_QUERY_CONTENT_LENGTH, PCWSTR::null(), Some(lenbuf.as_mut_ptr() as *mut c_void), &mut lsz, null_mut()).is_ok() {
                total = String::from_utf16_lossy(&lenbuf[..(lsz / 2) as usize]).trim().parse().unwrap_or(0);
            }
            if status == 200 {
                let tmp = path.map(|p| format!("{}.part", p));
                let mut file = match &tmp {
                    Some(t) => std::fs::File::create(t).ok(),
                    None => None,
                };
                if path.is_none() || file.is_some() {
                    ok = true;
                    let mut got = 0u64;
                    let mut buf = vec![0u8; 64 * 1024];
                    loop {
                        let mut n = 0u32;
                        if WinHttpReadData(r, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut n).is_err() {
                            ok = false;
                            break;
                        }
                        if n == 0 {
                            break;
                        }
                        got += n as u64;
                        if let Some(o) = out.as_deref_mut() {
                            o.extend_from_slice(&buf[..n as usize]);
                        }
                        if let Some(fh) = file.as_mut() {
                            use std::io::Write;
                            if fh.write_all(&buf[..n as usize]).is_err() {
                                ok = false;
                                break;
                            }
                        }
                        if total > 0 {
                            progress((got as f64 / total as f64).min(1.0) as f32);
                        }
                    }
                    if total > 0 && got < total {
                        ok = false;
                    }
                    if let (Some(fh), Some(t), Some(p)) = (file.take(), &tmp, path) {
                        drop(fh);
                        if ok {
                            ok = std::fs::rename(t, p).is_ok();
                        }
                        if !ok {
                            let _ = std::fs::remove_file(t);
                        }
                    }
                }
            }
        }
        if !r.is_null() {
            let _ = WinHttpCloseHandle(r);
        }
        if !c.is_null() {
            let _ = WinHttpCloseHandle(c);
        }
        let _ = WinHttpCloseHandle(s);
    }
    ok
}

fn start_download(item: usize) {
    let Some(it) = fu().cat.get(item).cloned() else { return };
    if it.url.is_empty() {
        return;
    }
    {
        let mut sh = SHARED.lock().unwrap();
        if sh.progress.contains_key(&it.id) {
            return;
        }
        sh.progress.insert(it.id.clone(), 0.0);
        sh.failed.remove(&it.id);
    }
    let h = hwnd().0 as isize;
    std::thread::spawn(move || {
        let dir = fonts_dir();
        let _ = std::fs::create_dir_all(&dir);
        let id = it.id.clone();
        let ok = http_get(&it.url, None, Some(&format!("{}\\{}", dir, it.file)), &mut |p| {
            SHARED.lock().unwrap().progress.insert(id.clone(), p);
        });
        let boxed = Box::into_raw(Box::new(it.id));
        unsafe {
            if PostMessageW(HWND(h as *mut c_void), WM_APP_FONT, WPARAM(ok as usize), LPARAM(boxed as isize)).is_err() {
                drop(Box::from_raw(boxed));
            }
        }
    });
}

// ---- preview pictures --------------------------------------------------------------
// PNG -> premultiplied BGRA. A grayscale picture is coverage: it becomes white with that
// alpha, and is drawn as an opacity mask in the theme's text color.
fn decode(wic: &IWICImagingFactory, png: &[u8]) -> Option<Preview> {
    unsafe {
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(png).ok()?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let fr = dec.GetFrame(0).ok()?;
        let mask = fr.GetPixelFormat().ok()? == GUID_WICPixelFormat8bppGray;
        let (mut w, mut h) = (0u32, 0u32);
        fr.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || w > 2048 || h > 1024 {
            return None;
        }
        let cv = wic.CreateFormatConverter().ok()?;
        cv.Initialize(&fr, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let mut px = vec![0u32; (w * h) as usize];
        cv.CopyPixels(std::ptr::null(), w * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr() as *mut u8, px.len() * 4)).ok()?;
        if mask {
            for p in px.iter_mut() {
                let g = *p & 0xFF;
                *p = (g << 24) | (g << 16) | (g << 8) | g;
            }
        }
        Some(Preview { ready: true, failed: false, px, w, h, mask })
    }
}

// Newest requests first, so the cards on screen load before ones scrolled past.
fn preview_worker() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let wic: Option<IWICImagingFactory> = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
    let dir = preview_dir();
    loop {
        let id = {
            let mut g = SHARED.lock().unwrap();
            loop {
                if g.quit {
                    drop(g);
                    unsafe { CoUninitialize() };
                    return;
                }
                if let Some(id) = g.queue.pop() {
                    break id;
                }
                g = PREVIEW_CV.wait(g).unwrap();
            }
        };
        let file = format!("{}\\{}.png", dir, id);
        let mut res = None;
        if let Some(wic) = &wic {
            if let Ok(png) = std::fs::read(&file) {
                res = decode(wic, &png);
            }
            if res.is_none() {
                let mut png = Vec::new();
                if http_get(&format!("{}{}.png", PREVIEW_BASE, id), Some(&mut png), None, &mut |_| {}) {
                    res = decode(wic, &png);
                    if res.is_some() {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::write(&file, &png);
                    }
                }
            }
        }
        let mut g = SHARED.lock().unwrap();
        g.previews.insert(id, res.unwrap_or(Preview { ready: false, failed: true, px: Vec::new(), w: 0, h: 0, mask: true }));
    }
}

pub fn start_workers() {
    let mut w = WORKERS.lock().unwrap();
    for _ in 0..4 {
        w.push(std::thread::spawn(preview_worker));
    }
}
pub fn stop_workers() {
    SHARED.lock().unwrap().quit = true;
    PREVIEW_CV.notify_all();
    for t in WORKERS.lock().unwrap().drain(..) {
        let _ = t.join();
    }
}

// The card picture, once it's loaded (asks for it the first time).
fn get_preview(id: &str) -> Option<(ID2D1Bitmap, f64, bool)> {
    let f = fu();
    if let Some(b) = f.bmps.get(id) {
        return Some(b.clone());
    }
    let mut g = SHARED.lock().unwrap();
    match g.previews.get(id) {
        None => {
            g.previews.insert(id.to_string(), Preview { ready: false, failed: false, px: Vec::new(), w: 0, h: 0, mask: true });
            g.queue.push(id.to_string());
            PREVIEW_CV.notify_one();
            None
        }
        Some(p) if !p.ready => None,
        Some(p) => {
            let rt = st().rt.as_ref()?;
            let bp = D2D1_BITMAP_PROPERTIES { pixelFormat: pixel_format(), dpiX: 0.0, dpiY: 0.0 };
            let bmp = unsafe {
                rt.CreateBitmap(D2D_SIZE_U { width: p.w, height: p.h }, Some(p.px.as_ptr() as *const c_void), p.w * 4, &bp)
            }
            .ok()?;
            let v = (bmp, now(), p.mask);
            f.bmps.insert(id.to_string(), v.clone());
            Some(v)
        }
    }
}

// Pictures that failed (offline) are tried again the next time the fonts are opened.
fn retry_failed() {
    SHARED.lock().unwrap().previews.retain(|_, p| !p.failed);
}

// A font that's only on this PC is previewed with the font itself.
fn local_format(file: &str) -> Option<IDWriteTextFormat> {
    let f = fu();
    if let Some(v) = f.local_fmt.get(file) {
        return v.clone();
    }
    let fmt = (|| unsafe {
        let dw = &st().dw;
        let f5: IDWriteFactory5 = dw.cast().ok()?;
        let b = f5.CreateFontSetBuilder().ok()?;
        let ff = f5.CreateFontFileReference(&HSTRING::from(format!("{}\\{}", fonts_dir(), file)), None).ok()?;
        b.AddFontFile(&ff).ok()?;
        let coll = f5.CreateFontCollectionFromFontSet(&b.CreateFontSet().ok()?).ok()?;
        if coll.GetFontFamilyCount() == 0 {
            return None;
        }
        let names = coll.GetFontFamily(0).ok()?.GetFamilyNames().ok()?;
        let mut buf = [0u16; 256];
        names.GetString(0, &mut buf).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
        let fmt = dw
            .CreateTextFormat(PCWSTR(wz(&String::from_utf16_lossy(&buf[..len])).as_ptr()), &coll, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 17.0, w!("en-us"))
            .ok()?;
        let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
        let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        Some(fmt)
    })();
    f.local_fmt.insert(file.to_string(), fmt.clone());
    fmt
}

// ---- using a font ---------------------------------------------------------------
// Switches every text in the bar to `file` from the Fonts folder ("" = the Windows font).
pub fn apply_font(file: &str) {
    write_string("Font", file);
    rebuild_formats();
}

fn activate_item(item: usize) {
    let Some(it) = fu().cat.get(item).cloned() else { return };
    if is_local(&it) {
        if !in_use(&it) {
            apply_font(&it.file);
        }
    } else {
        start_download(item);
    }
}

pub fn open_folder() {
    let dir = fonts_dir();
    let _ = std::fs::create_dir_all(&dir);
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    st().prev_fg = HWND::default();
    hide();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        ShellExecuteW(HWND::default(), w!("open"), w!("explorer.exe"), &HSTRING::from(format!("\"{}\"", dir)), PCWSTR::null(), SW_SHOWNORMAL);
        CoUninitialize();
    });
}

// ---- drawing -------------------------------------------------------------------------
fn progress_ring(c: D2D_POINT_2F, r: f32, p: f32, track: D2D1_COLOR_F, fill: D2D1_COLOR_F) {
    unsafe {
        set_color(track);
        rt().DrawEllipse(&D2D1_ELLIPSE { point: c, radiusX: r, radiusY: r }, solid(), 2.0, None::<&ID2D1StrokeStyle>);
        let p = p.clamp(0.02, 0.999);
        let Ok(geo) = st().d2d.CreatePathGeometry() else { return };
        let Ok(sink) = geo.Open() else { return };
        let a = p * std::f32::consts::TAU;
        sink.BeginFigure(pt(c.x, c.y - r), D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: pt(c.x + r * a.sin(), c.y - r * a.cos()),
            size: D2D_SIZE_F { width: r, height: r },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: if p > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
        });
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        let _ = sink.Close();
        set_color(fill);
        rt().DrawGeometry(&geo, solid(), 2.0, None::<&ID2D1StrokeStyle>);
    }
}

pub fn draw_grid(top: f32, dt: f32, alpha: f32, t_now: f64) {
    let s = st();
    let th = s.th;
    let f = fu();
    f.btn_hits.clear();
    f.default_hit = None;
    let cur = font_file();

    // header: title on the left; "Use default font" (or the keys) on the right
    let n = f.list.len();
    let title = if n == f.cat.len() { "Fonts".to_string() } else { format!("{} {}", n, if n == 1 { "font" } else { "fonts" }) };
    draw_layout(&ws(&title), &s.fmt_header, MARGIN + GRID_PAD + 2.0, top + GRID_PAD - 4.0, 200.0, GRID_HEADER_H, with_a(th.dim, alpha));
    if !cur.is_empty() {
        let w = 128.0;
        let x = MARGIN + BAR_W - GRID_PAD - w;
        let y = top + GRID_PAD - 1.0;
        let r = rect(x, y, x + w, y + GRID_HEADER_H - 6.0);
        fill_rr(r, 11.0, with_a(th.text, alpha * if f.default_hot { 0.14 } else { 0.08 }));
        draw_layout(&ws("Use default font"), &s.fmt_card, x, y, w, GRID_HEADER_H - 6.0, with_a(th.text, alpha * 0.9));
        f.default_hit = Some(r);
    } else {
        draw_layout(&ws("↵ download / use"), &s.fmt_row_right, MARGIN + BAR_W - GRID_PAD - 260.0, top + GRID_PAD - 4.0, 258.0, GRID_HEADER_H, with_a(th.dim, alpha * 0.85));
    }

    let c_top = top + TOP_H;
    let c_bottom = top + s.panel_h;
    f.off = damp(f.off, f.off_target, 18.0, dt);
    if (f.off - f.off_target).abs() < 0.3 {
        f.off = f.off_target;
    }
    unsafe { rt().PushAxisAlignedClip(&rect(MARGIN, c_top, MARGIN + BAR_W, c_bottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };

    if f.list.is_empty() {
        draw_layout(&ws("No fonts match"), &s.fmt_row, MARGIN + GRID_PAD + 2.0, c_top, 300.0, 40.0, with_a(th.dim, alpha));
    }
    for (text, hy) in &f.headers {
        let y = c_top + hy - f.off;
        if y > c_bottom || y + HEAD_H < c_top {
            continue;
        }
        draw_layout(&ws(text), &s.fmt_header, MARGIN + GRID_PAD + 2.0, y, 400.0, HEAD_H - 2.0, with_a(th.dim, alpha * 0.9));
    }

    let (progress, failed) = {
        let g = SHARED.lock().unwrap();
        (g.progress.clone(), g.failed.clone())
    };
    let sample = ws(SAMPLE);
    for k in 0..f.cells.len() {
        let (item, x, cy) = f.cells[k];
        let y = c_top + cy - f.off;
        f.cell_sel[k] = damp(f.cell_sel[k], if k as i32 == s.sel { 1.0 } else { 0.0 }, 16.0, dt);
        if y > c_bottom || y + ROW_H_F < c_top {
            continue;
        }
        let it = f.cat[item].clone();
        let sel = f.cell_sel[k];
        let card_a = alpha * clamp01(s.content_a as f64 * 1.3);
        let card = rect(x, y, x + CARD_W, y + PREV_H);
        let hit = rect(x, y.max(c_top), x + CARD_W, (y + PREV_H + NAME_H).min(c_bottom));
        if hit.bottom > hit.top {
            s.hits.push((hit, k as i32));
        }
        fill_rr(card, 9.0, with_a(th.text, card_a * (0.05 + 0.04 * sel)));

        // the picture: the font's own letters
        let local = is_local(&it);
        if item >= f.cat_n {
            if let Some(fmt) = local_format(&it.file) {
                unsafe { rt().PushAxisAlignedClip(&card, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
                draw_layout(&sample, &fmt, x + 10.0, y + 2.0, CARD_W - 20.0, PREV_H - 4.0, with_a(th.text, card_a));
                unsafe { rt().PopAxisAlignedClip() };
            }
        } else if let Some((bmp, shown, mask)) = get_preview(&it.id) {
            let fade = clamp01((t_now - shown) / 0.25);
            let size = unsafe { bmp.GetPixelSize() };
            let src = rect(0.0, 0.0, size.width as f32, size.height as f32);
            unsafe {
                if mask {
                    set_color(with_a(th.text, card_a * fade));
                    rt().SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
                    rt().FillOpacityMask(&bmp, solid(), D2D1_OPACITY_MASK_CONTENT_GRAPHICS, Some(&card), Some(&src));
                    rt().SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                } else {
                    rt().DrawBitmap(&bmp, Some(&card), card_a * fade, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&src));
                }
            }
        } else {
            // loading: two soft lines where the text will be
            let pulse = 0.05 + 0.03 * ((t_now * 4.0 + k as f64 * 0.6).sin() as f32);
            fill_rr(rect(x + 10.0, y + 18.0, x + CARD_W - 24.0, y + 28.0), 5.0, with_a(th.text, card_a * pulse));
            fill_rr(rect(x + 10.0, y + 36.0, x + CARD_W * 0.6, y + 46.0), 5.0, with_a(th.text, card_a * pulse));
        }

        if sel > 0.01 {
            set_color(with_a(th.accent, card_a * sel));
            unsafe {
                rt().DrawRoundedRectangle(&rrect(rect(x - 2.0, y - 2.0, x + CARD_W + 2.0, y + PREV_H + 2.0), 11.0), solid(), 2.0, None::<&ID2D1StrokeStyle>)
            };
        }

        // top-right corner: in use, downloading, or the Download button
        let bc = pt(x + CARD_W - 15.0, y + 15.0);
        let using = in_use(&it);
        if using {
            set_color(with_a(th.accent, card_a));
            unsafe { rt().FillEllipse(&D2D1_ELLIPSE { point: bc, radiusX: 9.0, radiusY: 9.0 }, solid()) };
            draw_layout(&ws("\u{E73E}"), &s.fmt_glyph_sm, bc.x - 8.0, bc.y - 8.0, 16.0, 16.0, with_a(c(0xFFFFFF, 1.0), card_a));
        } else if let Some(&p) = progress.get(&it.id) {
            progress_ring(bc, 8.0, p, with_a(th.text, card_a * 0.15), with_a(th.accent, card_a));
        } else if !local {
            let hot = f.btn_hot == k as i32;
            set_color(if hot { with_a(th.accent, card_a) } else { with_a(th.text, card_a * (0.10 + 0.06 * sel)) });
            unsafe { rt().FillEllipse(&D2D1_ELLIPSE { point: bc, radiusX: 11.0, radiusY: 11.0 }, solid()) };
            let fail = failed.contains(&it.id);
            let col = if hot { with_a(c(0xFFFFFF, 1.0), card_a) } else { with_a(if fail { c(0xE0605A, 1.0) } else { th.text }, card_a * 0.9) };
            draw_layout(&ws(if fail { "\u{E72C}" } else { "\u{E896}" }), &s.fmt_glyph_sm, bc.x - 8.0, bc.y - 8.0, 16.0, 16.0, col);
            let br = rect(bc.x - 13.0, bc.y - 13.0, bc.x + 13.0, bc.y + 13.0);
            if br.top >= c_top && br.bottom <= c_bottom {
                f.btn_hits.push((br, k as i32));
            }
        }

        let nc = lerp_c(th.dim, th.text, sel.max(if using { 1.0 } else { 0.0 }));
        draw_layout(&ws(&it.name), &s.fmt_card, x + 4.0, y + PREV_H + 2.0, CARD_W - 8.0, NAME_H - 4.0, with_a(nc, card_a));
    }
    unsafe { rt().PopAxisAlignedClip() };

    // scroll bar
    let view = c_bottom - c_top;
    if f.content_h + 6.0 > view + 1.0 {
        let track_h = view - 8.0;
        let thumb_h = (track_h * view / (f.content_h + 6.0)).max(24.0);
        let t = if max_off() > 0.0 { f.off / max_off() } else { 0.0 };
        let ty = c_top + 4.0 + (track_h - thumb_h) * t.clamp(0.0, 1.0);
        fill_rr(rect(MARGIN + BAR_W - 6.0, ty, MARGIN + BAR_W - 3.0, ty + thumb_h), 1.5, with_a(th.dim, alpha * 0.45));
    }
}

fn button_at(x: f32, y: f32) -> i32 {
    fu().btn_hits.iter().find(|(r, _)| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom).map(|h| h.1).unwrap_or(-1)
}
fn default_at(x: f32, y: f32) -> bool {
    fu().default_hit.map(|r| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom).unwrap_or(false)
}

// ---- glue for main.rs -------------------------------------------------------------------
// Opening the fonts: the one in use is selected and scrolled to.
pub fn enter() {
    retry_failed();
    let s = st();
    s.sel = 0;
    let f = fu();
    for (k, &i) in f.list.iter().enumerate() {
        if in_use(&f.cat[i]) {
            s.sel = k as i32;
        }
    }
    f.off = 0.0;
    f.off_target = 0.0;
    ensure_visible(s.sel, true);
}

pub fn activate_sel() {
    let s = st();
    if s.sel >= 0 && (s.sel as usize) < fu().list.len() {
        activate_item(fu().list[s.sel as usize]);
    }
}

pub fn on_key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    let n = fu().list.len() as i32;
    if n == 0 {
        return false;
    }
    let sel = st().sel;
    let k = match vk {
        VK_UP => vertical(sel, -1),
        VK_DOWN => vertical(sel, 1),
        VK_LEFT if !shift => sel - 1,
        VK_RIGHT if !shift => sel + 1,
        VK_PRIOR => (0..3).fold(sel, |k, _| vertical(k, -1)),
        VK_NEXT => (0..3).fold(sel, |k, _| vertical(k, 1)),
        _ => return false,
    };
    select(k.clamp(0, n - 1));
    ensure_visible(st().sel, false);
    true
}

// A click in the panel: "Use default font", a Download button, or a card (a font that's
// already on this PC is used right away; others wait for their Download button).
pub fn click(x: f32, y: f32) {
    if default_at(x, y) {
        apply_font("");
        return;
    }
    let b = button_at(x, y);
    let list_len = fu().list.len() as i32;
    if b >= 0 && b < list_len {
        st().sel = b;
        start_download(fu().list[b as usize]);
        return;
    }
    let k = hit_item_at(x, y);
    if k >= 0 && k < list_len {
        st().sel = k;
        let item = fu().list[k as usize];
        if is_local(&fu().cat[item]) {
            activate_item(item);
        }
    }
}

pub fn wheel(steps: i32) {
    scroll_to(fu().off_target + steps as f32 * 64.0, false);
}

pub fn hover(x: f32, y: f32) {
    let on = st().mode == Mode::Fonts;
    fu().btn_hot = if on { button_at(x, y) } else { -1 };
    fu().default_hot = on && default_at(x, y);
}
pub fn hand_at(x: f32, y: f32) -> bool {
    st().mode == Mode::Fonts && (button_at(x, y) >= 0 || default_at(x, y))
}

// A download finished: use the font, and refresh the cards (keeping the scroll position).
pub fn downloaded(id: &str, ok: bool) {
    {
        let mut g = SHARED.lock().unwrap();
        g.progress.remove(id);
        if !ok {
            g.failed.insert(id.to_string());
        }
    }
    let file = fu().cat[..fu().cat_n].iter().find(|i| i.id == id).map(|i| i.file.clone());
    if let (true, Some(file)) = (ok, file) {
        apply_font(&file);
    }
    if st().mode == Mode::Fonts {
        let sel = st().sel;
        let (off, target) = (fu().off, fu().off_target);
        let filter = fu().filter.clone();
        build_list(&filter);
        st().sel = sel.clamp(0, (fu().list.len() as i32 - 1).max(0));
        fu().off = off.min(max_off());
        fu().off_target = target.min(max_off());
    }
}
