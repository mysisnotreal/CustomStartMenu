// Live widgets: what's playing (+ visualizer), weather, this PC, network, clock & calendar,
// timer, stopwatch, volume, brightness, and the status strip on the empty bar.
// Nothing personal is shown: weather only names the country, network only the kind of
// connection and its speed. Same as cpp/widgets.inl.

use super::panels::*;
use super::*;
use std::f32::consts::PI;
use std::sync::atomic::AtomicBool;
use windows::Win32::Globalization::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::SystemInformation::*;
use windows::Win32::System::Time::*;

fn sys_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}
fn locale_info(lc: u32) -> String {
    let mut b = [0u16; 32];
    unsafe { GetLocaleInfoEx(PCWSTR::null(), lc, Some(&mut b)) };
    sys_wide(&b)
}
fn ft_u(f: &FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

// ---- images -------------------------------------------------------------------------------
// Decodes a PNG / JPEG into premultiplied BGRA, scaled to fit max_side.
pub fn decode_image(data: &[u8], max_side: u32) -> Option<(Vec<u32>, u32, u32)> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(data).ok()?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let fr = dec.GetFrame(0).ok()?;
        let (mut fw, mut fh) = (0u32, 0u32);
        fr.GetSize(&mut fw, &mut fh).ok()?;
        if fw == 0 || fh == 0 {
            return None;
        }
        let k = (max_side as f32 / fw.max(fh) as f32).min(1.0);
        let w = ((fw as f32 * k).round() as u32).max(1);
        let h = ((fh as f32 * k).round() as u32).max(1);
        let sc = wic.CreateBitmapScaler().ok()?;
        sc.Initialize(&fr, w, h, WICBitmapInterpolationModeHighQualityCubic).ok()?;
        let cv = wic.CreateFormatConverter().ok()?;
        cv.Initialize(&sc, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let mut px = vec![0u32; (w * h) as usize];
        cv.CopyPixels(std::ptr::null(), w * 4, std::slice::from_raw_parts_mut(px.as_mut_ptr() as *mut u8, px.len() * 4)).ok()?;
        Some((px, w, h))
    }
}

pub fn make_bitmap(px: &[u32], w: u32, h: u32) -> Option<ID2D1Bitmap> {
    if px.is_empty() {
        return None;
    }
    let rt = st().rt.as_ref()?;
    let bp = D2D1_BITMAP_PROPERTIES { pixelFormat: pixel_format(), dpiX: 0.0, dpiY: 0.0 };
    unsafe { rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(px.as_ptr() as *const c_void), w * 4, &bp).ok() }
}

// Fills a rounded rectangle with a bitmap (cropped to a square, like album art).
fn fill_rounded_bitmap(bmp: &ID2D1Bitmap, r: D2D_RECT_F, radius: f32, alpha: f32) {
    unsafe {
        let props = D2D1_BITMAP_BRUSH_PROPERTIES {
            extendModeX: D2D1_EXTEND_MODE_CLAMP,
            extendModeY: D2D1_EXTEND_MODE_CLAMP,
            interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
        };
        let bprops = D2D1_BRUSH_PROPERTIES { opacity: alpha, transform: m_identity() };
        let Ok(bb) = rt().CreateBitmapBrush(bmp, Some(&props), Some(&bprops)) else { return };
        let sz = bmp.GetSize();
        let side = sz.width.min(sz.height);
        let k = (r.right - r.left) / side;
        let m = m_mul(
            &m_mul(&m_translate(-(sz.width - side) / 2.0, -(sz.height - side) / 2.0), &m_scale(k, k, 0.0, 0.0)),
            &m_translate(r.left, r.top),
        );
        bb.SetTransform(&m);
        rt().FillRoundedRectangle(&rrect(r, radius), &bb);
    }
}

// A clickable spot in a self-drawn panel.
static mut PBTN: Vec<(D2D_RECT_F, i32)> = Vec::new();
fn pbtn() -> &'static mut Vec<(D2D_RECT_F, i32)> {
    unsafe { &mut *std::ptr::addr_of_mut!(PBTN) }
}
pub fn button_at(x: f32, y: f32) -> i32 {
    for (r, id) in pbtn().iter() {
        if x >= r.left && x <= r.right && y >= r.top && y <= r.bottom {
            return *id;
        }
    }
    -1
}

// ======================================================================================
// Media
// ======================================================================================
#[derive(Clone, Default)]
pub struct MediaInfo {
    pub has: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub app: String,
    pub pos: f64,
    pub dur: f64,
    pub can_prev: bool,
    pub can_next: bool,
}
#[derive(Default)]
struct MediaState {
    info: MediaInfo,
    read_at: f64,
    art: Vec<u32>,
    art_w: u32,
    art_h: u32,
    art_ver: i32,
}
static MEDIA: Mutex<Option<MediaState>> = Mutex::new(None);
static MEDIA_CMD: AtomicI32 = AtomicI32::new(-1);
static MEDIA_EVENT: AtomicIsize = AtomicIsize::new(0);
static mut ART_BMP: Option<(ID2D1Bitmap, i32)> = None;

fn with_media<R>(f: impl FnOnce(&mut MediaState) -> R) -> R {
    let mut g = MEDIA.lock().unwrap();
    f(g.get_or_insert_with(MediaState::default))
}

mod wb {
    use super::MediaInfo;
    use windows::core::Interface;
    use windows::Devices::Radios::*;
    use windows::Media::Control::*;
    use windows::Storage::Streams::*;

    fn manager() -> windows::core::Result<GlobalSystemMediaTransportControlsSessionManager> {
        static mut M: Option<GlobalSystemMediaTransportControlsSessionManager> = None;
        unsafe {
            let m = &mut *std::ptr::addr_of_mut!(M);
            if m.is_none() {
                *m = Some(GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?);
            }
            Ok(m.clone().unwrap())
        }
    }
    fn now_100ns() -> i64 {
        let ft = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
        (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) as i64
    }

    // Ok(None) when nothing is playing.
    pub fn poll(want_thumb: bool) -> windows::core::Result<(MediaInfo, Vec<u8>)> {
        let mut out = MediaInfo::default();
        let mut thumb = Vec::new();
        let Ok(session) = manager()?.GetCurrentSession() else { return Ok((out, thumb)) };
        let props = session.TryGetMediaPropertiesAsync()?.get()?;
        let pb = session.GetPlaybackInfo()?;
        let tl = session.GetTimelineProperties()?;
        out.has = true;
        out.title = props.Title()?.to_string();
        out.artist = props.Artist()?.to_string();
        if out.artist.is_empty() {
            out.artist = props.AlbumArtist()?.to_string();
        }
        out.app = session.SourceAppUserModelId()?.to_string();
        out.playing = pb.PlaybackStatus()? == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        let ctl = pb.Controls()?;
        out.can_prev = ctl.IsPreviousEnabled()?;
        out.can_next = ctl.IsNextEnabled()?;
        let start = tl.StartTime()?.Duration;
        out.dur = (tl.EndTime()?.Duration - start) as f64 / 1e7;
        out.pos = (tl.Position()?.Duration - start) as f64 / 1e7;
        if out.playing {
            // the position is from when the app last reported it
            let s = (now_100ns() - tl.LastUpdatedTime()?.UniversalTime) as f64 / 1e7;
            if s > 0.0 && s < 24.0 * 3600.0 {
                out.pos += s;
            }
        }
        if out.dur > 0.0 {
            out.pos = out.pos.clamp(0.0, out.dur);
        }
        if want_thumb {
            if let Ok(r) = props.Thumbnail() {
                let stream = r.OpenReadAsync()?.get()?;
                let size = stream.Size()? as u32;
                if size > 0 && size < (16 << 20) {
                    let reader = DataReader::CreateDataReader(&stream.cast::<IInputStream>()?)?;
                    let got = reader.LoadAsync(size)?.get()?;
                    thumb = vec![0u8; got as usize];
                    reader.ReadBytes(&mut thumb)?;
                }
            }
        }
        Ok((out, thumb))
    }

    pub fn command(cmd: i32) {
        let _ = (|| -> windows::core::Result<()> {
            let s = manager()?.GetCurrentSession()?;
            match cmd {
                0 => s.TryTogglePlayPauseAsync()?.get()?,
                1 => s.TrySkipNextAsync()?.get()?,
                _ => s.TrySkipPreviousAsync()?.get()?,
            };
            Ok(())
        })();
    }

    fn find_radio(kind: i32) -> windows::core::Result<Option<Radio>> {
        static mut ASKED: bool = false;
        unsafe {
            if !ASKED {
                ASKED = true;
                if let Ok(op) = Radio::RequestAccessAsync() {
                    let _ = op.get();
                }
            }
        }
        let want = if kind == 0 { RadioKind::WiFi } else { RadioKind::Bluetooth };
        for r in Radio::GetRadiosAsync()?.get()? {
            if r.Kind()? == want {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }
    // -1 none, 0 off, 1 on
    pub fn radio_state(kind: i32) -> i32 {
        match find_radio(kind) {
            Ok(Some(r)) => match r.State() {
                Ok(s) => (s == RadioState::On) as i32,
                Err(_) => -1,
            },
            _ => -1,
        }
    }
    pub fn radio_set(kind: i32, on: bool) -> bool {
        let Ok(Some(r)) = find_radio(kind) else { return false };
        let st = if on { RadioState::On } else { RadioState::Off };
        matches!(r.SetStateAsync(st).and_then(|op| op.get()), Ok(RadioAccessStatus::Allowed))
    }
}
pub use wb::{radio_set, radio_state};

fn quit_event() -> HANDLE {
    HANDLE(QUIT_EVENT.load(Relaxed) as *mut c_void)
}

fn media_worker() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut last_key = String::new();
        let mut art_tries = 0;
        loop {
            if WaitForSingleObject(quit_event(), 0) == WAIT_OBJECT_0 {
                break;
            }
            let cmd = MEDIA_CMD.swap(-1, SeqCst);
            if cmd >= 0 {
                wb::command(cmd);
                std::thread::sleep(std::time::Duration::from_millis(250)); // let the player catch up
            }
            let open = BAR_OPEN.load(Relaxed);
            if open || cmd >= 0 {
                let r = wb::poll(false);
                let ok = r.is_ok();
                let mi = r.map(|x| x.0).unwrap_or_default();
                let key = format!("{}\x01{}\x01{}", mi.title, mi.artist, mi.app);
                let mut new_art = false;
                let mut art: Option<(Vec<u32>, u32, u32)> = None;
                if ok && mi.has {
                    if key != last_key {
                        last_key = key;
                        art_tries = 0;
                    }
                    if art_tries < 4 {
                        // players often send the cover a moment after the title
                        art_tries += 1;
                        if let Ok((_, th)) = wb::poll(true) {
                            if !th.is_empty() {
                                art = decode_image(&th, 192);
                            }
                        }
                        if art.is_some() {
                            art_tries = 99;
                            new_art = true;
                        } else if art_tries == 1 {
                            new_art = true; // clears the previous song's cover
                        }
                    }
                } else if ok && !last_key.is_empty() {
                    last_key.clear();
                    new_art = true;
                }
                if ok {
                    with_media(|m| {
                        m.info = mi;
                        m.read_at = now();
                        if new_art {
                            let (px, w, h) = art.unwrap_or_default();
                            m.art = px;
                            m.art_w = w;
                            m.art_h = h;
                            m.art_ver += 1;
                        }
                    });
                }
            }
            let hs = [quit_event(), HANDLE(MEDIA_EVENT.load(Relaxed) as *mut c_void)];
            WaitForMultipleObjects(&hs, FALSE, if BAR_OPEN.load(Relaxed) { 1000 } else { INFINITE });
        }
        CoUninitialize();
    }
}
fn media_wake() {
    let e = MEDIA_EVENT.load(Relaxed);
    if e != 0 {
        unsafe {
            let _ = SetEvent(HANDLE(e as *mut c_void));
        }
    }
}
fn media_send(cmd: i32) {
    MEDIA_CMD.store(cmd, SeqCst);
    media_wake();
    // show the change right away
    with_media(|m| {
        if cmd == 0 && m.info.has {
            let p = m.info.pos + if m.info.playing { now() - m.read_at } else { 0.0 };
            m.info.playing = !m.info.playing;
            m.info.pos = p;
            m.read_at = now();
        }
    });
}
pub fn media_now() -> (MediaInfo, f64) {
    with_media(|m| {
        let i = m.info.clone();
        let mut p = i.pos + if i.playing { now() - m.read_at } else { 0.0 };
        if i.dur > 0.0 {
            p = p.min(i.dur);
        }
        (i, p)
    })
}
fn media_art() -> Option<ID2D1Bitmap> {
    with_media(|m| unsafe {
        let a = &mut *std::ptr::addr_of_mut!(ART_BMP);
        let ver = a.as_ref().map(|x| x.1).unwrap_or(-1);
        if ver != m.art_ver || (a.is_none() && !m.art.is_empty()) {
            *a = make_bitmap(&m.art, m.art_w, m.art_h).map(|b| (b, m.art_ver));
            if a.is_none() {
                return None;
            }
        }
        a.as_ref().map(|x| x.0.clone())
    })
}
// "Spotify", "Firefox" from the player's app id.
fn player_name(app: &str) -> String {
    let mut s = app.to_string();
    if let Some(bang) = s.find('!') {
        // packaged: "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"
        let tail = &s[bang + 1..];
        if tail.to_lowercase() != "app" {
            return tail.to_string();
        }
        s.truncate(bang);
        if let Some(u) = s.find('_') {
            s.truncate(u);
        }
        return match s.rfind('.') {
            Some(d) => s[d + 1..].to_string(),
            None => s,
        };
    }
    if let Some(sl) = s.rfind(['\\', '/']) {
        s = s[sl + 1..].to_string();
    }
    if s.to_lowercase().ends_with(".exe") && s.len() > 4 {
        s.truncate(s.len() - 4);
    }
    let mut c = s.chars();
    let s = match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    };
    if s.to_lowercase() == "msedge" {
        return "Edge".into();
    }
    s
}
pub fn clock(sec: f64) -> String {
    let s = sec.max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

// ---- visualizer: what the speakers play (WASAPI loopback) -> 28 bands ------------------
const VIZ_BANDS: usize = 28;
static VIZ_RAW: Mutex<[f32; VIZ_BANDS]> = Mutex::new([0.0; VIZ_BANDS]);
static mut VIZ_SHOWN: [f32; VIZ_BANDS] = [0.0; VIZ_BANDS];
static VIZ_WANT: AtomicBool = AtomicBool::new(false);
static VIZ_RUNNING: AtomicBool = AtomicBool::new(false);

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn viz_worker() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let run = || -> windows::core::Result<()> {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let ac: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let wf = ac.GetMixFormat()?;
            let res = (|| -> windows::core::Result<()> {
                ac.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 200000, 0, wf, None)?;
                let cap: IAudioCaptureClient = ac.GetService()?;
                ac.Start()?;
                let tag = (*wf).wFormatTag;
                let sub1 = if tag == 0xFFFE { std::ptr::read_unaligned((wf as *const u8).add(24) as *const u32) } else { 0 };
                let is_float = tag == 3 || (tag == 0xFFFE && sub1 == 3);
                let ch = ((*wf).nChannels as usize).max(1);
                let bits = (*wf).wBitsPerSample;
                let rate = (*wf).nSamplesPerSec as f32;
                const N: usize = 2048;
                let mut ring = vec![0f32; N];
                let (mut re, mut im) = (vec![0f32; N], vec![0f32; N]);
                let mut head = 0usize;
                let mut last_sound = 0.0;
                while VIZ_WANT.load(Relaxed) && WaitForSingleObject(quit_event(), 12) == WAIT_TIMEOUT {
                    let mut any = false;
                    while let Ok(pk) = cap.GetNextPacketSize() {
                        if pk == 0 {
                            break;
                        }
                        let mut data: *mut u8 = null_mut();
                        let (mut frames, mut flags) = (0u32, 0u32);
                        if cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                            break;
                        }
                        for f in 0..frames as usize {
                            let mut v = 0.0f32;
                            if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) == 0 {
                                for c in 0..ch {
                                    let i = f * ch + c;
                                    v += if is_float && bits == 32 {
                                        *(data as *const f32).add(i)
                                    } else if bits == 16 {
                                        *(data as *const i16).add(i) as f32 / 32768.0
                                    } else if bits == 32 {
                                        *(data as *const i32).add(i) as f32 / 2147483648.0
                                    } else {
                                        0.0
                                    };
                                }
                                v /= ch as f32;
                            }
                            ring[head] = v;
                            head = (head + 1) % N;
                        }
                        let _ = cap.ReleaseBuffer(frames);
                        any = true;
                    }
                    if any {
                        last_sound = now();
                    }
                    let mut bands = [0f32; VIZ_BANDS];
                    if now() - last_sound < 0.3 {
                        for i in 0..N {
                            let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / (N - 1) as f32).cos(); // Hann window
                            re[i] = ring[(head + i) % N] * w;
                            im[i] = 0.0;
                        }
                        fft(&mut re, &mut im);
                        // log-spaced bands from 45 Hz to 14 kHz
                        for (b, band) in bands.iter_mut().enumerate() {
                            let f0 = 45.0 * (14000.0f32 / 45.0).powf(b as f32 / VIZ_BANDS as f32);
                            let f1 = 45.0 * (14000.0f32 / 45.0).powf((b + 1) as f32 / VIZ_BANDS as f32);
                            let i0 = ((f0 / rate * N as f32) as usize).max(1);
                            let i1 = ((f1 / rate * N as f32) as usize).max(i0 + 1);
                            let mut m = 0.0f32;
                            for i in i0..i1.min(N / 2) {
                                m = m.max((re[i] * re[i] + im[i] * im[i]).sqrt());
                            }
                            let db = 20.0 * (m / (N as f32 / 4.0) + 1e-9).log10();
                            *band = ((db + 62.0) / 56.0).clamp(0.0, 1.0);
                        }
                    }
                    *VIZ_RAW.lock().unwrap() = bands;
                }
                let _ = ac.Stop();
                Ok(())
            })();
            CoTaskMemFree(Some(wf as *const c_void));
            res
        };
        let _ = run();
        *VIZ_RAW.lock().unwrap() = [0.0; VIZ_BANDS];
        VIZ_RUNNING.store(false, SeqCst);
        CoUninitialize();
    }
}
fn viz_start() {
    VIZ_WANT.store(true, SeqCst);
    if VIZ_RUNNING.swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(viz_worker);
}
pub fn viz_stop() {
    VIZ_WANT.store(false, SeqCst);
}

// Bars rising from the bottom of r.
fn draw_viz(r: D2D_RECT_F, alpha: f32, dt: f32) {
    let raw = *VIZ_RAW.lock().unwrap();
    let th = st().th;
    let shown = unsafe { &mut *std::ptr::addr_of_mut!(VIZ_SHOWN) };
    let gap = 3.0;
    let bw = ((r.right - r.left) - gap * (VIZ_BANDS as f32 - 1.0)) / VIZ_BANDS as f32;
    for b in 0..VIZ_BANDS {
        let target = raw[b];
        // quick to rise, slow to fall
        shown[b] = damp(shown[b], target, if target > shown[b] { 30.0 } else { 6.0 }, dt);
        let hgt = ((r.bottom - r.top) * shown[b]).max(2.0);
        let x = r.left + b as f32 * (bw + gap);
        let c = lerp_c(th.accent, th.text, 0.15 * b as f32 / VIZ_BANDS as f32);
        if let Some(br) = make_gradient(pt(0.0, r.bottom - hgt), pt(0.0, r.bottom), with_a(c, alpha), with_a(c, alpha * 0.45)) {
            unsafe { rt().FillRoundedRectangle(&rrect(rect(x, r.bottom - hgt, x + bw, r.bottom), bw / 2.0), &br) };
        }
    }
}

// ---- the panel ----------------------------------------------------------------------------
const MB_PREV: i32 = 1;
const MB_PLAY: i32 = 2;
const MB_NEXT: i32 = 3;
const MB_SEEK: i32 = 4;
pub fn media_height() -> f32 {
    170.0
}
pub fn media_enter(_: i32, _: bool) {
    media_send(0);
}
pub fn draw_media(top: f32, dt: f32, alpha: f32, _t: f64) {
    let s = st();
    let th = s.th;
    pbtn().clear();
    viz_start();
    let (m, pos) = media_now();
    let (art, x0, y0) = (132.0, MARGIN + 18.0, top + 18.0);
    let ar = rect(x0, y0, x0 + art, y0 + art);
    let bmp = if m.has { media_art() } else { None };
    if let Some(b) = &bmp {
        draw_shadow(ar, 0.35 * alpha, &s.tile_shadow);
        fill_rounded_bitmap(b, ar, 10.0, alpha);
    } else {
        draw_card(ar, alpha, 10.0);
        dl("\u{E8D6}", &s.fmt_big_c, ar.left, ar.top, art, art, with_a(th.dim, alpha * 0.8));
    }
    let tx = ar.right + 22.0;
    let tw = MARGIN + bar_w() - 20.0 - tx;
    if !m.has {
        dl("Nothing is playing", &s.fmt_mid, tx, y0 + 18.0, tw, 30.0, with_a(th.text, alpha));
        if let Some(l) = make_layout(&ws("Play something in Spotify, a browser or any music app, and it shows up here."), &s.fmt_row, tw, 44.0) {
            unsafe {
                let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                set_color(with_a(th.dim, alpha));
                rt().DrawTextLayout(pt(tx, y0 + 50.0), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
        return;
    }
    dl(if m.title.is_empty() { "Unknown title" } else { &m.title }, &s.fmt_mid, tx, y0 + 2.0, tw, 28.0, with_a(th.text, alpha));
    let mut sub = m.artist.clone();
    let player = player_name(&m.app);
    if !player.is_empty() {
        if !sub.is_empty() {
            sub.push_str("  ·  ");
        }
        sub.push_str(&player);
    }
    dl(&sub, &s.fmt_row, tx, y0 + 30.0, tw, 22.0, with_a(th.dim, alpha));

    // progress
    let py = y0 + 66.0;
    if m.dur > 0.0 {
        let f = (pos / m.dur).clamp(0.0, 1.0) as f32;
        fill_rr(rect(tx, py, tx + tw, py + 4.0), 2.0, with_a(th.dim, alpha * 0.25));
        fill_rr(rect(tx, py, tx + tw * f, py + 4.0), 2.0, with_a(th.accent, alpha));
        fill_circle(tx + tw * f, py + 2.0, 5.0, with_a(th.accent, alpha));
        dl(&clock(pos), &s.fmt_small, tx, py + 7.0, 80.0, 16.0, with_a(th.dim, alpha));
        dl(&clock(m.dur), &s.fmt_small_r, tx + tw - 80.0, py + 7.0, 80.0, 16.0, with_a(th.dim, alpha));
    }

    // controls
    let cy = y0 + 112.0;
    let bs = [
        (MB_PREV, "\u{E892}", tx + 18.0, 17.0, m.can_prev),
        (MB_PLAY, if m.playing { "\u{E769}" } else { "\u{E768}" }, tx + 64.0, 21.0, true),
        (MB_NEXT, "\u{E893}", tx + 110.0, 17.0, m.can_next),
    ];
    for (id, g, x, r, on) in bs {
        if id == MB_PLAY {
            fill_circle(x, cy, r, with_a(th.accent, alpha));
            dl(g, &s.fmt_glyph, x - r, cy - r, r * 2.0, r * 2.0, with_a(c(0xFFFFFF, 1.0), alpha));
        } else {
            fill_circle(x, cy, r, with_a(th.highlight, alpha * 1.4));
            dl(g, &s.fmt_glyph, x - r, cy - r, r * 2.0, r * 2.0, with_a(if on { th.text } else { th.dim }, alpha * if on { 1.0 } else { 0.5 }));
        }
        pbtn().push((rect(x - r, cy - r, x + r, cy + r), id));
    }
    if m.dur > 0.0 {
        pbtn().push((rect(tx, py - 8.0, tx + tw, py + 12.0), MB_SEEK));
    }
    // visualizer to the right of the buttons
    draw_viz(rect(tx + 150.0, cy - 22.0, tx + tw, cy + 20.0), alpha * 0.9, dt);
}
pub fn media_key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    if shift {
        return false;
    }
    match vk {
        VK_LEFT => media_send(2),
        VK_RIGHT => media_send(1),
        _ => return false,
    }
    true
}
pub fn media_click(x: f32, y: f32) -> bool {
    match button_at(x, y) {
        MB_PREV => media_send(2),
        MB_NEXT => media_send(1),
        MB_PLAY => media_send(0),
        _ => {}
    }
    true
}
pub fn media_fill(_: bool) {
    viz_start();
    media_wake();
}
pub fn volume_wheel(steps: i32) {
    let v = unsafe { VOL_SHOWN };
    if v >= 0 {
        volume_set(v - steps * 2);
    }
}
pub fn brightness_wheel(steps: i32) {
    let b = BRIGHT.load(Relaxed);
    if b >= 0 {
        brightness_set(b - steps * 5);
    }
}
pub fn media_leave() {
    viz_stop();
}

// ======================================================================================
// Weather (Open-Meteo, no account). Located by the country set in Windows (and, in big
// countries, its time zone); only the country's name is ever shown.
// ======================================================================================
#[derive(Clone)]
pub struct WeatherHour {
    hour: i32,
    t: f64,
    code: i32,
    day: bool,
}
#[derive(Clone)]
pub struct WeatherState {
    pub ok: bool,
    failed: bool,
    country: String,
    why: String,
    pub temp: f64,
    pub code: i32,
    pub day: bool,
    hours: Vec<WeatherHour>,
    at: f64,
}
impl Default for WeatherState {
    fn default() -> Self {
        WeatherState { ok: false, failed: false, country: String::new(), why: String::new(), temp: 0.0, code: 0, day: true, hours: Vec::new(), at: -1e9 }
    }
}
static WX: Mutex<Option<WeatherState>> = Mutex::new(None);
static WX_LOADING: AtomicBool = AtomicBool::new(false);

// Rough spots for time zones in countries that span several, so "the weather" isn't the
// middle of nowhere. Never shown anywhere.
const TZ_SPOTS: &[(&str, &str, f64, f64)] = &[
    ("US", "Eastern Standard Time", 40.7, -74.0),
    ("US", "Central Standard Time", 41.9, -87.6),
    ("US", "Mountain Standard Time", 39.7, -105.0),
    ("US", "US Mountain Standard Time", 33.4, -112.1),
    ("US", "Pacific Standard Time", 34.05, -118.2),
    ("US", "Alaskan Standard Time", 61.2, -149.9),
    ("US", "Hawaiian Standard Time", 21.3, -157.9),
    ("US", "US Eastern Standard Time", 39.8, -86.2),
    ("CA", "Eastern Standard Time", 43.7, -79.4),
    ("CA", "Central Standard Time", 49.9, -97.1),
    ("CA", "Canada Central Standard Time", 50.4, -104.6),
    ("CA", "Mountain Standard Time", 51.0, -114.1),
    ("CA", "Pacific Standard Time", 49.3, -123.1),
    ("CA", "Atlantic Standard Time", 44.6, -63.6),
    ("CA", "Newfoundland Standard Time", 47.6, -52.7),
    ("AU", "AUS Eastern Standard Time", -33.9, 151.2),
    ("AU", "E. Australia Standard Time", -27.5, 153.0),
    ("AU", "Cen. Australia Standard Time", -34.9, 138.6),
    ("AU", "AUS Central Standard Time", -12.5, 130.8),
    ("AU", "W. Australia Standard Time", -31.95, 115.9),
    ("AU", "Tasmania Standard Time", -42.9, 147.3),
    ("RU", "Russian Standard Time", 55.75, 37.6),
    ("RU", "Ekaterinburg Standard Time", 56.8, 60.6),
    ("RU", "N. Central Asia Standard Time", 55.0, 82.9),
    ("RU", "North Asia Standard Time", 56.0, 92.9),
    ("RU", "North Asia East Standard Time", 52.3, 104.3),
    ("RU", "Yakutsk Standard Time", 62.0, 129.7),
    ("RU", "Vladivostok Standard Time", 43.1, 131.9),
    ("RU", "Kaliningrad Standard Time", 54.7, 20.5),
    ("RU", "Omsk Standard Time", 55.0, 73.4),
    ("RU", "Samara Standard Time", 53.2, 50.1),
    ("BR", "E. South America Standard Time", -23.55, -46.6),
    ("BR", "SA Western Standard Time", -3.1, -60.0),
    ("BR", "Central Brazilian Standard Time", -15.6, -56.1),
    ("BR", "SA Eastern Standard Time", -3.7, -38.5),
    ("BR", "Bahia Standard Time", -12.97, -38.5),
    ("BR", "Tocantins Standard Time", -10.2, -48.3),
    ("MX", "Central Standard Time (Mexico)", 19.4, -99.1),
    ("MX", "Mountain Standard Time (Mexico)", 23.2, -106.4),
    ("MX", "Pacific Standard Time (Mexico)", 32.5, -117.0),
    ("MX", "Eastern Standard Time (Mexico)", 21.2, -86.8),
    ("CN", "China Standard Time", 31.2, 121.5),
    ("IN", "India Standard Time", 19.1, 72.9),
    ("ID", "SE Asia Standard Time", -6.2, 106.8),
    ("KZ", "Central Asia Standard Time", 43.2, 76.9),
];

fn atof(s: &str) -> f64 {
    let s = s.trim_start();
    let end = s.find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))).unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}
fn json_num(js: &str, from: usize, key: &str, end: usize) -> f64 {
    let k = format!("\"{}\":", key);
    match js[from..].find(&k).map(|p| p + from) {
        Some(p) if p < end => atof(&js[p + k.len()..]),
        _ => 0.0,
    }
}
fn json_array(js: &str, from: usize, key: &str) -> Vec<String> {
    let k = format!("\"{}\":[", key);
    let Some(p) = js[from..].find(&k).map(|p| p + from + k.len()) else { return Vec::new() };
    let e = js[p..].find(']').map(|x| x + p).unwrap_or(js.len());
    js[p..e].split(',').map(|v| v.trim_matches('"').to_string()).collect()
}

fn weather_fetch() {
    let mut w = WeatherState::default();
    let (mut lat, mut lon, mut have) = (0.0, 0.0, false);
    unsafe {
        let geo = GetUserGeoID(GEOCLASS_NATION);
        let mut buf = [0u16; 128];
        if geo != GEOID_NOT_AVAILABLE {
            if GetGeoInfoW(geo, GEO_FRIENDLYNAME, Some(&mut buf), 0) > 0 {
                w.country = sys_wide(&buf);
            }
            let mut iso = String::new();
            if GetGeoInfoW(geo, GEO_ISO2, Some(&mut buf), 0) > 0 {
                iso = sys_wide(&buf);
            }
            let mut tz = DYNAMIC_TIME_ZONE_INFORMATION::default();
            GetDynamicTimeZoneInformation(&mut tz);
            let tzname = sys_wide(&tz.TimeZoneKeyName);
            if let Some(sp) = TZ_SPOTS.iter().find(|s| s.0 == iso && s.1 == tzname) {
                lat = sp.2;
                lon = sp.3;
                have = true;
            } else if GetGeoInfoW(geo, GEO_LATITUDE, Some(&mut buf), 0) > 0 {
                lat = atof(&sys_wide(&buf));
                if GetGeoInfoW(geo, GEO_LONGITUDE, Some(&mut buf), 0) > 0 {
                    lon = atof(&sys_wide(&buf));
                    have = true;
                }
            }
        }
    }
    let fahrenheit = locale_info(LOCALE_IMEASURE).starts_with('1');
    if !have {
        w.failed = true;
        w.why = "Set your country in Windows Settings › Time & language › Language & region".into();
    } else {
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={:.2}&longitude={:.2}&current=temperature_2m,weather_code,is_day\
&hourly=temperature_2m,weather_code,is_day&forecast_hours=7&timezone=auto{}",
            lat,
            lon,
            if fahrenheit { "&temperature_unit=fahrenheit" } else { "" }
        );
        let mut buf = Vec::new();
        let ok = fonts::http_get(&url, Some(&mut buf), None, &mut |_| {});
        let js = String::from_utf8_lossy(&buf).into_owned();
        match js.find("\"current\":{") {
            Some(cur) if ok => {
                let end = js[cur..].find('}').map(|x| x + cur).unwrap_or(js.len());
                w.temp = json_num(&js, cur, "temperature_2m", end);
                w.code = json_num(&js, cur, "weather_code", end) as i32;
                w.day = json_num(&js, cur, "is_day", end) != 0.0;
                if let Some(hr) = js.find("\"hourly\":{") {
                    let times = json_array(&js, hr, "time");
                    let temps = json_array(&js, hr, "temperature_2m");
                    let codes = json_array(&js, hr, "weather_code");
                    let days = json_array(&js, hr, "is_day");
                    let n = times.len().min(temps.len()).min(codes.len()).min(days.len());
                    for i in 1..n {
                        let hour = times[i].find('T').map(|t| atof(&times[i][t + 1..t + 3.min(times[i].len() - t - 1) + 1]) as i32).unwrap_or(0);
                        w.hours.push(WeatherHour { hour, t: atof(&temps[i]), code: atof(&codes[i]) as i32, day: atof(&days[i]) != 0.0 });
                    }
                }
                w.ok = true;
            }
            _ => {
                w.failed = true;
                w.why = "Couldn't reach the weather service".into();
            }
        }
    }
    w.at = now();
    *WX.lock().unwrap() = Some(w);
    WX_LOADING.store(false, SeqCst);
    unsafe {
        let _ = PostMessageW(hwnd(), WM_APP_PANEL, WPARAM(0), LPARAM(0));
    }
}
// Refreshes when it's older than 20 minutes (or a minute after a failure).
pub fn weather_maybe_refresh(force: bool) {
    let w = weather_now();
    let age = now() - w.at;
    if !force && age < if w.failed { 60.0 } else { 20.0 * 60.0 } {
        return;
    }
    if WX_LOADING.swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(weather_fetch);
}
pub fn weather_now() -> WeatherState {
    WX.lock().unwrap().clone().unwrap_or_default()
}

const WI_SUN: i32 = 0;
const WI_MOON: i32 = 1;
const WI_PARTLY_DAY: i32 = 2;
const WI_PARTLY_NIGHT: i32 = 3;
const WI_CLOUD: i32 = 4;
const WI_FOG: i32 = 5;
const WI_DRIZZLE: i32 = 6;
const WI_RAIN: i32 = 7;
const WI_SNOW: i32 = 8;
const WI_THUNDER: i32 = 9;
pub fn weather_icon(code: i32, day: bool) -> i32 {
    match code {
        0 | 1 => {
            if day {
                WI_SUN
            } else {
                WI_MOON
            }
        }
        2 => {
            if day {
                WI_PARTLY_DAY
            } else {
                WI_PARTLY_NIGHT
            }
        }
        3 => WI_CLOUD,
        45 | 48 => WI_FOG,
        51..=57 => WI_DRIZZLE,
        61..=67 | 80..=82 => WI_RAIN,
        71..=77 | 85 | 86 => WI_SNOW,
        95.. => WI_THUNDER,
        _ => WI_CLOUD,
    }
}
pub fn weather_word(code: i32, day: bool) -> &'static str {
    match code {
        0 => {
            if day {
                "Sunny"
            } else {
                "Clear"
            }
        }
        1 => {
            if day {
                "Mostly sunny"
            } else {
                "Mostly clear"
            }
        }
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        56 | 57 => "Freezing drizzle",
        51..=55 => "Drizzle",
        66 | 67 => "Freezing rain",
        65 => "Heavy rain",
        61..=64 => "Rain",
        80..=82 => "Showers",
        85 | 86 => "Snow showers",
        75 => "Heavy snow",
        71..=77 => "Snow",
        95.. => "Thunderstorm",
        _ => "Cloudy",
    }
}
fn theme_is_light() -> bool {
    let t = st().th.text;
    0.299 * t.r + 0.587 * t.g + 0.114 * t.b < 0.5
}

// ---- weather icons, drawn and animated (s = size in DIPs) ---------------------------------
fn line(a: D2D_POINT_2F, b: D2D_POINT_2F, w: f32, col: D2D1_COLOR_F) {
    set_color(col);
    unsafe { rt().DrawLine(a, b, solid(), w, None::<&ID2D1StrokeStyle>) };
}
fn draw_sun(cx: f32, cy: f32, s: f32, t: f64) {
    let rot = (t * 0.35).rem_euclid(2.0 * std::f64::consts::PI) as f32;
    for i in 0..8 {
        let a = rot + i as f32 * PI / 4.0;
        let (r0, r1) = (s * 0.31, s * (0.42 + 0.02 * (t as f32 * 3.0 + i as f32).sin()));
        line(pt(cx + a.cos() * r0, cy + a.sin() * r0), pt(cx + a.cos() * r1, cy + a.sin() * r1), (s * 0.055).max(1.5), c(0xFFC83D, 1.0));
    }
    unsafe {
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: c(0xFFE27A, 1.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: c(0xFFB020, 1.0) }];
        if let Ok(coll) = rt().CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
            let p = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: pt(cx - s * 0.06, cy - s * 0.06),
                gradientOriginOffset: pt(0.0, 0.0),
                radiusX: s * 0.26,
                radiusY: s * 0.26,
            };
            if let Ok(rb) = rt().CreateRadialGradientBrush(&p, None, &coll) {
                rt().FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: s * 0.22, radiusY: s * 0.22 }, &rb);
            }
        }
    }
}
fn draw_moon(cx: f32, cy: f32, s: f32, t: f64, stars: bool) {
    unsafe {
        let d2d = &st().d2d;
        let a = d2d.CreateEllipseGeometry(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: s * 0.27, radiusY: s * 0.27 });
        let b = d2d.CreateEllipseGeometry(&D2D1_ELLIPSE { point: pt(cx + s * 0.14, cy - s * 0.10), radiusX: s * 0.23, radiusY: s * 0.23 });
        if let (Ok(a), Ok(b), Ok(p)) = (a, b, d2d.CreatePathGeometry()) {
            if let Ok(sink) = p.Open() {
                let _ = a.CombineWithGeometry(&b, D2D1_COMBINE_MODE_EXCLUDE, None, 0.25, &sink);
                let _ = sink.Close();
            }
            set_color(c(0xF2EBC8, 1.0));
            rt().FillGeometry(&p, solid(), None::<&ID2D1Brush>);
        }
    }
    if stars {
        for q in [[0.30f32, -0.30, 0.0], [0.38, 0.05, 1.7], [0.12, -0.42, 3.1]] {
            let tw = 0.5 + 0.5 * (t as f32 * 2.2 + q[2]).sin();
            fill_circle(cx + s * q[0], cy + s * q[1], s * (0.025 + 0.02 * tw), c(0xFFF6D8, 0.35 + 0.65 * tw));
        }
    }
}
fn draw_cloud(cx: f32, cy: f32, s: f32, col: D2D1_COLOR_F) {
    fill_circle(cx - s * 0.16, cy + s * 0.02, s * 0.17, col);
    fill_circle(cx + s * 0.05, cy - s * 0.07, s * 0.22, col);
    fill_circle(cx + s * 0.22, cy + s * 0.05, s * 0.14, col);
    fill_rr(rect(cx - s * 0.33, cy + s * 0.02, cx + s * 0.36, cy + s * 0.19), s * 0.085, col);
}
pub fn draw_weather_icon(kind: i32, cx: f32, cy: f32, s: f32, alpha: f32, t: f64) {
    if alpha <= 0.01 {
        return;
    }
    push_layer(alpha);
    let light = theme_is_light();
    let cloud = if light { c(0xB9C4D3, 1.0) } else { c(0xDDE4EF, 1.0) };
    let dark = if light { c(0x8C97A8, 1.0) } else { c(0x9AA4B5, 1.0) };
    let drift = (t as f32 * 0.9).sin() * s * 0.025;
    match kind {
        WI_SUN => draw_sun(cx, cy, s, t),
        WI_MOON => draw_moon(cx, cy, s, t, true),
        WI_PARTLY_DAY => {
            draw_sun(cx + s * 0.13, cy - s * 0.12, s * 0.8, t);
            draw_cloud(cx - s * 0.04 + drift, cy + s * 0.08, s * 0.9, cloud);
        }
        WI_PARTLY_NIGHT => {
            draw_moon(cx + s * 0.12, cy - s * 0.12, s * 0.8, t, false);
            draw_cloud(cx - s * 0.04 + drift, cy + s * 0.08, s * 0.9, cloud);
        }
        WI_CLOUD => {
            draw_cloud(cx + s * 0.12 - drift, cy - s * 0.10, s * 0.7, dark);
            draw_cloud(cx - s * 0.03 + drift, cy + s * 0.04, s * 0.95, cloud);
        }
        WI_FOG => {
            for i in 0..4 {
                let off = (t as f32 * 0.8 + i as f32 * 1.3).sin() * s * 0.06;
                let y = cy - s * 0.18 + i as f32 * s * 0.12;
                let w = s * if i % 2 == 1 { 0.30 } else { 0.36 };
                fill_rr(rect(cx - w + off, y - s * 0.03, cx + w + off, y + s * 0.03), s * 0.03, with_a(cloud, 0.55 + 0.12 * i as f32));
            }
        }
        WI_DRIZZLE | WI_RAIN | WI_THUNDER => {
            let drops = if kind == WI_DRIZZLE { 3 } else { 4 };
            for i in 0..drops {
                let ph = (t * if kind == WI_DRIZZLE { 0.9 } else { 1.4 } + i as f64 * 0.37).rem_euclid(1.0) as f32;
                let x = cx - s * 0.18 + i as f32 * s * (0.36 / (drops - 1) as f32) - ph * s * 0.05;
                let y = cy + s * 0.18 + ph * s * 0.28;
                let len = if kind == WI_DRIZZLE { s * 0.05 } else { s * 0.10 };
                line(pt(x, y), pt(x - len * 0.35, y + len), (s * 0.04).max(1.4), c(0x5AB0FF, 1.0 - ph));
            }
            if kind == WI_THUNDER {
                let fl = if (t as f32).rem_euclid(2.6) < 0.12 { 1.0 } else { 0.75 };
                unsafe {
                    if let Ok(bolt) = st().d2d.CreatePathGeometry() {
                        if let Ok(sk) = bolt.Open() {
                            sk.BeginFigure(pt(cx + s * 0.02, cy + s * 0.08), D2D1_FIGURE_BEGIN_FILLED);
                            for (px, py) in [(-0.10, 0.30), (0.01, 0.29), (-0.05, 0.46), (0.13, 0.22), (0.02, 0.23), (0.09, 0.08)] {
                                sk.AddLine(pt(cx + s * px, cy + s * py));
                            }
                            sk.EndFigure(D2D1_FIGURE_END_CLOSED);
                            let _ = sk.Close();
                        }
                        set_color(c(0xFFD43B, fl));
                        rt().FillGeometry(&bolt, solid(), None::<&ID2D1Brush>);
                    }
                }
            }
            draw_cloud(cx + drift, cy - s * 0.06, s, if kind == WI_THUNDER { dark } else { cloud });
        }
        WI_SNOW => {
            for i in 0..5 {
                let ph = (t * 0.45 + i as f64 * 0.29).rem_euclid(1.0) as f32;
                let x = cx - s * 0.22 + i as f32 * s * 0.11 + (t as f32 * 2.0 + i as f32).sin() * s * 0.03;
                let y = cy + s * 0.16 + ph * s * 0.30;
                let col = if light { c(0x9FB6D6, 1.0 - ph) } else { c(0xFFFFFF, 1.0 - ph) };
                fill_circle(x, y, s * 0.035, col);
            }
            draw_cloud(cx + drift, cy - s * 0.06, s, cloud);
        }
        _ => {}
    }
    unsafe { rt().PopLayer() };
}

pub fn deg(t: f64) -> String {
    format!("{}°", t.round() as i64)
}
fn hour_label(h: i32) -> String {
    if locale_info(LOCALE_ITIME).starts_with('1') {
        return format!("{:02}:00", h);
    }
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    format!("{} {}", h12, if h < 12 { "AM" } else { "PM" })
}
pub fn weather_height() -> f32 {
    168.0
}
pub fn draw_weather(top: f32, _dt: f32, alpha: f32, t: f64) {
    let s = st();
    let th = s.th;
    let w = weather_now();
    panel_header("Weather", if w.ok { "↵ refresh" } else { "" }, top, alpha);
    let y0 = top + 44.0;
    if !w.ok {
        let loading = WX_LOADING.load(Relaxed) || !w.failed;
        dl(if loading { "\u{E895}" } else { "\u{E7BA}" }, &s.fmt_big_c, MARGIN + 24.0, y0, 80.0, 80.0, with_a(th.dim, alpha * 0.8));
        dl(
            if loading { "Checking the weather…" } else { "No weather right now" },
            &s.fmt_mid,
            MARGIN + 120.0,
            y0 + 10.0,
            bar_w() - 150.0,
            30.0,
            with_a(th.text, alpha),
        );
        if !loading {
            dl(&w.why, &s.fmt_row, MARGIN + 120.0, y0 + 40.0, bar_w() - 150.0, 40.0, with_a(th.dim, alpha));
        }
        return;
    }
    draw_weather_icon(weather_icon(w.code, w.day), MARGIN + 70.0, y0 + 46.0, 96.0, alpha, t);
    let tx = MARGIN + 132.0;
    dl(&deg(w.temp), &s.fmt_big, tx, y0 + 2.0, 160.0, 52.0, with_a(th.text, alpha));
    dl(weather_word(w.code, w.day), &s.fmt_mid, tx, y0 + 52.0, 200.0, 26.0, with_a(th.text, alpha * 0.9));
    dl("\u{E707}", &s.fmt_glyph_sm, tx, y0 + 80.0, 14.0, 20.0, with_a(th.dim, alpha));
    dl(if w.country.is_empty() { "Your country" } else { &w.country }, &s.fmt_small, tx + 18.0, y0 + 80.0, 180.0, 20.0, with_a(th.dim, alpha));
    // the next hours
    let n = w.hours.len().min(6);
    let col_w = 50.0;
    let hx = MARGIN + bar_w() - 18.0 - n as f32 * col_w;
    for (i, h) in w.hours.iter().take(n).enumerate() {
        let x = hx + i as f32 * col_w;
        dl(&hour_label(h.hour), &s.fmt_small_c, x, y0 + 4.0, col_w, 18.0, with_a(th.dim, alpha));
        draw_weather_icon(weather_icon(h.code, h.day), x + col_w / 2.0, y0 + 46.0, 34.0, alpha, t + i as f64 * 0.7);
        dl(&deg(h.t), &s.fmt_small_c, x, y0 + 72.0, col_w, 18.0, with_a(th.text, alpha));
    }
}

// ======================================================================================
// This PC and network: sampled once a second
// ======================================================================================
#[derive(Clone, Default)]
pub struct SysState {
    cpu: Vec<f32>, // last 60 seconds; net in bytes / s
    ram: Vec<f32>,
    gpu: Vec<f32>,
    down: Vec<f32>,
    up: Vec<f32>,
    cpu_now: f32,
    ram_now: f32,
    gpu_now: f32,
    ram_used: f64,
    ram_total: f64,
    down_now: f64,
    up_now: f64,
    net: i32, // 0 offline, 1 Wi-Fi, 2 Ethernet
    battery: i32,
    charging: bool,
    disk_free: f64,
    disk_total: f64,
}
static SYS: Mutex<Option<SysState>> = Mutex::new(None);
static SYS_STARTED: AtomicBool = AtomicBool::new(false);

fn push_hist(v: &mut Vec<f32>, x: f32) {
    v.push(x);
    if v.len() > 60 {
        v.remove(0);
    }
}

fn sys_worker() {
    use windows::Win32::NetworkManagement::IpHelper::*;
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::System::Performance::*;
    use windows::Win32::System::Power::*;
    let (mut p_idle, mut p_kern, mut p_user, mut p_in, mut p_out) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut p_t = 0.0;
    let mut q: isize = 0;
    let mut gpu_c: isize = 0;
    let mut pdh_buf: Vec<u64> = Vec::new();
    unsafe {
        loop {
            let (mut fi, mut fk, mut fu) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
            let mut cpu = 0.0f32;
            if GetSystemTimes(Some(&mut fi), Some(&mut fk), Some(&mut fu)).is_ok() {
                let (i, k, u) = (ft_u(&fi), ft_u(&fk), ft_u(&fu));
                let total = (k - p_kern) + (u - p_user);
                let idle = i - p_idle;
                if p_kern != 0 && total != 0 {
                    cpu = total.saturating_sub(idle) as f32 / total as f32;
                }
                p_idle = i;
                p_kern = k;
                p_user = u;
            }
            let mut ms = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            let _ = GlobalMemoryStatusEx(&mut ms);
            let ram_total = ms.ullTotalPhys as f64;
            let ram_used = (ms.ullTotalPhys - ms.ullAvailPhys) as f64;
            let ram = if ram_total > 0.0 { (ram_used / ram_total) as f32 } else { 0.0 };

            // GPU: busiest engine type, as Task Manager shows it (only while the bar is open)
            let mut gpu = -1.0f32;
            let open = BAR_OPEN.load(Relaxed);
            if open {
                if q == 0
                    && PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == 0
                    && PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut gpu_c) != 0
                {
                    PdhCloseQuery(q);
                    q = 0;
                }
                if q != 0 && PdhCollectQueryData(q) == 0 {
                    let (mut size, mut count) = (0u32, 0u32);
                    if PdhGetFormattedCounterArrayW(gpu_c, PDH_FMT_DOUBLE, &mut size, &mut count, None) == PDH_MORE_DATA {
                        pdh_buf.resize(size as usize / 8 + 1, 0);
                        let items = pdh_buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                        if PdhGetFormattedCounterArrayW(gpu_c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) == 0 {
                            let mut by_type: HashMap<String, f64> = HashMap::new();
                            for i in 0..count as usize {
                                let it = &*items.add(i);
                                let n = it.szName.to_string().unwrap_or_default();
                                let Some(e) = n.find("engtype_") else { continue };
                                if it.FmtValue.CStatus != 0 {
                                    continue;
                                }
                                *by_type.entry(n[e..].to_string()).or_default() += it.FmtValue.Anonymous.doubleValue;
                            }
                            let mx = by_type.values().fold(0.0f64, |a, &b| a.max(b));
                            gpu = (mx / 100.0).min(1.0) as f32;
                        }
                    }
                }
            }

            // network: physical Wi-Fi / Ethernet adapters only
            let mut tab: *mut MIB_IF_TABLE2 = null_mut();
            let (mut in_b, mut out_b, mut net) = (0u64, 0u64, 0);
            if GetIfTable2(&mut tab).is_ok() && !tab.is_null() {
                let rows = std::slice::from_raw_parts((*tab).Table.as_ptr(), (*tab).NumEntries as usize);
                for r in rows {
                    if r.InterfaceAndOperStatusFlags._bitfield & 1 == 0 || r.OperStatus != IfOperStatusUp {
                        continue;
                    }
                    if r.Type != IF_TYPE_IEEE80211 && r.Type != IF_TYPE_ETHERNET_CSMACD {
                        continue;
                    }
                    in_b += r.InOctets;
                    out_b += r.OutOctets;
                    if r.Type == IF_TYPE_IEEE80211 {
                        net = 1;
                    } else if net == 0 {
                        net = 2;
                    }
                }
                FreeMibTable(tab as *const c_void);
            }
            let t = now();
            let (mut down, mut up) = (0.0, 0.0);
            if p_t > 0.0 && t > p_t {
                down = if in_b >= p_in { (in_b - p_in) as f64 / (t - p_t) } else { 0.0 };
                up = if out_b >= p_out { (out_b - p_out) as f64 / (t - p_t) } else { 0.0 };
            }
            p_in = in_b;
            p_out = out_b;
            p_t = t;

            let mut ps = SYSTEM_POWER_STATUS::default();
            let (mut battery, mut charging) = (-1, false);
            if GetSystemPowerStatus(&mut ps).is_ok() && ps.BatteryFlag & 128 == 0 && ps.BatteryLifePercent <= 100 {
                battery = ps.BatteryLifePercent as i32;
                charging = ps.ACLineStatus == 1;
            }
            let mut wd = [0u16; 260];
            let n = windows::Win32::System::SystemInformation::GetWindowsDirectoryW(Some(&mut wd));
            let drive = if n >= 3 { String::from_utf16_lossy(&wd[..3]) } else { "C:\\".into() };
            let (mut fa, mut tot) = (0u64, 0u64);
            let disk = windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&HSTRING::from(drive), Some(&mut fa), Some(&mut tot), None).is_ok();
            {
                let mut g = SYS.lock().unwrap();
                let s = g.get_or_insert_with(|| SysState { battery: -1, gpu_now: -1.0, ..Default::default() });
                s.cpu_now = cpu;
                s.ram_now = ram;
                s.ram_used = ram_used;
                s.ram_total = ram_total;
                s.gpu_now = gpu;
                s.down_now = down;
                s.up_now = up;
                s.net = net;
                s.battery = battery;
                s.charging = charging;
                if disk {
                    s.disk_free = fa as f64;
                    s.disk_total = tot as f64;
                }
                push_hist(&mut s.cpu, cpu);
                push_hist(&mut s.ram, ram);
                push_hist(&mut s.gpu, gpu.max(0.0));
                push_hist(&mut s.down, down as f32);
                push_hist(&mut s.up, up as f32);
            }
            if !BAR_OPEN.load(Relaxed) && q != 0 {
                PdhCloseQuery(q);
                q = 0;
            }
            if WaitForSingleObject(quit_event(), 1000) != WAIT_TIMEOUT {
                break;
            }
        }
        if q != 0 {
            PdhCloseQuery(q);
        }
    }
}
fn sys_start() {
    if SYS_STARTED.swap(true, SeqCst) {
        return;
    }
    std::thread::spawn(sys_worker);
}
fn sys_now() -> SysState {
    SYS.lock().unwrap().clone().unwrap_or(SysState { battery: -1, gpu_now: -1.0, ..Default::default() })
}
fn pctf(f: f32) -> String {
    format!("{}%", (f.clamp(0.0, 1.0) * 100.0).round() as i32)
}
fn speed(bps: f64) -> String {
    let bits = bps * 8.0;
    if bits >= 1e9 {
        format!("{:.2} Gbps", bits / 1e9)
    } else if bits >= 1e8 {
        format!("{:.0} Mbps", bits / 1e6)
    } else if bits >= 1e6 {
        format!("{:.1} Mbps", bits / 1e6)
    } else if bits >= 1e3 {
        format!("{:.0} Kbps", bits / 1e3)
    } else {
        format!("{:.0} bps", bits)
    }
}
fn uptime() -> String {
    let s = unsafe { GetTickCount64() } / 1000;
    let (d, h, m) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if d > 0 {
        format!("{}d {}h", d, h)
    } else if h > 0 {
        format!("{}h {}m", h, m)
    } else {
        format!("{} min", m)
    }
}

// A stat card: label, big value, small note, and a graph.
#[allow(clippy::too_many_arguments)]
fn stat_card(r: D2D_RECT_F, label: &str, value: &str, note: &str, hist: Option<&Vec<f32>>, col: D2D1_COLOR_F, alpha: f32, max_v: f32) {
    let s = st();
    let th = s.th;
    draw_card(r, alpha, 10.0);
    let (x, w) = (r.left + 14.0, r.right - r.left - 28.0);
    dl(label, &s.fmt_small, x, r.top + 8.0, w, 18.0, with_a(th.dim, alpha));
    dl(value, &s.fmt_mid, x, r.top + 24.0, w, 28.0, with_a(th.text, alpha));
    dl(note, &s.fmt_small_r, x, r.top + 8.0, w, 18.0, with_a(th.dim, alpha));
    if let Some(h) = hist {
        let gr = rect(r.left + 1.0, r.top + 54.0, r.right - 1.0, r.bottom - 1.0);
        unsafe { rt().PushAxisAlignedClip(&gr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let mut v = vec![0.0f32; 60usize.saturating_sub(h.len())];
        v.extend_from_slice(h);
        draw_graph(&v, rect(gr.left, gr.top + 2.0, gr.right, gr.bottom - 2.0), col, alpha, max_v);
        unsafe { rt().PopAxisAlignedClip() };
    }
}
pub fn system_height() -> f32 {
    44.0 + 2.0 * 96.0 + 10.0 + 14.0
}
pub fn system_detail() -> String {
    let s = sys_now();
    format!("CPU {}  ·  RAM {}", pctf(s.cpu_now), pctf(s.ram_now))
}
pub fn draw_system(top: f32, _dt: f32, alpha: f32, _t: f64) {
    let sy = sys_now();
    panel_header("This PC", &format!("Up {}", uptime()), top, alpha);
    let cw = (bar_w() - 2.0 * GRID_PAD - CARD_GAP) / 2.0;
    let ch = 96.0;
    let x0 = MARGIN + GRID_PAD;
    let x1 = x0 + cw + CARD_GAP;
    let y0 = top + 44.0;
    let y1 = y0 + ch + 10.0;
    let ram = format!("{:.1} / {:.0} GB", sy.ram_used / 1073741824.0, (sy.ram_total / 1073741824.0).round());
    stat_card(rect(x0, y0, x0 + cw, y0 + ch), "Processor", &pctf(sy.cpu_now), "", Some(&sy.cpu), c(0x60CDFF, 1.0), alpha, 1.0);
    stat_card(rect(x1, y0, x1 + cw, y0 + ch), "Memory", &pctf(sy.ram_now), &ram, Some(&sy.ram), c(0xB48CFF, 1.0), alpha, 1.0);
    let g = if sy.gpu_now < 0.0 { "…".to_string() } else { pctf(sy.gpu_now) };
    stat_card(rect(x0, y1, x0 + cw, y1 + ch), "Graphics", &g, "", Some(&sy.gpu), c(0x5EE6A8, 1.0), alpha, 1.0);
    // storage + battery
    let s = st();
    let th = s.th;
    let r = rect(x1, y1, x1 + cw, y1 + ch);
    draw_card(r, alpha, 10.0);
    let (x, w) = (r.left + 14.0, cw - 28.0);
    dl("Storage", &s.fmt_small, x, r.top + 8.0, w, 18.0, with_a(th.dim, alpha));
    dl(&format!("{} free", format_bytes(sy.disk_free)), &s.fmt_mid, x, r.top + 24.0, w, 28.0, with_a(th.text, alpha));
    let used = if sy.disk_total > 0.0 { (1.0 - sy.disk_free / sy.disk_total) as f32 } else { 0.0 };
    fill_rr(rect(x, r.top + 58.0, x + w, r.top + 64.0), 3.0, with_a(th.dim, alpha * 0.25));
    fill_rr(rect(x, r.top + 58.0, x + w * used, r.top + 64.0), 3.0, with_a(if used > 0.9 { c(0xFF6B6B, 1.0) } else { c(0xFFB648, 1.0) }, alpha));
    let bat = if sy.battery < 0 {
        "Plugged in, no battery".to_string()
    } else {
        format!("Battery {}%{}", sy.battery, if sy.charging { " · charging" } else { "" })
    };
    dl(&bat, &s.fmt_small, x, r.top + 70.0, w, 18.0, with_a(th.dim, alpha));
}

fn net_kind(n: i32) -> &'static str {
    match n {
        1 => "Wi‑Fi",
        2 => "Ethernet",
        _ => "Offline",
    }
}
pub fn network_height() -> f32 {
    44.0 + 112.0 + 14.0
}
pub fn network_detail() -> String {
    net_kind(sys_now().net).to_string()
}
pub fn draw_network(top: f32, _dt: f32, alpha: f32, _t: f64) {
    let sy = sys_now();
    panel_header("Network", &format!("{}{}", net_kind(sy.net), if sy.net != 0 { " · connected" } else { "" }), top, alpha);
    let cw = (bar_w() - 2.0 * GRID_PAD - CARD_GAP) / 2.0;
    let ch = 112.0;
    let x0 = MARGIN + GRID_PAD;
    let x1 = x0 + cw + CARD_GAP;
    let y0 = top + 44.0;
    let mut mx = 125000.0f32; // 1 Mbps
    for &v in sy.down.iter().chain(sy.up.iter()) {
        mx = mx.max(v);
    }
    stat_card(rect(x0, y0, x0 + cw, y0 + ch), "↓  Download", &speed(sy.down_now), "", Some(&sy.down), c(0x60CDFF, 1.0), alpha, mx * 1.15);
    stat_card(rect(x1, y0, x1 + cw, y0 + ch), "↑  Upload", &speed(sy.up_now), "", Some(&sy.up), c(0xFF9E64, 1.0), alpha, mx * 1.15);
}

// ======================================================================================
// Clock and calendar
// ======================================================================================
static mut CAL_MONTH_OFF: i32 = 0;
pub fn time_text(t: &SYSTEMTIME, seconds: bool) -> String {
    let mut b = [0u16; 64];
    unsafe { GetTimeFormatEx(PCWSTR::null(), if seconds { TIME_FORMAT_FLAGS(0) } else { TIME_NOSECONDS }, Some(t), PCWSTR::null(), Some(&mut b)) };
    sys_wide(&b)
}
pub fn local_time() -> SYSTEMTIME {
    unsafe { GetLocalTime() }
}
pub fn calendar_height() -> f32 {
    44.0 + 7.0 * 28.0 + 14.0
}
pub fn calendar_enter(_: i32, _: bool) {
    unsafe { CAL_MONTH_OFF = 0 };
}
pub fn calendar_leave() {
    unsafe { CAL_MONTH_OFF = 0 };
}
pub fn calendar_detail() -> String {
    time_text(&local_time(), false)
}
pub fn draw_calendar(top: f32, _dt: f32, alpha: f32, _t: f64) {
    let s = st();
    let th = s.th;
    pbtn().clear();
    let now_st = local_time();
    let off = unsafe { CAL_MONTH_OFF };
    panel_header("Clock", "← → month   ↵ today", top, alpha);
    let y0 = top + 44.0;
    // time, big, with the seconds ticking beside it
    let t = time_text(&now_st, false);
    dl(&t, &s.fmt_big, MARGIN + 24.0, y0 + 6.0, 260.0, 54.0, with_a(th.text, alpha));
    let tw = text_width(&t, &s.fmt_big);
    dl(&format!(":{:02}", now_st.wSecond), &s.fmt_mid, MARGIN + 26.0 + tw, y0 + 24.0, 60.0, 30.0, with_a(th.dim, alpha));
    let mut date = [0u16; 128];
    unsafe { GetDateFormatEx(PCWSTR::null(), DATE_LONGDATE, Some(&now_st), PCWSTR::null(), Some(&mut date), PCWSTR::null()) };
    dl(&sys_wide(&date), &s.fmt_row, MARGIN + 26.0, y0 + 62.0, 280.0, 22.0, with_a(th.text, alpha * 0.85));
    // day of the year
    let mut jan = now_st;
    jan.wMonth = 1;
    jan.wDay = 1;
    let (mut a, mut b) = (FILETIME::default(), FILETIME::default());
    unsafe {
        let _ = SystemTimeToFileTime(&jan, &mut a);
        let _ = SystemTimeToFileTime(&now_st, &mut b);
    }
    let doy = (ft_u(&b).saturating_sub(ft_u(&a)) / 864000000000) + 1;
    dl(&format!("Day {} of the year", doy), &s.fmt_small, MARGIN + 26.0, y0 + 86.0, 280.0, 18.0, with_a(th.dim, alpha));
    // a little analog dial under it
    let (dcx, dcy, dr) = (MARGIN + 80.0, y0 + 160.0, 36.0);
    fill_circle(dcx, dcy, dr, with_a(th.highlight, alpha * 1.4));
    for i in 0..12 {
        let an = i as f32 * PI / 6.0;
        fill_circle(dcx + an.sin() * (dr - 6.0), dcy - an.cos() * (dr - 6.0), 1.3, with_a(th.dim, alpha * 0.6));
    }
    let secs = now_st.wSecond as f64 + now_st.wMilliseconds as f64 / 1000.0;
    let ah = ((now_st.wHour % 12) as f64 + now_st.wMinute as f64 / 60.0) as f32 * PI / 6.0;
    let am = (now_st.wMinute as f64 + secs / 60.0) as f32 * PI / 30.0;
    let as_ = secs as f32 * PI / 30.0;
    let round = unsafe {
        let p = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() };
        ID2D1Factory::CreateStrokeStyle(&s.d2d, &p, None).ok()
    };
    let hand = |an: f32, len: f32, wdt: f32, col: D2D1_COLOR_F| {
        set_color(col);
        unsafe { rt().DrawLine(pt(dcx, dcy), pt(dcx + an.sin() * len, dcy - an.cos() * len), solid(), wdt, round.as_ref()) };
    };
    hand(ah, dr * 0.5, 3.0, with_a(th.text, alpha));
    hand(am, dr * 0.75, 2.0, with_a(th.text, alpha));
    hand(as_, dr * 0.82, 1.0, with_a(th.accent, alpha));
    fill_circle(dcx, dcy, 2.5, with_a(th.accent, alpha));

    // the month
    let mo0 = now_st.wMonth as i32 - 1 + off;
    let y = now_st.wYear as i32 + mo0.div_euclid(12);
    let mo = mo0.rem_euclid(12);
    let mut first = SYSTEMTIME { wYear: y as u16, wMonth: (mo + 1) as u16, wDay: 1, ..Default::default() };
    let mut ft = FILETIME::default();
    unsafe {
        let _ = SystemTimeToFileTime(&first, &mut ft);
        let _ = FileTimeToSystemTime(&ft, &mut first); // fills in the day of the week
    }
    const MDAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = MDAYS[mo as usize] + (mo == 1 && leap) as i32;
    let first_dow = (locale_info(LOCALE_IFIRSTDAYOFWEEK).trim().parse::<i32>().unwrap_or(6) + 1) % 7; // locale: 0 = Monday
    let lead = (first.wDayOfWeek as i32 - first_dow + 7) % 7;
    let (cell_w, cell_h) = (38.0, 28.0);
    let gx = MARGIN + bar_w() - 22.0 - 7.0 * cell_w;
    let gy = y0;
    let mut mname = [0u16; 64];
    unsafe { GetDateFormatEx(PCWSTR::null(), DATE_YEARMONTH, Some(&first), PCWSTR::null(), Some(&mut mname), PCWSTR::null()) };
    dl("\u{E76B}", &s.fmt_glyph_sm, gx, gy, 24.0, cell_h, with_a(th.dim, alpha));
    dl(&sys_wide(&mname), &s.fmt_row, gx + 24.0, gy, 7.0 * cell_w - 48.0, cell_h, with_a(th.text, alpha));
    dl("\u{E76C}", &s.fmt_glyph_sm, gx + 7.0 * cell_w - 24.0, gy, 24.0, cell_h, with_a(th.dim, alpha));
    pbtn().push((rect(gx, gy, gx + 24.0, gy + cell_h), -1));
    pbtn().push((rect(gx + 7.0 * cell_w - 24.0, gy, gx + 7.0 * cell_w, gy + cell_h), 1));
    for d in 0..7 {
        let lc_day = ((first_dow + d + 6) % 7) as u32; // LOCALE_SSHORTESTDAYNAME1 = Monday
        let dn = locale_info(LOCALE_SSHORTESTDAYNAME1 + lc_day);
        dl(&dn, &s.fmt_small_c, gx + d as f32 * cell_w, gy + cell_h, cell_w, cell_h - 6.0, with_a(th.dim, alpha));
    }
    for dday in 1..=days {
        let idx = lead + dday - 1;
        let (row, col) = (idx / 7, idx % 7);
        let cx = gx + col as f32 * cell_w;
        let cy = gy + cell_h * 2.0 - 4.0 + row as f32 * cell_h;
        let today = off == 0 && dday == now_st.wDay as i32;
        if today {
            fill_circle(cx + cell_w / 2.0, cy + cell_h / 2.0, 12.0, with_a(th.accent, alpha));
        }
        dl(
            &dday.to_string(),
            &s.fmt_small_c,
            cx,
            cy,
            cell_w,
            cell_h,
            with_a(if today { c(0xFFFFFF, 1.0) } else { th.text }, alpha * if today { 1.0 } else { 0.85 }),
        );
    }
}
pub fn calendar_key(vk: VIRTUAL_KEY, shift: bool) -> bool {
    if shift {
        return false;
    }
    let d = match vk {
        VK_LEFT => -1,
        VK_RIGHT => 1,
        VK_UP => -12,
        VK_DOWN => 12,
        _ => return false,
    };
    unsafe { CAL_MONTH_OFF += d };
    true
}
pub fn calendar_click(x: f32, y: f32) -> bool {
    let b = button_at(x, y);
    if b == -1 || b == 1 {
        unsafe { CAL_MONTH_OFF += b };
    }
    true
}

// ======================================================================================
// Timer and stopwatch
// ======================================================================================
#[derive(Default)]
struct Tm {
    set: bool,
    run: bool,
    end: f64,
    left: f64,
    dur: f64,
    arg: String, // what started it ("5m")
    sw_run: bool,
    sw_start: f64,
    sw_accum: f64,
    sw_laps: Vec<f64>,
}
static mut TM: Option<Tm> = None;
fn tm() -> &'static mut Tm {
    unsafe { (*std::ptr::addr_of_mut!(TM)).get_or_insert_with(Tm::default) }
}

// "90s", "5m", "1h30m", "5:00", "1:30:00", "2.5" (minutes)
pub fn parse_duration(s0: &str) -> f64 {
    let s = s0.trim().to_lowercase();
    if s.is_empty() {
        return -1.0;
    }
    if s.contains(':') {
        let mut total = 0.0;
        let mut parts = 0;
        for n in s.split(':') {
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
                return -1.0;
            }
            total = total * 60.0 + n.parse::<f64>().unwrap_or(0.0);
            parts += 1;
        }
        return if parts <= 3 && total > 0.0 { total } else { -1.0 };
    }
    let c: Vec<char> = s.chars().collect();
    let (mut total, mut i, mut any) = (0.0, 0usize, false);
    while i < c.len() {
        while i < c.len() && c[i] == ' ' {
            i += 1;
        }
        let st0 = i;
        while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
            i += 1;
        }
        if st0 == i {
            return -1.0;
        }
        let v: f64 = c[st0..i].iter().collect::<String>().parse().unwrap_or(0.0);
        while i < c.len() && c[i] == ' ' {
            i += 1;
        }
        let us = i;
        while i < c.len() && c[i].is_alphabetic() {
            i += 1;
        }
        let u: String = c[us..i].iter().collect();
        match u.as_str() {
            "" | "m" | "min" | "mins" | "minute" | "minutes" => total += v * 60.0,
            "s" | "sec" | "secs" | "second" | "seconds" => total += v,
            "h" | "hr" | "hrs" | "hour" | "hours" => total += v * 3600.0,
            _ => return -1.0,
        }
        any = true;
    }
    if any && total > 0.0 {
        total
    } else {
        -1.0
    }
}
fn timer_left() -> f64 {
    let t = tm();
    if !t.set {
        0.0
    } else if t.run {
        (t.end - now()).max(0.0)
    } else {
        t.left
    }
}
fn timer_start(secs: f64, arg: &str) {
    let t = tm();
    t.set = true;
    t.run = true;
    t.dur = secs;
    t.end = now() + secs;
    t.arg = arg.to_lowercase();
    unsafe { SetTimer(hwnd(), TIMER_TICK, 250, None) };
}
fn timer_toggle() {
    let t = tm();
    if !t.set {
        return;
    }
    if t.run {
        t.left = (t.end - now()).max(0.0);
        t.run = false;
    } else {
        t.end = now() + t.left;
        t.run = true;
        unsafe { SetTimer(hwnd(), TIMER_TICK, 250, None) };
    }
}
fn timer_reset() {
    let t = tm();
    t.set = false;
    t.run = false;
    t.arg.clear();
}
// From WM_TIMER while a timer runs.
pub fn timer_tick() {
    let t = tm();
    if !t.set || !t.run {
        unsafe {
            let _ = KillTimer(hwnd(), TIMER_TICK);
        }
        return;
    }
    if now() < t.end {
        return;
    }
    let what = clock(t.dur);
    timer_reset();
    unsafe {
        let _ = KillTimer(hwnd(), TIMER_TICK);
    }
    custom::play_chime();
    custom::tray_balloon("Time's up", &format!("{} timer finished", what));
    if st().visible {
        flash("Time's up!");
    }
}
fn draw_ring(cx: f32, cy: f32, r: f32, frac: f32, width: f32, track: D2D1_COLOR_F, col: D2D1_COLOR_F) {
    set_color(track);
    unsafe { rt().DrawEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r }, solid(), width, None::<&ID2D1StrokeStyle>) };
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.001 {
        return;
    }
    unsafe {
        let d2d = &st().d2d;
        let Ok(p) = d2d.CreatePathGeometry() else { return };
        if let Ok(s) = p.Open() {
            let a1 = -PI / 2.0 + frac * 2.0 * PI;
            s.BeginFigure(pt(cx, cy - r), D2D1_FIGURE_BEGIN_HOLLOW);
            let arc = |to: D2D_POINT_2F, large: bool| D2D1_ARC_SEGMENT {
                point: to,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            };
            if frac >= 0.999 {
                s.AddArc(&arc(pt(cx, cy + r), false));
                s.AddArc(&arc(pt(cx, cy - r), false));
            } else {
                s.AddArc(&arc(pt(cx + a1.cos() * r, cy + a1.sin() * r), frac > 0.5));
            }
            s.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = s.Close();
        }
        let sp = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() };
        let round = ID2D1Factory::CreateStrokeStyle(d2d, &sp, None).ok();
        set_color(col);
        rt().DrawGeometry(&p, solid(), width, round.as_ref());
    }
}
pub fn timer_height() -> f32 {
    172.0
}
pub fn timer_detail() -> String {
    let t = tm();
    if t.set {
        format!("{}{}", clock(timer_left().ceil()), if t.run { " left" } else { " paused" })
    } else {
        String::new()
    }
}
pub fn draw_timer(top: f32, _dt: f32, alpha: f32, _t: f64) {
    let s = st();
    let th = s.th;
    let arg_s = ps().arg.clone();
    let t = tm();
    let arg = parse_duration(&arg_s);
    let fresh = arg > 0.0 && (!t.set || arg_s.to_lowercase() != t.arg);
    let hints = if fresh {
        "↵ start"
    } else if t.set {
        if t.run {
            "↵ pause   Del reset"
        } else {
            "↵ resume   Del reset"
        }
    } else {
        ""
    };
    panel_header("Timer", hints, top, alpha);
    let (cx, cy, r) = (MARGIN + 86.0, top + 98.0, 50.0);
    let left = if fresh { arg } else { timer_left() };
    let frac = if fresh {
        1.0
    } else if t.set {
        (left / t.dur.max(1.0)) as f32
    } else {
        0.0
    };
    draw_ring(cx, cy, r, frac, 6.0, with_a(th.dim, alpha * 0.2), with_a(if t.run || fresh { th.accent } else { th.dim }, alpha));
    dl(if t.run { "\u{E916}" } else { "\u{E768}" }, &s.fmt_glyph, cx - 20.0, cy - 20.0, 40.0, 40.0, with_a(th.dim, alpha));
    let tx = MARGIN + 170.0;
    if !t.set && !fresh {
        dl("0:00", &s.fmt_big, tx, top + 46.0, 300.0, 54.0, with_a(th.dim, alpha));
        dl("Type a time after > timer, like 5m, 90s, 1h30m or 10:00", &s.fmt_row, tx, top + 104.0, bar_w() - 190.0, 22.0, with_a(th.dim, alpha));
        return;
    }
    dl(&clock(left.ceil()), &s.fmt_big, tx, top + 46.0, 300.0, 54.0, with_a(th.text, alpha));
    let note = if fresh {
        "Press Enter to start".to_string()
    } else if t.run {
        let mut lt = local_time();
        let mut ft = FILETIME::default();
        unsafe {
            let _ = SystemTimeToFileTime(&lt, &mut ft);
            let u = ft_u(&ft) + (left * 1e7) as u64;
            ft = FILETIME { dwLowDateTime: u as u32, dwHighDateTime: (u >> 32) as u32 };
            let _ = FileTimeToSystemTime(&ft, &mut lt);
        }
        format!("Ends at {} · you'll hear a chime", time_text(&lt, false))
    } else {
        "Paused".to_string()
    };
    dl(&note, &s.fmt_row, tx, top + 104.0, bar_w() - 190.0, 22.0, with_a(th.dim, alpha));
}
pub fn timer_enter(_: i32, _: bool) {
    let a = ps().arg.clone();
    let arg = parse_duration(&a);
    let t = tm();
    if arg > 0.0 && (!t.set || a.to_lowercase() != t.arg) {
        timer_start(arg, &a);
    } else {
        timer_toggle();
    }
}
pub fn timer_key(vk: VIRTUAL_KEY, _: bool) -> bool {
    let s = st();
    if vk == VK_DELETE && s.caret == s.text.len() && tm().set {
        timer_reset();
        return true;
    }
    false
}

fn stopwatch_now() -> f64 {
    let t = tm();
    t.sw_accum + if t.sw_run { now() - t.sw_start } else { 0.0 }
}
fn sw_text(t: f64, cs: bool) -> String {
    let ms = (t * 100.0) as i64;
    if t >= 3600.0 {
        format!("{}:{:02}:{:02}", ms / 360000, ms / 6000 % 60, ms / 100 % 60)
    } else if cs {
        format!("{:02}:{:02}.{:02}", ms / 6000, ms / 100 % 60, ms % 100)
    } else {
        format!("{:02}:{:02}", ms / 6000, ms / 100 % 60)
    }
}
pub fn stopwatch_height() -> f32 {
    172.0
}
pub fn stopwatch_detail() -> String {
    let t = stopwatch_now();
    if t > 0.0 {
        sw_text(t, false)
    } else {
        String::new()
    }
}
pub fn draw_stopwatch(top: f32, _dt: f32, alpha: f32, _t: f64) {
    let s = st();
    let th = s.th;
    let run = tm().sw_run;
    panel_header("Stopwatch", if run { "↵ pause   ⇧↵ lap   Del reset" } else { "↵ start   Del reset" }, top, alpha);
    let t = stopwatch_now();
    let (cx, cy, r) = (MARGIN + 86.0, top + 98.0, 50.0);
    draw_ring(cx, cy, r, (t % 60.0) as f32 / 60.0, 6.0, with_a(th.dim, alpha * 0.2), with_a(if run { th.accent } else { th.dim }, alpha));
    dl(if run { "\u{E916}" } else { "\u{E768}" }, &s.fmt_glyph, cx - 20.0, cy - 20.0, 40.0, 40.0, with_a(th.dim, alpha));
    dl(&sw_text(t, true), &s.fmt_big, MARGIN + 170.0, top + 46.0, 260.0, 54.0, with_a(if t > 0.0 { th.text } else { th.dim }, alpha));
    let st_text = if run {
        "Running"
    } else if t > 0.0 {
        "Paused"
    } else {
        "Press Enter to start"
    };
    dl(st_text, &s.fmt_row, MARGIN + 170.0, top + 104.0, 220.0, 22.0, with_a(th.dim, alpha));
    // laps, newest first
    let lx = MARGIN + bar_w() - 190.0;
    let laps = &tm().sw_laps;
    let n = laps.len();
    for i in 0..n.min(4) {
        let k = n - 1 - i;
        let lap = laps[k] - if k > 0 { laps[k - 1] } else { 0.0 };
        let ly = top + 46.0 + i as f32 * 26.0;
        dl(&format!("Lap {}", k + 1), &s.fmt_small, lx, ly, 60.0, 22.0, with_a(th.dim, alpha));
        dl(&sw_text(lap, true), &s.fmt_row_right, lx + 50.0, ly, 120.0, 22.0, with_a(th.text, alpha * if i > 0 { 0.75 } else { 1.0 }));
    }
}
pub fn stopwatch_enter(_: i32, shift: bool) {
    let n = stopwatch_now();
    let t = tm();
    if shift {
        if t.sw_run {
            t.sw_laps.push(n);
        }
        return;
    }
    if t.sw_run {
        t.sw_accum = n;
        t.sw_run = false;
    } else {
        t.sw_start = now();
        t.sw_run = true;
    }
}
pub fn stopwatch_key(vk: VIRTUAL_KEY, _: bool) -> bool {
    let s = st();
    if vk == VK_DELETE && s.caret == s.text.len() {
        let t = tm();
        t.sw_run = false;
        t.sw_accum = 0.0;
        t.sw_laps.clear();
        return true;
    }
    false
}

// ======================================================================================
// Volume and brightness
// ======================================================================================
fn endpoint_volume() -> Option<IAudioEndpointVolume> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        dev.Activate(CLSCTX_ALL, None).ok()
    }
}
static mut VOL_SHOWN: i32 = -1; // cached for drawing, refreshed by the panel
static mut MUTE_SHOWN: bool = false;
static mut VOL_READ_AT: f64 = -10.0;
pub fn volume_read(force: bool) {
    unsafe {
        if !force && now() - VOL_READ_AT < 0.5 {
            return;
        }
        VOL_READ_AT = now();
        match endpoint_volume() {
            Some(v) => {
                VOL_SHOWN = (v.GetMasterVolumeLevelScalar().unwrap_or(0.0) * 100.0).round() as i32;
                MUTE_SHOWN = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            }
            None => VOL_SHOWN = -1,
        }
    }
}
fn volume_set(pct: i32) {
    let pct = pct.clamp(0, 100);
    if let Some(v) = endpoint_volume() {
        unsafe {
            let _ = v.SetMasterVolumeLevelScalar(pct as f32 / 100.0, std::ptr::null());
            if pct > 0 {
                let _ = v.SetMute(FALSE, std::ptr::null());
            }
        }
    }
    volume_read(true);
}
fn volume_mute() {
    if let Some(v) = endpoint_volume() {
        unsafe {
            let m = v.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            let _ = v.SetMute(BOOL::from(!m), std::ptr::null());
        }
    }
    volume_read(true);
}
pub fn volume_detail() -> String {
    volume_read(false);
    unsafe {
        if VOL_SHOWN < 0 {
            String::new()
        } else if MUTE_SHOWN {
            "Muted".into()
        } else {
            format!("{}%", VOL_SHOWN)
        }
    }
}

// Brightness goes through WMI (built-in screens only), on its own thread.
static BRIGHT: AtomicI32 = AtomicI32::new(-2); // -2 not read yet, -1 can't be changed
static BRIGHT_WANT: AtomicI32 = AtomicI32::new(-1); // a change waiting to be sent
static BRIGHT_BUSY: AtomicBool = AtomicBool::new(false);
fn brightness_worker() {
    use windows::core::BSTR;
    use windows::Win32::System::Rpc::*;
    use windows::Win32::System::Wmi::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let go = || -> windows::core::Result<()> {
            let loc: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let svc = loc.ConnectServer(&BSTR::from("ROOT\\WMI"), &BSTR::new(), &BSTR::new(), &BSTR::new(), 0, &BSTR::new(), None::<&IWbemContext>)?;
            let _ = CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE);
            let wql = BSTR::from("WQL");
            let first = |q: &str| -> Option<IWbemClassObject> {
                let en = svc.ExecQuery(&wql, &BSTR::from(q), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None::<&IWbemContext>).ok()?;
                let mut objs = [None];
                let mut got = 0u32;
                let _ = en.Next(3000, &mut objs, &mut got);
                if got > 0 {
                    objs[0].take()
                } else {
                    None
                }
            };
            loop {
                let want = BRIGHT_WANT.swap(-1, SeqCst);
                if want >= 0 {
                    if let Some(inst) = first("SELECT * FROM WmiMonitorBrightnessMethods WHERE Active=TRUE") {
                        let _ = (|| -> windows::core::Result<()> {
                            let mut path = VARIANT::default();
                            inst.Get(w!("__PATH"), 0, &mut path, None, None)?;
                            let mut cls: Option<IWbemClassObject> = None;
                            svc.GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None::<&IWbemContext>, Some(&mut cls), None)?;
                            let cls = cls.ok_or(windows::core::Error::from(E_FAIL))?;
                            let mut sig: Option<IWbemClassObject> = None;
                            let mut out_sig: Option<IWbemClassObject> = None;
                            cls.GetMethod(w!("WmiSetBrightness"), 0, &mut sig, &mut out_sig)?;
                            let input = sig.ok_or(windows::core::Error::from(E_FAIL))?.SpawnInstance(0)?;
                            input.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0)?;
                            input.Put(w!("Brightness"), 0, &VARIANT::from(want.clamp(0, 100) as u8), 0)?;
                            let p = BSTR::try_from(&path)?;
                            svc.ExecMethod(&p, &BSTR::from("WmiSetBrightness"), WBEM_GENERIC_FLAG_TYPE(0), None::<&IWbemContext>, &input, None, None)?;
                            Ok(())
                        })();
                    }
                }
                match first("SELECT CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE") {
                    Some(o) => {
                        let mut v = VARIANT::default();
                        if o.Get(w!("CurrentBrightness"), 0, &mut v, None, None).is_ok() {
                            BRIGHT.store(i32::try_from(&v).unwrap_or(-1), SeqCst);
                        }
                    }
                    None => BRIGHT.store(-1, SeqCst),
                }
                let _ = PostMessageW(hwnd(), WM_APP_PANEL, WPARAM(0), LPARAM(0));
                if BRIGHT_WANT.load(SeqCst) < 0 {
                    break;
                }
            }
            Ok(())
        };
        if go().is_err() {
            BRIGHT.store(-1, SeqCst);
        }
        BRIGHT_BUSY.store(false, SeqCst);
        CoUninitialize();
    }
    if BRIGHT_WANT.load(SeqCst) >= 0 && !BRIGHT_BUSY.swap(true, SeqCst) {
        std::thread::spawn(brightness_worker);
    }
}
pub fn brightness_kick() {
    if !BRIGHT_BUSY.swap(true, SeqCst) {
        std::thread::spawn(brightness_worker);
    }
}
fn brightness_set(pct: i32) {
    let pct = pct.clamp(0, 100);
    BRIGHT_WANT.store(pct, SeqCst);
    if BRIGHT.load(SeqCst) >= 0 {
        BRIGHT.store(pct, SeqCst); // shown at once
    }
    brightness_kick();
}
pub fn brightness_detail() -> String {
    let b = BRIGHT.load(Relaxed);
    if b >= 0 {
        format!("{}%", b)
    } else {
        String::new()
    }
}

// The shared slider panel. which: 0 volume, 1 brightness.
pub fn slider_height() -> f32 {
    128.0
}
static mut SLIDER_TRACK: D2D_RECT_F = D2D_RECT_F { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
fn digits_pct(a: &str) -> Option<i32> {
    let a = a.strip_suffix('%').unwrap_or(a);
    if !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()) {
        Some(a.parse::<i64>().unwrap_or(100).min(100) as i32)
    } else {
        None
    }
}
pub fn draw_slider(which: i32, top: f32, alpha: f32) {
    let s = st();
    let th = s.th;
    let v = if which == 0 { unsafe { VOL_SHOWN } } else { BRIGHT.load(Relaxed) };
    let muted = which == 0 && unsafe { MUTE_SHOWN };
    let arg = ps().arg.clone();
    let a = if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_digit() || c == '%') { digits_pct(&arg) } else { None };
    let hints = match a {
        Some(p) => format!("↵ set {}%", p),
        None if which == 0 => "← → adjust   ↵ mute".into(),
        None => "← → adjust".into(),
    };
    panel_header(if which == 0 { "Volume" } else { "Brightness" }, &hints, top, alpha);
    let y0 = top + 44.0;
    if v < 0 {
        let msg = if which == 0 {
            "No speakers or headphones found"
        } else if v == -2 {
            "Checking the screen…"
        } else {
            "This screen's brightness can't be changed here; external monitors use their own buttons"
        };
        dl(if which == 0 { "\u{E74F}" } else { "\u{E706}" }, &s.fmt_big_c, MARGIN + 14.0, y0, 70.0, 64.0, with_a(th.dim, alpha));
        if let Some(l) = make_layout(&ws(msg), &s.fmt_row, bar_w() - 110.0, 48.0) {
            unsafe {
                let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                set_color(with_a(th.dim, alpha));
                rt().DrawTextLayout(pt(MARGIN + 90.0, y0 + 8.0), &l, solid(), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
        unsafe { SLIDER_TRACK = rect(0.0, 0.0, 0.0, 0.0) };
        return;
    }
    let glyph = if which == 1 {
        "\u{E706}"
    } else if muted || v == 0 {
        "\u{E74F}"
    } else if v < 33 {
        "\u{E993}"
    } else if v < 66 {
        "\u{E994}"
    } else {
        "\u{E995}"
    };
    dl(glyph, &s.fmt_big_c, MARGIN + 14.0, y0, 70.0, 64.0, with_a(if muted { th.dim } else { th.text }, alpha));
    let (tx, tw, ty) = (MARGIN + 92.0, bar_w() - 92.0 - 116.0, y0 + 30.0);
    unsafe { SLIDER_TRACK = rect(tx, ty - 12.0, tx + tw, ty + 12.0) };
    let f = v as f32 / 100.0;
    fill_rr(rect(tx, ty - 3.0, tx + tw, ty + 3.0), 3.0, with_a(th.dim, alpha * 0.25));
    let col = with_a(if muted { th.dim } else { th.accent }, alpha);
    fill_rr(rect(tx, ty - 3.0, tx + tw * f, ty + 3.0), 3.0, col);
    fill_circle(tx + tw * f, ty, 9.0, col);
    fill_circle(tx + tw * f, ty, 4.0, with_a(c(0xFFFFFF, 1.0), alpha));
    let label = if muted { "Muted".to_string() } else { format!("{}%", v) };
    dl(&label, &s.fmt_mid, MARGIN + bar_w() - 86.0, y0 + 14.0, 70.0, 32.0, with_a(th.text, alpha));
}
pub fn slider_click(which: i32, x: f32, y: f32) -> bool {
    let r = unsafe { SLIDER_TRACK };
    if r.right <= r.left || y < r.top - 6.0 || y > r.bottom + 6.0 || x < r.left - 10.0 || x > r.right + 10.0 {
        return true;
    }
    let pct = (((x - r.left) / (r.right - r.left)).clamp(0.0, 1.0) * 100.0).round() as i32;
    if which == 0 {
        volume_set(pct);
    } else {
        brightness_set(pct);
    }
    true
}
pub fn slider_key(which: i32, vk: VIRTUAL_KEY, shift: bool) -> bool {
    let step = if shift { 10 } else { 2 };
    let cur = if which == 0 { unsafe { VOL_SHOWN } } else { BRIGHT.load(Relaxed) };
    if cur < 0 {
        return false;
    }
    let d = match vk {
        VK_LEFT | VK_DOWN => -step,
        VK_RIGHT | VK_UP => step,
        _ => return false,
    };
    if which == 0 {
        volume_set(cur + d);
    } else {
        brightness_set(cur + d);
    }
    true
}
pub fn slider_enter(which: i32) {
    if let Some(pct) = digits_pct(&ps().arg.clone()) {
        if which == 0 {
            volume_set(pct);
        } else {
            brightness_set(pct);
        }
        flash(&format!("{}{}%", if which == 0 { "Volume " } else { "Brightness " }, pct));
        return;
    }
    if which == 0 {
        volume_mute();
    }
}

// ======================================================================================
// Status strip: what's playing, a running timer, the weather and the time, on the right
// of the empty bar. Each opens its panel when clicked.
// ======================================================================================
static mut STRIP_HITS: Vec<(D2D_RECT_F, &'static str)> = Vec::new();
pub static mut STRIP_ON: bool = true; // "> layout" can turn it off
fn strip_hits() -> &'static mut Vec<(D2D_RECT_F, &'static str)> {
    unsafe { &mut *std::ptr::addr_of_mut!(STRIP_HITS) }
}

pub fn draw_status_strip(left_limit: f32, right_edge: f32, alpha: f32, t: f64) {
    strip_hits().clear();
    if unsafe { !STRIP_ON } || alpha < 0.02 {
        return;
    }
    let s = st();
    let th = s.th;
    let cy = MARGIN + BAR_H / 2.0;
    let h = 22.0;
    let mut x = right_edge;
    let mut item = |text: &str, glyph: Option<&str>, wx_icon: i32, cmd: &'static str, max_w: f32, col: D2D1_COLOR_F| -> bool {
        let tw = max_w.min(text_width(text, &s.fmt_hint));
        let iw = if glyph.is_some() || wx_icon >= 0 { 20.0 } else { 0.0 };
        let w = tw + iw;
        if x - w < left_limit + 10.0 {
            return false;
        }
        let x0 = x - w;
        if wx_icon >= 0 {
            draw_weather_icon(wx_icon, x0 + 8.0, cy, 20.0, alpha, t);
        } else if let Some(g) = glyph {
            dl(g, &s.fmt_glyph_sm, x0, cy - h / 2.0, 16.0, h, with_a(th.accent, alpha));
        }
        dl(text, &s.fmt_hint_l, x0 + iw, cy - h / 2.0, tw + 2.0, h, with_a(col, alpha));
        strip_hits().push((rect(x0 - 4.0, cy - 14.0, x + 4.0, cy + 14.0), cmd));
        x = x0 - 16.0;
        true
    };
    item(&time_text(&local_time(), false), None, -1, "clock", 120.0, th.dim);
    let w = weather_now();
    if w.ok {
        item(&deg(w.temp), None, weather_icon(w.code, w.day), "weather", 60.0, th.dim);
    }
    let tmr = tm();
    if tmr.set {
        item(&clock(timer_left().ceil()), Some("\u{E916}"), -1, "timer", 80.0, if tmr.run { th.text } else { th.dim });
    } else if tmr.sw_run {
        item(&sw_text(stopwatch_now(), false), Some("\u{E916}"), -1, "stopwatch", 80.0, th.text);
    }
    let (m, _) = media_now();
    if m.has && m.playing && !m.title.is_empty() {
        item(&m.title, Some("\u{E8D6}"), -1, "media", 170.0, th.dim);
    }
}
pub fn strip_at(x: f32, y: f32) -> Option<&'static str> {
    strip_hits().iter().find(|(r, _)| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom).map(|h| h.1)
}

pub fn release_bitmaps() {
    unsafe { *std::ptr::addr_of_mut!(ART_BMP) = None };
}

// Starts the background readers when the bar opens.
pub fn on_show() {
    if MEDIA_EVENT.load(Relaxed) == 0 {
        let e = unsafe { CreateEventW(None, FALSE, FALSE, PCWSTR::null()) }.unwrap_or_default();
        MEDIA_EVENT.store(e.0 as isize, Relaxed);
        std::thread::spawn(media_worker);
    }
    media_wake();
    sys_start();
    weather_maybe_refresh(false);
}
