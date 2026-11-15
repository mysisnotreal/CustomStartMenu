// ---------------------------------------------------------------------------
// Live widgets: what's playing (+ visualizer), weather, this PC, network, clock & calendar,
// timer, stopwatch, volume, brightness, and the status strip on the empty bar.
// Nothing personal is shown: weather only names the country, network only the kind of
// connection and its speed.
// ---------------------------------------------------------------------------
static const float kPI = 3.14159265f;

// ---- images -------------------------------------------------------------------------------
// Decodes a PNG / JPEG into premultiplied BGRA, scaled to fit maxSide.
static bool DecodeImage(const uint8_t* data, size_t size, int maxSide, std::vector<UINT32>& px, int& w, int& h) {
    IWICImagingFactory* wic = nullptr;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic)))) return false;
    bool ok = false;
    IWICStream* st = nullptr;
    IWICBitmapDecoder* dec = nullptr;
    IWICBitmapFrameDecode* fr = nullptr;
    IWICBitmapScaler* sc = nullptr;
    IWICFormatConverter* cv = nullptr;
    if (SUCCEEDED(wic->CreateStream(&st)) && SUCCEEDED(st->InitializeFromMemory((BYTE*)data, (DWORD)size)) &&
        SUCCEEDED(wic->CreateDecoderFromStream(st, nullptr, WICDecodeMetadataCacheOnDemand, &dec)) &&
        SUCCEEDED(dec->GetFrame(0, &fr))) {
        UINT fw = 0, fh = 0;
        fr->GetSize(&fw, &fh);
        if (fw && fh) {
            float k = std::min(1.f, (float)maxSide / (float)std::max(fw, fh));
            w = std::max(1, (int)std::lround(fw * k));
            h = std::max(1, (int)std::lround(fh * k));
            if (SUCCEEDED(wic->CreateBitmapScaler(&sc)) &&
                SUCCEEDED(sc->Initialize(fr, w, h, WICBitmapInterpolationModeHighQualityCubic)) &&
                SUCCEEDED(wic->CreateFormatConverter(&cv)) &&
                SUCCEEDED(cv->Initialize(sc, GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, nullptr, 0,
                                         WICBitmapPaletteTypeCustom))) {
                px.resize((size_t)w * h);
                ok = SUCCEEDED(cv->CopyPixels(nullptr, w * 4, (UINT)(px.size() * 4), (BYTE*)px.data()));
            }
        }
    }
    SafeRelease(cv);
    SafeRelease(sc);
    SafeRelease(fr);
    SafeRelease(dec);
    SafeRelease(st);
    wic->Release();
    return ok;
}

static ID2D1Bitmap* MakeBitmap(const std::vector<UINT32>& px, int w, int h) {
    if (px.empty() || !g_rt) return nullptr;
    ID2D1Bitmap* b = nullptr;
    D2D1_BITMAP_PROPERTIES bp = D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    g_rt->CreateBitmap(D2D1::SizeU(w, h), px.data(), w * 4, bp, &b);
    return b;
}

// Fills a rounded rectangle with a bitmap (cropped to a square, like album art).
static void FillRoundedBitmap(ID2D1Bitmap* bmp, const D2D1_RECT_F& r, float radius, float alpha) {
    ID2D1BitmapBrush* bb = nullptr;
    if (FAILED(g_rt->CreateBitmapBrush(bmp, D2D1::BitmapBrushProperties(), D2D1::BrushProperties(alpha), &bb))) return;
    D2D1_SIZE_F sz = bmp->GetSize();
    float side = std::min(sz.width, sz.height);
    float k = (r.right - r.left) / side;
    bb->SetTransform(Matrix3x2F::Translation(-(sz.width - side) / 2, -(sz.height - side) / 2) * Matrix3x2F::Scale(k, k) *
                     Matrix3x2F::Translation(r.left, r.top));
    bb->SetInterpolationMode(D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, radius, radius), bb);
    bb->Release();
}

// A clickable spot in a self-drawn panel.
struct PanelButton {
    D2D1_RECT_F r;
    int id;
};
static std::vector<PanelButton> g_pbtn;
static int PanelButtonAt(float x, float y) {
    for (auto& b : g_pbtn)
        if (x >= b.r.left && x <= b.r.right && y >= b.r.top && y <= b.r.bottom) return b.id;
    return -1;
}

// ======================================================================================
// Media
// ======================================================================================
struct MediaState {
    wb::MediaInfo info;
    double readAt = 0;
    std::vector<UINT32> art;
    int artW = 0, artH = 0, artVer = 0;
};
static std::mutex g_mediaMx;
static MediaState g_media;
static std::atomic<int> g_mediaCmd{-1};
static HANDLE g_mediaEvent;
static ID2D1Bitmap* g_artBmp;
static int g_artBmpVer = -1;

static void MediaWorker() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    std::wstring lastKey;
    int artTries = 0;
    for (;;) {
        if (WaitForSingleObject(g_quitEvent, 0) == WAIT_OBJECT_0) break;
        int cmd = g_mediaCmd.exchange(-1);
        if (cmd >= 0) {
            wb::MediaCommand(cmd);
            Sleep(250);  // let the player catch up before reading it again
        }
        if (g_barOpen || cmd >= 0) {
            wb::MediaInfo mi;
            bool ok = wb::MediaPoll(mi, false);
            std::wstring key = mi.title + L"\x1" + mi.artist + L"\x1" + mi.app;
            bool newArt = false;
            std::vector<UINT32> px;
            int w = 0, h = 0;
            if (ok && mi.has) {
                if (key != lastKey) { lastKey = key; artTries = 0; }
                if (artTries < 4) {  // players often send the cover a moment after the title
                    artTries++;
                    wb::MediaInfo t;
                    if (wb::MediaPoll(t, true) && !t.thumb.empty() && DecodeImage(t.thumb.data(), t.thumb.size(), 192, px, w, h)) {
                        artTries = 99;
                        newArt = true;
                    } else if (artTries == 1) newArt = true;  // clears the previous song's cover
                }
            } else if (ok && !lastKey.empty()) {
                lastKey.clear();
                newArt = true;
            }
            if (ok) {
                std::lock_guard<std::mutex> lk(g_mediaMx);
                g_media.info = mi;
                g_media.readAt = Now();
                if (newArt) {
                    g_media.art = std::move(px);
                    g_media.artW = w;
                    g_media.artH = h;
                    g_media.artVer++;
                }
            }
        }
        HANDLE hs[2] = {g_quitEvent, g_mediaEvent};
        WaitForMultipleObjects(2, hs, FALSE, g_barOpen ? 1000 : INFINITE);
    }
    CoUninitialize();
}
static void MediaWake() {
    if (g_mediaEvent) SetEvent(g_mediaEvent);
}
static void MediaSend(int cmd) {
    g_mediaCmd = cmd;
    MediaWake();
    // show the change right away
    std::lock_guard<std::mutex> lk(g_mediaMx);
    if (cmd == 0 && g_media.info.has) {
        double p = g_media.info.pos + (g_media.info.playing ? Now() - g_media.readAt : 0);
        g_media.info.playing = !g_media.info.playing;
        g_media.info.pos = p;
        g_media.readAt = Now();
    }
}
static wb::MediaInfo MediaNow(double* pos) {
    std::lock_guard<std::mutex> lk(g_mediaMx);
    wb::MediaInfo m = g_media.info;
    double p = m.pos + (m.playing ? Now() - g_media.readAt : 0);
    if (m.dur > 0) p = std::min(p, m.dur);
    if (pos) *pos = p;
    return m;
}
static ID2D1Bitmap* MediaArt() {
    std::lock_guard<std::mutex> lk(g_mediaMx);
    if (g_artBmpVer != g_media.artVer || (!g_artBmp && !g_media.art.empty())) {
        SafeRelease(g_artBmp);
        g_artBmp = MakeBitmap(g_media.art, g_media.artW, g_media.artH);
        g_artBmpVer = g_media.artVer;
    }
    return g_artBmp;
}
// "Spotify", "Firefox" from the player's app id.
static std::wstring PlayerName(const std::wstring& app) {
    std::wstring s = app;
    size_t bang = s.find(L'!');
    if (bang != std::wstring::npos) {  // packaged: "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"
        std::wstring tail = s.substr(bang + 1);
        if (Lower(tail) != L"app") return tail;
        s = s.substr(0, bang);
        s = s.substr(0, s.find(L'_'));
        size_t dot = s.find_last_of(L'.');
        return dot == std::wstring::npos ? s : s.substr(dot + 1);
    }
    size_t sl = s.find_last_of(L"\\/");
    if (sl != std::wstring::npos) s = s.substr(sl + 1);
    if (Lower(s).size() > 4 && Lower(s).compare(s.size() - 4, 4, L".exe") == 0) s = s.substr(0, s.size() - 4);
    if (!s.empty()) s[0] = (wchar_t)towupper(s[0]);
    if (Lower(s) == L"msedge") return L"Edge";
    return s;
}
static std::wstring Clock(double sec) {
    if (sec < 0) sec = 0;
    int s = (int)sec;
    wchar_t b[32];
    if (s >= 3600) swprintf(b, 32, L"%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60);
    else swprintf(b, 32, L"%d:%02d", s / 60, s % 60);
    return b;
}

// ---- visualizer: what the speakers play (WASAPI loopback) -> 28 bands ------------------
static const int kVizBands = 28;
static std::mutex g_vizMx;
static float g_vizRaw[kVizBands];
static float g_vizShown[kVizBands];
static std::atomic<bool> g_vizWant{false};
static std::atomic<bool> g_vizRunning{false};

static void FFT(std::vector<float>& re, std::vector<float>& im) {
    size_t n = re.size();
    for (size_t i = 1, j = 0; i < n; i++) {
        size_t bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) { std::swap(re[i], re[j]); std::swap(im[i], im[j]); }
    }
    for (size_t len = 2; len <= n; len <<= 1) {
        float ang = -2 * kPI / (float)len;
        float wr = std::cos(ang), wi = std::sin(ang);
        for (size_t i = 0; i < n; i += len) {
            float cr = 1, ci = 0;
            for (size_t k = 0; k < len / 2; k++) {
                size_t a = i + k, b = i + k + len / 2;
                float tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                float nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
    }
}

static void VizWorker() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IMMDeviceEnumerator* en = nullptr;
    IMMDevice* dev = nullptr;
    IAudioClient* ac = nullptr;
    IAudioCaptureClient* cap = nullptr;
    WAVEFORMATEX* wf = nullptr;
    bool ok = SUCCEEDED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, IID_PPV_ARGS(&en))) &&
              SUCCEEDED(en->GetDefaultAudioEndpoint(eRender, eConsole, &dev)) &&
              SUCCEEDED(dev->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr, (void**)&ac)) &&
              SUCCEEDED(ac->GetMixFormat(&wf)) &&
              SUCCEEDED(ac->Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 200000, 0, wf, nullptr)) &&
              SUCCEEDED(ac->GetService(IID_PPV_ARGS(&cap))) && SUCCEEDED(ac->Start());
    if (ok) {
        bool isFloat = wf->wFormatTag == WAVE_FORMAT_IEEE_FLOAT ||
                       (wf->wFormatTag == WAVE_FORMAT_EXTENSIBLE &&
                        reinterpret_cast<WAVEFORMATEXTENSIBLE*>(wf)->SubFormat.Data1 == WAVE_FORMAT_IEEE_FLOAT);
        int ch = std::max<int>(1, wf->nChannels), bits = wf->wBitsPerSample;
        float rate = (float)wf->nSamplesPerSec;
        const size_t N = 2048;
        std::vector<float> ring(N, 0.f), re(N), im(N);
        size_t head = 0;
        double lastSound = 0;
        while (g_vizWant && WaitForSingleObject(g_quitEvent, 12) == WAIT_TIMEOUT) {
            UINT32 pk = 0;
            bool any = false;
            while (SUCCEEDED(cap->GetNextPacketSize(&pk)) && pk) {
                BYTE* data = nullptr;
                UINT32 frames = 0;
                DWORD flags = 0;
                if (FAILED(cap->GetBuffer(&data, &frames, &flags, nullptr, nullptr))) break;
                for (UINT32 f = 0; f < frames; f++) {
                    float v = 0;
                    if (!(flags & AUDCLNT_BUFFERFLAGS_SILENT)) {
                        for (int c = 0; c < ch; c++) {
                            if (isFloat && bits == 32) v += reinterpret_cast<float*>(data)[f * ch + c];
                            else if (bits == 16) v += reinterpret_cast<int16_t*>(data)[f * ch + c] / 32768.f;
                            else if (bits == 32) v += reinterpret_cast<int32_t*>(data)[f * ch + c] / 2147483648.f;
                        }
                        v /= ch;
                    }
                    ring[head] = v;
                    head = (head + 1) % N;
                }
                cap->ReleaseBuffer(frames);
                any = true;
            }
            if (any) lastSound = Now();
            float bands[kVizBands] = {};
            if (Now() - lastSound < 0.3) {
                for (size_t i = 0; i < N; i++) {
                    float w = 0.5f - 0.5f * std::cos(2 * kPI * i / (N - 1));  // Hann window
                    re[i] = ring[(head + i) % N] * w;
                    im[i] = 0;
                }
                FFT(re, im);
                // log-spaced bands from 45 Hz to 14 kHz
                for (int b = 0; b < kVizBands; b++) {
                    float f0 = 45.f * std::pow(14000.f / 45.f, (float)b / kVizBands);
                    float f1 = 45.f * std::pow(14000.f / 45.f, (float)(b + 1) / kVizBands);
                    int i0 = std::max(1, (int)(f0 / rate * N)), i1 = std::max(i0 + 1, (int)(f1 / rate * N));
                    float m = 0;
                    for (int i = i0; i < i1 && i < (int)N / 2; i++) m = std::max(m, std::sqrt(re[i] * re[i] + im[i] * im[i]));
                    float db = 20.f * std::log10(m / (N / 4.f) + 1e-9f);
                    bands[b] = std::clamp((db + 62.f) / 56.f, 0.f, 1.f);
                }
            }
            std::lock_guard<std::mutex> lk(g_vizMx);
            memcpy(g_vizRaw, bands, sizeof(bands));
        }
        ac->Stop();
    }
    if (wf) CoTaskMemFree(wf);
    SafeRelease(cap);
    SafeRelease(ac);
    SafeRelease(dev);
    SafeRelease(en);
    {
        std::lock_guard<std::mutex> lk(g_vizMx);
        memset(g_vizRaw, 0, sizeof(g_vizRaw));
    }
    g_vizRunning = false;
    CoUninitialize();
}
static void VizStart() {
    g_vizWant = true;
    if (g_vizRunning.exchange(true)) return;
    std::thread(VizWorker).detach();
}
static void VizStop() { g_vizWant = false; }

// Bars rising from the bottom of r, mirrored softly.
static void DrawViz(const D2D1_RECT_F& r, float alpha, float dt) {
    float raw[kVizBands];
    {
        std::lock_guard<std::mutex> lk(g_vizMx);
        memcpy(raw, g_vizRaw, sizeof(raw));
    }
    const Theme& th = g_th;
    float gap = 3.f, bw = ((r.right - r.left) - gap * (kVizBands - 1)) / kVizBands;
    for (int b = 0; b < kVizBands; b++) {
        float target = raw[b];
        // quick to rise, slow to fall
        g_vizShown[b] = target > g_vizShown[b] ? Damp(g_vizShown[b], target, 30.f, dt) : Damp(g_vizShown[b], target, 6.f, dt);
        float hgt = std::max(2.f, (r.bottom - r.top) * g_vizShown[b]);
        float x = r.left + b * (bw + gap);
        D2D1_COLOR_F c = LerpC(th.accent, th.text, 0.15f * (float)b / kVizBands);
        if (ID2D1LinearGradientBrush* br = MakeGradient({0, r.bottom - hgt}, {0, r.bottom}, WithA(c, alpha), WithA(c, alpha * 0.45f))) {
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, r.bottom - hgt, x + bw, r.bottom), bw / 2, bw / 2), br);
            br->Release();
        }
    }
}

// ---- the panel ----------------------------------------------------------------------------
enum { MB_PREV = 1, MB_PLAY, MB_NEXT, MB_SEEK };
static float MediaHeight() { return 170.f; }
// Text that may take two lines.
static void DrawWrapped(const std::wstring& s, IDWriteTextFormat* fmt, float x, float y, float w, float h, D2D1_COLOR_F col) {
    IDWriteTextLayout* l = nullptr;
    if (SUCCEEDED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), fmt, w, h, &l))) {
        l->SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
        g_solid->SetColor(col);
        g_rt->DrawTextLayout({x, y}, l, g_solid);
        l->Release();
    }
}
static void DrawMedia(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    g_pbtn.clear();
    double pos = 0;
    wb::MediaInfo m = MediaNow(&pos);
    const float art = 132.f, x0 = Margin + 18, y0 = top + 18;
    D2D1_RECT_F ar = D2D1::RectF(x0, y0, x0 + art, y0 + art);
    ID2D1Bitmap* bmp = m.has ? MediaArt() : nullptr;
    if (bmp) {
        DrawShadow(ar, 0.35f * alpha, g_tileShadow);
        FillRoundedBitmap(bmp, ar, 10, alpha);
    } else {
        DrawCard(ar, alpha);
        DrawLayout(L"\xE8D6", g_fmtBigC, ar.left, ar.top, art, art, WithA(th.dim, alpha * 0.8f));
    }
    float tx = ar.right + 22, tw = Margin + BarW - 20 - tx;
    if (!m.has) {
        DrawLayout(L"Nothing is playing", g_fmtMid, tx, y0 + 18, tw, 30, WithA(th.text, alpha));
        DrawWrapped(L"Play something in Spotify, a browser or any music app, and it shows up here.", g_fmtRow, tx, y0 + 50, tw, 44,
                    WithA(th.dim, alpha));
        return;
    }
    DrawLayout(m.title.empty() ? L"Unknown title" : m.title, g_fmtMid, tx, y0 + 2, tw, 28, WithA(th.text, alpha));
    std::wstring sub = m.artist;
    std::wstring player = PlayerName(m.app);
    if (!player.empty()) sub += (sub.empty() ? L"" : L"  ·  ") + player;
    DrawLayout(sub, g_fmtRow, tx, y0 + 30, tw, 22, WithA(th.dim, alpha));

    // progress
    float py = y0 + 66;
    if (m.dur > 0) {
        float f = (float)std::clamp(pos / m.dur, 0.0, 1.0);
        g_solid->SetColor(WithA(th.dim, alpha * 0.25f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(tx, py, tx + tw, py + 4), 2, 2), g_solid);
        g_solid->SetColor(WithA(th.accent, alpha));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(tx, py, tx + tw * f, py + 4), 2, 2), g_solid);
        g_rt->FillEllipse(D2D1::Ellipse({tx + tw * f, py + 2}, 5, 5), g_solid);
        DrawLayout(Clock(pos), g_fmtSmall, tx, py + 7, 80, 16, WithA(th.dim, alpha));
        DrawLayout(Clock(m.dur), g_fmtSmallR, tx + tw - 80, py + 7, 80, 16, WithA(th.dim, alpha));
    }

    // controls
    float cy = y0 + 112;
    struct B { int id; const wchar_t* g; float x; float r; bool on; };
    B bs[] = {{MB_PREV, L"\xE892", tx + 18, 17, m.canPrev}, {MB_PLAY, m.playing ? L"\xE769" : L"\xE768", tx + 64, 21, true},
              {MB_NEXT, L"\xE893", tx + 110, 17, m.canNext}};
    for (auto& b : bs) {
        if (b.id == MB_PLAY) {
            g_solid->SetColor(WithA(th.accent, alpha));
            g_rt->FillEllipse(D2D1::Ellipse({b.x, cy}, b.r, b.r), g_solid);
            DrawLayout(b.g, g_fmtGlyph, b.x - b.r, cy - b.r, b.r * 2, b.r * 2, WithA(C(0xFFFFFF), alpha));
        } else {
            g_solid->SetColor(WithA(th.highlight, alpha * 1.4f));
            g_rt->FillEllipse(D2D1::Ellipse({b.x, cy}, b.r, b.r), g_solid);
            DrawLayout(b.g, g_fmtGlyph, b.x - b.r, cy - b.r, b.r * 2, b.r * 2, WithA(b.on ? th.text : th.dim, alpha * (b.on ? 1.f : 0.5f)));
        }
        g_pbtn.push_back({D2D1::RectF(b.x - b.r, cy - b.r, b.x + b.r, cy + b.r), b.id});
    }
    if (m.dur > 0) g_pbtn.push_back({D2D1::RectF(tx, py - 8, tx + tw, py + 12), MB_SEEK});
    // visualizer to the right of the buttons
    DrawViz(D2D1::RectF(tx + 150, cy - 22, tx + tw, cy + 20), alpha * 0.9f, dt);
}
static bool MediaKey(WPARAM vk, bool shift) {
    if (shift) return false;
    if (vk == VK_LEFT) { MediaSend(2); return true; }
    if (vk == VK_RIGHT) { MediaSend(1); return true; }
    return false;
}
static bool MediaClick(float x, float y) {
    int b = PanelButtonAt(x, y);
    if (b == MB_PREV) MediaSend(2);
    else if (b == MB_NEXT) MediaSend(1);
    else if (b == MB_PLAY) MediaSend(0);
    return true;
}

// ======================================================================================
// Weather (Open-Meteo, no account). Located by the country set in Windows (and, in big
// countries, its time zone); only the country's name is ever shown.
// ======================================================================================
struct WeatherHour {
    int hour;
    double t;
    int code;
    bool day;
};
struct WeatherState {
    bool ok = false, failed = false;
    std::wstring country, why;
    double temp = 0;
    int code = 0;
    bool day = true, fahrenheit = false;
    std::vector<WeatherHour> hours;
    double at = -1e9;
};
static std::mutex g_wxMx;
static WeatherState g_wx;
static std::atomic<bool> g_wxLoading{false};

// Rough spots for time zones in countries that span several, so "the weather" isn't the
// middle of nowhere. Never shown anywhere.
static bool TzSpot(const std::wstring& iso, const std::wstring& tz, double& lat, double& lon) {
    static const struct { const wchar_t* iso; const wchar_t* tz; double lat, lon; } spots[] = {
        {L"US", L"Eastern Standard Time", 40.7, -74.0},   {L"US", L"Central Standard Time", 41.9, -87.6},
        {L"US", L"Mountain Standard Time", 39.7, -105.0}, {L"US", L"US Mountain Standard Time", 33.4, -112.1},
        {L"US", L"Pacific Standard Time", 34.05, -118.2}, {L"US", L"Alaskan Standard Time", 61.2, -149.9},
        {L"US", L"Hawaiian Standard Time", 21.3, -157.9}, {L"US", L"US Eastern Standard Time", 39.8, -86.2},
        {L"CA", L"Eastern Standard Time", 43.7, -79.4},   {L"CA", L"Central Standard Time", 49.9, -97.1},
        {L"CA", L"Canada Central Standard Time", 50.4, -104.6}, {L"CA", L"Mountain Standard Time", 51.0, -114.1},
        {L"CA", L"Pacific Standard Time", 49.3, -123.1},  {L"CA", L"Atlantic Standard Time", 44.6, -63.6},
        {L"CA", L"Newfoundland Standard Time", 47.6, -52.7},
        {L"AU", L"AUS Eastern Standard Time", -33.9, 151.2}, {L"AU", L"E. Australia Standard Time", -27.5, 153.0},
        {L"AU", L"Cen. Australia Standard Time", -34.9, 138.6}, {L"AU", L"AUS Central Standard Time", -12.5, 130.8},
        {L"AU", L"W. Australia Standard Time", -31.95, 115.9}, {L"AU", L"Tasmania Standard Time", -42.9, 147.3},
        {L"RU", L"Russian Standard Time", 55.75, 37.6},   {L"RU", L"Ekaterinburg Standard Time", 56.8, 60.6},
        {L"RU", L"N. Central Asia Standard Time", 55.0, 82.9}, {L"RU", L"North Asia Standard Time", 56.0, 92.9},
        {L"RU", L"North Asia East Standard Time", 52.3, 104.3}, {L"RU", L"Yakutsk Standard Time", 62.0, 129.7},
        {L"RU", L"Vladivostok Standard Time", 43.1, 131.9}, {L"RU", L"Kaliningrad Standard Time", 54.7, 20.5},
        {L"RU", L"Omsk Standard Time", 55.0, 73.4},       {L"RU", L"Samara Standard Time", 53.2, 50.1},
        {L"BR", L"E. South America Standard Time", -23.55, -46.6}, {L"BR", L"SA Western Standard Time", -3.1, -60.0},
        {L"BR", L"Central Brazilian Standard Time", -15.6, -56.1}, {L"BR", L"SA Eastern Standard Time", -3.7, -38.5},
        {L"BR", L"Bahia Standard Time", -12.97, -38.5},   {L"BR", L"Tocantins Standard Time", -10.2, -48.3},
        {L"MX", L"Central Standard Time (Mexico)", 19.4, -99.1}, {L"MX", L"Mountain Standard Time (Mexico)", 23.2, -106.4},
        {L"MX", L"Pacific Standard Time (Mexico)", 32.5, -117.0}, {L"MX", L"Eastern Standard Time (Mexico)", 21.2, -86.8},
        {L"CN", L"China Standard Time", 31.2, 121.5},     {L"IN", L"India Standard Time", 19.1, 72.9},
        {L"ID", L"SE Asia Standard Time", -6.2, 106.8},   {L"KZ", L"Central Asia Standard Time", 43.2, 76.9},
    };
    for (auto& s : spots)
        if (iso == s.iso && tz == s.tz) { lat = s.lat; lon = s.lon; return true; }
    return false;
}

static double JsonNum(const std::string& js, size_t from, const char* key, size_t end, bool* found = nullptr) {
    std::string k = std::string("\"") + key + "\":";
    size_t p = js.find(k, from);
    if (found) *found = p != std::string::npos && p < end;
    if (p == std::string::npos || p >= end) return 0;
    return atof(js.c_str() + p + k.size());
}
static std::vector<std::string> JsonArray(const std::string& js, size_t from, const char* key) {
    std::vector<std::string> out;
    std::string k = std::string("\"") + key + "\":[";
    size_t p = js.find(k, from);
    if (p == std::string::npos) return out;
    p += k.size();
    size_t e = js.find(']', p);
    while (p < e) {
        size_t c = js.find(',', p);
        if (c == std::string::npos || c > e) c = e;
        std::string v = js.substr(p, c - p);
        if (!v.empty() && v[0] == '"') v = v.substr(1, v.size() >= 2 ? v.size() - 2 : 0);
        out.push_back(v);
        p = c + 1;
    }
    return out;
}

static void WeatherFetch() {
    WeatherState w;
    GEOID geo = GetUserGeoID(GEOCLASS_NATION);
    wchar_t buf[128] = {};
    double lat = 0, lon = 0;
    bool have = false;
    if (geo != GEOID_NOT_AVAILABLE) {
        if (GetGeoInfoW(geo, GEO_FRIENDLYNAME, buf, 128, 0)) w.country = buf;
        std::wstring iso;
        if (GetGeoInfoW(geo, GEO_ISO2, buf, 128, 0)) iso = buf;
        DYNAMIC_TIME_ZONE_INFORMATION tz = {};
        GetDynamicTimeZoneInformation(&tz);
        if (TzSpot(iso, tz.TimeZoneKeyName, lat, lon)) have = true;
        else if (GetGeoInfoW(geo, GEO_LATITUDE, buf, 128, 0)) {
            lat = _wtof(buf);
            if (GetGeoInfoW(geo, GEO_LONGITUDE, buf, 128, 0)) { lon = _wtof(buf); have = true; }
        }
    }
    wchar_t meas[4] = {};
    GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_IMEASURE, meas, 4);
    w.fahrenheit = meas[0] == L'1';
    if (!have) {
        w.failed = true;
        w.why = L"Set your country in Windows Settings › Time & language › Language & region";
    } else {
        wchar_t url[512];
        swprintf(url, 512,
                 L"https://api.open-meteo.com/v1/forecast?latitude=%.2f&longitude=%.2f&current=temperature_2m,weather_code,is_day"
                 L"&hourly=temperature_2m,weather_code,is_day&forecast_hours=7&timezone=auto%s",
                 lat, lon, w.fahrenheit ? L"&temperature_unit=fahrenheit" : L"");
        std::string js;
        size_t cur;
        if (HttpGet(url, &js, L"", nullptr) && (cur = js.find("\"current\":{")) != std::string::npos) {
            size_t end = js.find('}', cur);
            w.temp = JsonNum(js, cur, "temperature_2m", end);
            w.code = (int)JsonNum(js, cur, "weather_code", end);
            w.day = JsonNum(js, cur, "is_day", end) != 0;
            size_t hr = js.find("\"hourly\":{");
            if (hr != std::string::npos) {
                auto times = JsonArray(js, hr, "time"), temps = JsonArray(js, hr, "temperature_2m"),
                     codes = JsonArray(js, hr, "weather_code"), days = JsonArray(js, hr, "is_day");
                for (size_t i = 1; i < times.size() && i < temps.size() && i < codes.size() && i < days.size(); i++) {
                    size_t t = times[i].find('T');
                    int hour = t == std::string::npos ? 0 : atoi(times[i].c_str() + t + 1);
                    w.hours.push_back({hour, atof(temps[i].c_str()), atoi(codes[i].c_str()), atoi(days[i].c_str()) != 0});
                }
            }
            w.ok = true;
        } else {
            w.failed = true;
            w.why = L"Couldn't reach the weather service";
        }
    }
    w.at = Now();
    {
        std::lock_guard<std::mutex> lk(g_wxMx);
        g_wx = w;
    }
    g_wxLoading = false;
    PostMessageW(g_hwnd, WM_APP_PANEL, 0, 0);
}
// Refreshes when it's older than 20 minutes (or a minute after a failure).
static void WeatherMaybeRefresh(bool force = false) {
    double age;
    bool failed;
    {
        std::lock_guard<std::mutex> lk(g_wxMx);
        age = Now() - g_wx.at;
        failed = g_wx.failed;
    }
    if (!force && age < (failed ? 60 : 20 * 60)) return;
    if (g_wxLoading.exchange(true)) return;
    std::thread(WeatherFetch).detach();
}
static WeatherState WeatherNow() {
    std::lock_guard<std::mutex> lk(g_wxMx);
    return g_wx;
}

enum { WI_SUN, WI_MOON, WI_PARTLY_DAY, WI_PARTLY_NIGHT, WI_CLOUD, WI_FOG, WI_DRIZZLE, WI_RAIN, WI_SNOW, WI_THUNDER };
static int WeatherIcon(int code, bool day) {
    if (code <= 1) return day ? WI_SUN : WI_MOON;
    if (code == 2) return day ? WI_PARTLY_DAY : WI_PARTLY_NIGHT;
    if (code == 3) return WI_CLOUD;
    if (code == 45 || code == 48) return WI_FOG;
    if (code >= 51 && code <= 57) return WI_DRIZZLE;
    if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return WI_RAIN;
    if ((code >= 71 && code <= 77) || code == 85 || code == 86) return WI_SNOW;
    if (code >= 95) return WI_THUNDER;
    return WI_CLOUD;
}
static std::wstring WeatherWord(int code, bool day) {
    if (code == 0) return day ? L"Sunny" : L"Clear";
    if (code == 1) return day ? L"Mostly sunny" : L"Mostly clear";
    if (code == 2) return L"Partly cloudy";
    if (code == 3) return L"Cloudy";
    if (code == 45 || code == 48) return L"Fog";
    if (code == 56 || code == 57) return L"Freezing drizzle";
    if (code >= 51 && code <= 55) return L"Drizzle";
    if (code == 66 || code == 67) return L"Freezing rain";
    if (code >= 61 && code <= 65) return code == 65 ? L"Heavy rain" : L"Rain";
    if (code >= 80 && code <= 82) return L"Showers";
    if (code == 85 || code == 86) return L"Snow showers";
    if (code >= 71 && code <= 77) return code == 75 ? L"Heavy snow" : L"Snow";
    if (code >= 95) return L"Thunderstorm";
    return L"Cloudy";
}
static bool ThemeIsLight() {
    const D2D1_COLOR_F& t = g_th.text;
    return 0.299f * t.r + 0.587f * t.g + 0.114f * t.b < 0.5f;
}

// ---- weather icons, drawn and animated (s = size in DIPs) ---------------------------------
static void DrawSun(float cx, float cy, float s, double now) {
    float rot = (float)std::fmod(now * 0.35, 2 * kPI);
    g_solid->SetColor(C(0xFFC83D));
    for (int i = 0; i < 8; i++) {
        float a = rot + i * kPI / 4;
        float r0 = s * 0.31f, r1 = s * (0.42f + 0.02f * std::sin((float)now * 3 + i));
        g_rt->DrawLine({cx + std::cos(a) * r0, cy + std::sin(a) * r0}, {cx + std::cos(a) * r1, cy + std::sin(a) * r1}, g_solid,
                       std::max(1.5f, s * 0.055f), nullptr);
    }
    D2D1_GRADIENT_STOP st[2] = {{0, C(0xFFE27A)}, {1, C(0xFFB020)}};
    ID2D1GradientStopCollection* coll = nullptr;
    ID2D1RadialGradientBrush* rb = nullptr;
    if (SUCCEEDED(g_rt->CreateGradientStopCollection(st, 2, &coll)) &&
        SUCCEEDED(g_rt->CreateRadialGradientBrush(D2D1::RadialGradientBrushProperties({cx - s * 0.06f, cy - s * 0.06f}, {0, 0}, s * 0.26f, s * 0.26f), coll, &rb)))
        g_rt->FillEllipse(D2D1::Ellipse({cx, cy}, s * 0.22f, s * 0.22f), rb);
    SafeRelease(rb);
    SafeRelease(coll);
}
static void DrawMoon(float cx, float cy, float s, double now, bool stars) {
    ID2D1EllipseGeometry *a = nullptr, *b = nullptr;
    ID2D1PathGeometry* p = nullptr;
    g_d2d->CreateEllipseGeometry(D2D1::Ellipse({cx, cy}, s * 0.27f, s * 0.27f), &a);
    g_d2d->CreateEllipseGeometry(D2D1::Ellipse({cx + s * 0.14f, cy - s * 0.10f}, s * 0.23f, s * 0.23f), &b);
    if (a && b && SUCCEEDED(g_d2d->CreatePathGeometry(&p))) {
        ID2D1GeometrySink* sink = nullptr;
        if (SUCCEEDED(p->Open(&sink))) {
            a->CombineWithGeometry(b, D2D1_COMBINE_MODE_EXCLUDE, nullptr, sink);
            sink->Close();
            sink->Release();
        }
        g_solid->SetColor(C(0xF2EBC8));
        g_rt->FillGeometry(p, g_solid);
    }
    SafeRelease(p);
    SafeRelease(a);
    SafeRelease(b);
    if (stars) {
        const float sp[3][3] = {{0.30f, -0.30f, 0.0f}, {0.38f, 0.05f, 1.7f}, {0.12f, -0.42f, 3.1f}};
        for (auto& q : sp) {
            float tw = 0.5f + 0.5f * std::sin((float)now * 2.2f + q[2]);
            g_solid->SetColor(C(0xFFF6D8, 0.35f + 0.65f * tw));
            float r = s * (0.025f + 0.02f * tw);
            g_rt->FillEllipse(D2D1::Ellipse({cx + s * q[0], cy + s * q[1]}, r, r), g_solid);
        }
    }
}
static void DrawCloud(float cx, float cy, float s, D2D1_COLOR_F col) {
    g_solid->SetColor(col);
    g_rt->FillEllipse(D2D1::Ellipse({cx - s * 0.16f, cy + s * 0.02f}, s * 0.17f, s * 0.17f), g_solid);
    g_rt->FillEllipse(D2D1::Ellipse({cx + s * 0.05f, cy - s * 0.07f}, s * 0.22f, s * 0.22f), g_solid);
    g_rt->FillEllipse(D2D1::Ellipse({cx + s * 0.22f, cy + s * 0.05f}, s * 0.14f, s * 0.14f), g_solid);
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(cx - s * 0.33f, cy + s * 0.02f, cx + s * 0.36f, cy + s * 0.19f), s * 0.085f, s * 0.085f), g_solid);
}
static void DrawWeatherIcon(int kind, float cx, float cy, float s, float alpha, double now) {
    if (alpha <= 0.01f) return;
    g_rt->PushLayer(D2D1::LayerParameters(D2D1::InfiniteRect(), nullptr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1::IdentityMatrix(), alpha), nullptr);
    bool light = ThemeIsLight();
    D2D1_COLOR_F cloud = light ? C(0xB9C4D3) : C(0xDDE4EF);
    D2D1_COLOR_F dark = light ? C(0x8C97A8) : C(0x9AA4B5);
    float drift = std::sin((float)now * 0.9f) * s * 0.025f;
    switch (kind) {
    case WI_SUN: DrawSun(cx, cy, s, now); break;
    case WI_MOON: DrawMoon(cx, cy, s, now, true); break;
    case WI_PARTLY_DAY:
        DrawSun(cx + s * 0.13f, cy - s * 0.12f, s * 0.8f, now);
        DrawCloud(cx - s * 0.04f + drift, cy + s * 0.08f, s * 0.9f, cloud);
        break;
    case WI_PARTLY_NIGHT:
        DrawMoon(cx + s * 0.12f, cy - s * 0.12f, s * 0.8f, now, false);
        DrawCloud(cx - s * 0.04f + drift, cy + s * 0.08f, s * 0.9f, cloud);
        break;
    case WI_CLOUD:
        DrawCloud(cx + s * 0.12f - drift, cy - s * 0.10f, s * 0.7f, dark);
        DrawCloud(cx - s * 0.03f + drift, cy + s * 0.04f, s * 0.95f, cloud);
        break;
    case WI_FOG:
        for (int i = 0; i < 4; i++) {
            float off = std::sin((float)now * 0.8f + i * 1.3f) * s * 0.06f;
            float y = cy - s * 0.18f + i * s * 0.12f, w = s * (i % 2 ? 0.30f : 0.36f);
            g_solid->SetColor(WithA(cloud, 0.55f + 0.12f * i));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(cx - w + off, y - s * 0.03f, cx + w + off, y + s * 0.03f), s * 0.03f, s * 0.03f), g_solid);
        }
        break;
    case WI_DRIZZLE:
    case WI_RAIN:
    case WI_THUNDER: {
        int drops = kind == WI_DRIZZLE ? 3 : 4;
        g_solid->SetColor(C(0x5AB0FF));
        for (int i = 0; i < drops; i++) {
            float ph = (float)std::fmod(now * (kind == WI_DRIZZLE ? 0.9 : 1.4) + i * 0.37, 1.0);
            float x = cx - s * 0.18f + i * s * (0.36f / (drops - 1)) - ph * s * 0.05f;
            float y = cy + s * 0.18f + ph * s * 0.28f;
            float len = kind == WI_DRIZZLE ? s * 0.05f : s * 0.10f;
            g_solid->SetColor(C(0x5AB0FF, 1.f - ph));
            g_rt->DrawLine({x, y}, {x - len * 0.35f, y + len}, g_solid, std::max(1.4f, s * 0.04f), nullptr);
        }
        if (kind == WI_THUNDER) {
            float flash = std::fmod((float)now, 2.6f) < 0.12f ? 1.f : 0.75f;
            ID2D1PathGeometry* bolt = nullptr;
            if (SUCCEEDED(g_d2d->CreatePathGeometry(&bolt))) {
                ID2D1GeometrySink* sk = nullptr;
                if (SUCCEEDED(bolt->Open(&sk))) {
                    sk->BeginFigure({cx + s * 0.02f, cy + s * 0.08f}, D2D1_FIGURE_BEGIN_FILLED);
                    sk->AddLine({cx - s * 0.10f, cy + s * 0.30f});
                    sk->AddLine({cx + s * 0.01f, cy + s * 0.29f});
                    sk->AddLine({cx - s * 0.05f, cy + s * 0.46f});
                    sk->AddLine({cx + s * 0.13f, cy + s * 0.22f});
                    sk->AddLine({cx + s * 0.02f, cy + s * 0.23f});
                    sk->AddLine({cx + s * 0.09f, cy + s * 0.08f});
                    sk->EndFigure(D2D1_FIGURE_END_CLOSED);
                    sk->Close();
                    sk->Release();
                }
                g_solid->SetColor(C(0xFFD43B, flash));
                g_rt->FillGeometry(bolt, g_solid);
                bolt->Release();
            }
        }
        DrawCloud(cx + drift, cy - s * 0.06f, s, kind == WI_THUNDER ? dark : cloud);
        break;
    }
    case WI_SNOW: {
        for (int i = 0; i < 5; i++) {
            float ph = (float)std::fmod(now * 0.45 + i * 0.29, 1.0);
            float x = cx - s * 0.22f + i * s * 0.11f + std::sin((float)now * 2 + i) * s * 0.03f;
            float y = cy + s * 0.16f + ph * s * 0.30f;
            g_solid->SetColor(C(0xFFFFFF, (1.f - ph) * (light ? 0.9f : 1.f)));
            if (light) g_solid->SetColor(C(0x9FB6D6, 1.f - ph));
            g_rt->FillEllipse(D2D1::Ellipse({x, y}, s * 0.035f, s * 0.035f), g_solid);
        }
        DrawCloud(cx + drift, cy - s * 0.06f, s, cloud);
        break;
    }
    }
    g_rt->PopLayer();
}

static std::wstring Deg(double t) { return std::to_wstring((int)std::lround(t)) + L"°"; }
static std::wstring HourLabel(int h) {
    wchar_t fmt[8] = {};
    GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_ITIME, fmt, 8);
    if (fmt[0] == L'1') return (h < 10 ? L"0" : L"") + std::to_wstring(h) + L":00";
    int h12 = h % 12 == 0 ? 12 : h % 12;
    return std::to_wstring(h12) + (h < 12 ? L" AM" : L" PM");
}
static float WeatherHeight() { return 168.f; }
static void DrawWeather(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    WeatherState w = WeatherNow();
    PanelHeader(L"Weather", w.ok ? L"↵ refresh" : L"", top, alpha);
    float y0 = top + 44;
    if (!w.ok) {
        bool loading = g_wxLoading || !w.failed;
        DrawLayout(loading ? L"\xE895" : L"\xE7BA", g_fmtBigC, Margin + 24, y0, 80, 80, WithA(th.dim, alpha * 0.8f));
        DrawLayout(loading ? L"Checking the weather…" : L"No weather right now", g_fmtMid, Margin + 120, y0 + 10, BarW - 150, 30, WithA(th.text, alpha));
        if (!loading) DrawLayout(w.why, g_fmtRow, Margin + 120, y0 + 40, BarW - 150, 40, WithA(th.dim, alpha));
        return;
    }
    DrawWeatherIcon(WeatherIcon(w.code, w.day), Margin + 70, y0 + 46, 96, alpha, now);
    float tx = Margin + 132;
    DrawLayout(Deg(w.temp), g_fmtBig, tx, y0 + 2, 160, 52, WithA(th.text, alpha));
    DrawLayout(WeatherWord(w.code, w.day), g_fmtMid, tx, y0 + 52, 200, 26, WithA(th.text, alpha * 0.9f));
    DrawLayout(L"\xE707", g_fmtGlyphSm, tx, y0 + 80, 14, 20, WithA(th.dim, alpha));
    DrawLayout(w.country.empty() ? L"Your country" : w.country, g_fmtSmall, tx + 18, y0 + 80, 180, 20, WithA(th.dim, alpha));
    // the next hours
    int n = std::min<int>(6, (int)w.hours.size());
    float colW = 50.f, hx = Margin + BarW - 18 - n * colW;
    for (int i = 0; i < n; i++) {
        auto& h = w.hours[i];
        float x = hx + i * colW;
        DrawLayout(HourLabel(h.hour), g_fmtSmallC, x, y0 + 4, colW, 18, WithA(th.dim, alpha));
        DrawWeatherIcon(WeatherIcon(h.code, h.day), x + colW / 2, y0 + 46, 34, alpha, now + i * 0.7);
        DrawLayout(Deg(h.t), g_fmtSmallC, x, y0 + 72, colW, 18, WithA(th.text, alpha));
    }
}

// ======================================================================================
// This PC and network: sampled once a second
// ======================================================================================
struct SysState {
    std::vector<float> cpu, ram, gpu, down, up;  // last 60 seconds; net in bytes / s
    float cpuNow = 0, ramNow = 0, gpuNow = -1;
    double ramUsed = 0, ramTotal = 0;
    double downNow = 0, upNow = 0;
    int net = 0;  // 0 offline, 1 Wi-Fi, 2 Ethernet
    int battery = -1;
    bool charging = false;
    double diskFree = 0, diskTotal = 0;
};
static std::mutex g_sysMx;
static SysState g_sys;
static std::atomic<bool> g_sysStarted{false};

static void PushHist(std::vector<float>& v, float x) {
    v.push_back(x);
    if (v.size() > 60) v.erase(v.begin());
}
static unsigned long long FtU(const FILETIME& f) { return ((unsigned long long)f.dwHighDateTime << 32) | f.dwLowDateTime; }

static void SysWorker() {
    unsigned long long pIdle = 0, pKern = 0, pUser = 0, pIn = 0, pOut = 0;
    double pT = 0;
    PDH_HQUERY q = nullptr;
    PDH_HCOUNTER gpuC = nullptr;
    std::vector<BYTE> pdhBuf;
    do {
        SysState s;
        {
            std::lock_guard<std::mutex> lk(g_sysMx);
            s = g_sys;
        }
        FILETIME fi, fk, fu;
        float cpu = 0;
        if (GetSystemTimes(&fi, &fk, &fu)) {
            unsigned long long i = FtU(fi), k = FtU(fk), u = FtU(fu);
            unsigned long long total = (k - pKern) + (u - pUser), idle = i - pIdle;
            if (pKern && total) cpu = (float)(total - idle) / total;
            pIdle = i;
            pKern = k;
            pUser = u;
        }
        MEMORYSTATUSEX ms = {sizeof(ms)};
        GlobalMemoryStatusEx(&ms);
        s.ramTotal = (double)ms.ullTotalPhys;
        s.ramUsed = (double)(ms.ullTotalPhys - ms.ullAvailPhys);
        float ram = s.ramTotal > 0 ? (float)(s.ramUsed / s.ramTotal) : 0;

        // GPU: busiest engine type, as Task Manager shows it (only while the bar is open)
        float gpu = -1;
        if (g_barOpen) {
            if (!q && PdhOpenQueryW(nullptr, 0, &q) == ERROR_SUCCESS &&
                PdhAddEnglishCounterW(q, L"\\GPU Engine(*)\\Utilization Percentage", 0, &gpuC) != ERROR_SUCCESS) {
                PdhCloseQuery(q);
                q = nullptr;
            }
            if (q && PdhCollectQueryData(q) == ERROR_SUCCESS) {
                DWORD size = 0, count = 0;
                if (PdhGetFormattedCounterArrayW(gpuC, PDH_FMT_DOUBLE, &size, &count, nullptr) == (PDH_STATUS)PDH_MORE_DATA) {
                    pdhBuf.resize(size);
                    auto* items = (PDH_FMT_COUNTERVALUE_ITEM_W*)pdhBuf.data();
                    if (PdhGetFormattedCounterArrayW(gpuC, PDH_FMT_DOUBLE, &size, &count, items) == ERROR_SUCCESS) {
                        std::unordered_map<std::wstring, double> byType;
                        for (DWORD i = 0; i < count; i++) {
                            std::wstring n = items[i].szName;
                            size_t e = n.find(L"engtype_");
                            if (e == std::wstring::npos || items[i].FmtValue.CStatus != ERROR_SUCCESS) continue;
                            byType[n.substr(e)] += items[i].FmtValue.doubleValue;
                        }
                        double mx = 0;
                        for (auto& kv : byType) mx = std::max(mx, kv.second);
                        gpu = (float)std::min(1.0, mx / 100.0);
                    }
                }
            }
        }

        // network: physical Wi-Fi / Ethernet adapters only
        MIB_IF_TABLE2* tab = nullptr;
        unsigned long long inB = 0, outB = 0;
        int net = 0;
        if (GetIfTable2(&tab) == NO_ERROR) {
            for (ULONG i = 0; i < tab->NumEntries; i++) {
                const MIB_IF_ROW2& r = tab->Table[i];
                if (!r.InterfaceAndOperStatusFlags.HardwareInterface || r.OperStatus != IfOperStatusUp) continue;
                if (r.Type != IF_TYPE_IEEE80211 && r.Type != IF_TYPE_ETHERNET_CSMACD) continue;
                inB += r.InOctets;
                outB += r.OutOctets;
                if (r.Type == IF_TYPE_IEEE80211) net = 1;
                else if (net == 0) net = 2;
            }
            FreeMibTable(tab);
        }
        double t = Now(), down = 0, up = 0;
        if (pT > 0 && t > pT) {
            down = inB >= pIn ? (inB - pIn) / (t - pT) : 0;
            up = outB >= pOut ? (outB - pOut) / (t - pT) : 0;
        }
        pIn = inB;
        pOut = outB;
        pT = t;

        SYSTEM_POWER_STATUS ps;
        if (GetSystemPowerStatus(&ps) && !(ps.BatteryFlag & 128) && ps.BatteryLifePercent <= 100) {
            s.battery = ps.BatteryLifePercent;
            s.charging = ps.ACLineStatus == 1;
        } else s.battery = -1;
        ULARGE_INTEGER fa, tot, fr;
        wchar_t sysDrive[MAX_PATH] = L"C:\\";
        GetWindowsDirectoryW(sysDrive, MAX_PATH);
        sysDrive[3] = 0;
        if (GetDiskFreeSpaceExW(sysDrive, &fa, &tot, &fr)) {
            s.diskFree = (double)fa.QuadPart;
            s.diskTotal = (double)tot.QuadPart;
        }
        {
            std::lock_guard<std::mutex> lk(g_sysMx);
            g_sys.cpuNow = cpu;
            g_sys.ramNow = ram;
            g_sys.ramUsed = s.ramUsed;
            g_sys.ramTotal = s.ramTotal;
            g_sys.gpuNow = gpu;
            g_sys.downNow = down;
            g_sys.upNow = up;
            g_sys.net = net;
            g_sys.battery = s.battery;
            g_sys.charging = s.charging;
            g_sys.diskFree = s.diskFree;
            g_sys.diskTotal = s.diskTotal;
            PushHist(g_sys.cpu, cpu);
            PushHist(g_sys.ram, ram);
            PushHist(g_sys.gpu, std::max(0.f, gpu));
            PushHist(g_sys.down, (float)down);
            PushHist(g_sys.up, (float)up);
        }
        if (!g_barOpen && q) {
            PdhCloseQuery(q);
            q = nullptr;
        }
    } while (WaitForSingleObject(g_quitEvent, 1000) == WAIT_TIMEOUT);
    if (q) PdhCloseQuery(q);
}
static void SysStart() {
    if (g_sysStarted.exchange(true)) return;
    std::thread(SysWorker).detach();
}
static SysState SysNow() {
    std::lock_guard<std::mutex> lk(g_sysMx);
    return g_sys;
}
static std::wstring Pct(float f) { return std::to_wstring((int)std::lround(std::clamp(f, 0.f, 1.f) * 100)) + L"%"; }
static std::wstring Speed(double bytesPerSec) {
    double bits = bytesPerSec * 8;
    wchar_t b[32];
    if (bits >= 1e9) swprintf(b, 32, L"%.2f Gbps", bits / 1e9);
    else if (bits >= 1e6) swprintf(b, 32, bits >= 1e8 ? L"%.0f Mbps" : L"%.1f Mbps", bits / 1e6);
    else if (bits >= 1e3) swprintf(b, 32, L"%.0f Kbps", bits / 1e3);
    else swprintf(b, 32, L"%.0f bps", bits);
    return b;
}
static std::wstring Uptime() {
    unsigned long long s = GetTickCount64() / 1000;
    unsigned long long d = s / 86400, h = s / 3600 % 24, m = s / 60 % 60;
    if (d) return std::to_wstring(d) + L"d " + std::to_wstring(h) + L"h";
    if (h) return std::to_wstring(h) + L"h " + std::to_wstring(m) + L"m";
    return std::to_wstring(m) + L" min";
}

// A stat card: label, big value, small note, and a graph.
static void StatCard(const D2D1_RECT_F& r, const std::wstring& label, const std::wstring& value, const std::wstring& note,
                     const std::vector<float>* hist, D2D1_COLOR_F col, float alpha, float maxV = 1.f) {
    const Theme& th = g_th;
    DrawCard(r, alpha);
    float x = r.left + 14, w = r.right - r.left - 28;
    DrawLayout(label, g_fmtSmall, x, r.top + 8, w, 18, WithA(th.dim, alpha));
    DrawLayout(value, g_fmtMid, x, r.top + 24, w, 28, WithA(th.text, alpha));
    if (!note.empty()) DrawLayout(note, g_fmtSmallR, x, r.top + 8, w, 18, WithA(th.dim, alpha));
    if (hist) {
        D2D1_RECT_F gr = D2D1::RectF(r.left + 1, r.top + 54, r.right - 1, r.bottom - 1);
        g_rt->PushAxisAlignedClip(gr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        std::vector<float> v = *hist;
        if (v.size() < 60) v.insert(v.begin(), 60 - v.size(), 0.f);
        DrawGraph(v, D2D1::RectF(gr.left, gr.top + 2, gr.right, gr.bottom - 2), col, alpha, maxV);
        g_rt->PopAxisAlignedClip();
    }
}
static float SystemHeight() { return 44.f + 2 * 96.f + 10.f + 14.f; }
static void DrawSystem(float top, float dt, float alpha, double now) {
    using namespace Lay;
    SysState s = SysNow();
    PanelHeader(L"This PC", L"Up " + Uptime(), top, alpha);
    float cw = (BarW - 2 * GridPad - CardGap) / 2, ch = 96.f;
    float x0 = Margin + GridPad, x1 = x0 + cw + CardGap, y0 = top + 44, y1 = y0 + ch + 10;
    wchar_t ram[64];
    swprintf(ram, 64, L"%.1f / %.0f GB", s.ramUsed / 1073741824.0, std::round(s.ramTotal / 1073741824.0));
    StatCard(D2D1::RectF(x0, y0, x0 + cw, y0 + ch), L"Processor", Pct(s.cpuNow), L"", &s.cpu, C(0x60CDFF), alpha);
    StatCard(D2D1::RectF(x1, y0, x1 + cw, y0 + ch), L"Memory", Pct(s.ramNow), ram, &s.ram, C(0xB48CFF), alpha);
    StatCard(D2D1::RectF(x0, y1, x0 + cw, y1 + ch), L"Graphics", s.gpuNow < 0 ? L"…" : Pct(s.gpuNow), L"", &s.gpu, C(0x5EE6A8), alpha);
    // storage + battery
    D2D1_RECT_F r = D2D1::RectF(x1, y1, x1 + cw, y1 + ch);
    const Theme& th = g_th;
    DrawCard(r, alpha);
    float x = r.left + 14, w = cw - 28;
    DrawLayout(L"Storage", g_fmtSmall, x, r.top + 8, w, 18, WithA(th.dim, alpha));
    DrawLayout(FormatBytes(s.diskFree) + L" free", g_fmtMid, x, r.top + 24, w, 28, WithA(th.text, alpha));
    float used = s.diskTotal > 0 ? (float)(1 - s.diskFree / s.diskTotal) : 0;
    g_solid->SetColor(WithA(th.dim, alpha * 0.25f));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, r.top + 58, x + w, r.top + 64), 3, 3), g_solid);
    g_solid->SetColor(WithA(used > 0.9f ? C(0xFF6B6B) : C(0xFFB648), alpha));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x, r.top + 58, x + w * used, r.top + 64), 3, 3), g_solid);
    std::wstring bat = s.battery < 0 ? L"Plugged in, no battery"
                                     : L"Battery " + std::to_wstring(s.battery) + L"%" + (s.charging ? L" · charging" : L"");
    DrawLayout(bat, g_fmtSmall, x, r.top + 70, w, 18, WithA(th.dim, alpha));
}

static float NetworkHeight() { return 44.f + 112.f + 14.f; }
static void DrawNetwork(float top, float dt, float alpha, double now) {
    using namespace Lay;
    SysState s = SysNow();
    const wchar_t* kind = s.net == 1 ? L"Wi‑Fi" : s.net == 2 ? L"Ethernet" : L"Offline";
    PanelHeader(L"Network", std::wstring(kind) + (s.net ? L" · connected" : L""), top, alpha);
    float cw = (BarW - 2 * GridPad - CardGap) / 2, ch = 112.f;
    float x0 = Margin + GridPad, x1 = x0 + cw + CardGap, y0 = top + 44;
    float mx = 125000.f;  // 1 Mbps
    for (float v : s.down) mx = std::max(mx, v);
    for (float v : s.up) mx = std::max(mx, v);
    StatCard(D2D1::RectF(x0, y0, x0 + cw, y0 + ch), L"↓  Download", Speed(s.downNow), L"", &s.down, C(0x60CDFF), alpha, mx * 1.15f);
    StatCard(D2D1::RectF(x1, y0, x1 + cw, y0 + ch), L"↑  Upload", Speed(s.upNow), L"", &s.up, C(0xFF9E64), alpha, mx * 1.15f);
}

// ======================================================================================
// Clock and calendar
// ======================================================================================
static int g_calMonthOff = 0;
static std::wstring TimeText(const SYSTEMTIME& st, bool seconds) {
    wchar_t b[64] = {};
    GetTimeFormatEx(LOCALE_NAME_USER_DEFAULT, seconds ? 0 : TIME_NOSECONDS, &st, nullptr, b, 64);
    return b;
}
static float CalendarHeight() { return 44.f + 7 * 28.f + 14.f; }
static void DrawCalendar(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    g_pbtn.clear();
    SYSTEMTIME st;
    GetLocalTime(&st);
    PanelHeader(L"Clock", L"← → month   ↵ today", top, alpha);
    float y0 = top + 44;
    // time, big, with the seconds ticking beside it
    std::wstring t = TimeText(st, false);
    DrawLayout(t, g_fmtBig, Margin + 24, y0 + 6, 260, 54, WithA(th.text, alpha));
    float tw = TextWidth(t, g_fmtBig);
    wchar_t sec[8];
    swprintf(sec, 8, L":%02d", st.wSecond);
    DrawLayout(sec, g_fmtMid, Margin + 26 + tw, y0 + 24, 60, 30, WithA(th.dim, alpha));
    wchar_t date[128] = {};
    GetDateFormatEx(LOCALE_NAME_USER_DEFAULT, DATE_LONGDATE, &st, nullptr, date, 128, nullptr);
    DrawLayout(date, g_fmtRow, Margin + 26, y0 + 62, 280, 22, WithA(th.text, alpha * 0.85f));
    // day of the year
    SYSTEMTIME jan = st;
    jan.wMonth = 1;
    jan.wDay = 1;
    FILETIME a, b;
    SystemTimeToFileTime(&jan, &a);
    SystemTimeToFileTime(&st, &b);
    int doy = (int)((FtU(b) - FtU(a)) / 864000000000ULL) + 1;
    DrawLayout(L"Day " + std::to_wstring(doy) + L" of the year", g_fmtSmall, Margin + 26, y0 + 86, 280, 18, WithA(th.dim, alpha));
    // a little analog dial under it
    float dcx = Margin + 80, dcy = y0 + 160, dr = 36;
    g_solid->SetColor(WithA(th.highlight, alpha * 1.4f));
    g_rt->FillEllipse(D2D1::Ellipse({dcx, dcy}, dr, dr), g_solid);
    for (int i = 0; i < 12; i++) {
        float an = i * kPI / 6;
        g_solid->SetColor(WithA(th.dim, alpha * 0.6f));
        g_rt->FillEllipse(D2D1::Ellipse({dcx + std::sin(an) * (dr - 6), dcy - std::cos(an) * (dr - 6)}, 1.3f, 1.3f), g_solid);
    }
    double secs = st.wSecond + st.wMilliseconds / 1000.0;
    float ah = (float)((st.wHour % 12 + st.wMinute / 60.0) * kPI / 6), am = (float)((st.wMinute + secs / 60) * kPI / 30),
          as = (float)(secs * kPI / 30);
    auto hand = [&](float an, float len, float wdt, D2D1_COLOR_F c) {
        g_solid->SetColor(c);
        ID2D1StrokeStyle* round = nullptr;
        D2D1_STROKE_STYLE_PROPERTIES sp = D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND);
        g_d2d->CreateStrokeStyle(sp, nullptr, 0, &round);
        g_rt->DrawLine({dcx, dcy}, {dcx + std::sin(an) * len, dcy - std::cos(an) * len}, g_solid, wdt, round);
        SafeRelease(round);
    };
    hand(ah, dr * 0.5f, 3, WithA(th.text, alpha));
    hand(am, dr * 0.75f, 2, WithA(th.text, alpha));
    hand(as, dr * 0.82f, 1, WithA(th.accent, alpha));
    g_solid->SetColor(WithA(th.accent, alpha));
    g_rt->FillEllipse(D2D1::Ellipse({dcx, dcy}, 2.5f, 2.5f), g_solid);

    // the month
    int y = st.wYear, mo = st.wMonth - 1 + g_calMonthOff;
    y += (int)std::floor(mo / 12.0);
    mo = ((mo % 12) + 12) % 12;
    SYSTEMTIME first = {};
    first.wYear = (WORD)y;
    first.wMonth = (WORD)(mo + 1);
    first.wDay = 1;
    FILETIME ft;
    SystemTimeToFileTime(&first, &ft);
    FileTimeToSystemTime(&ft, &first);  // fills in the day of the week
    static const int mdays[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    int days = mdays[mo] + (mo == 1 && ((y % 4 == 0 && y % 100 != 0) || y % 400 == 0));
    wchar_t fdw[4] = {};
    GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_IFIRSTDAYOFWEEK, fdw, 4);
    int firstDow = (_wtoi(fdw) + 1) % 7;  // locale: 0 = Monday; SYSTEMTIME: 0 = Sunday
    int lead = (first.wDayOfWeek - firstDow + 7) % 7;
    const float cellW = 38, cellH = 28, gx = Margin + BarW - 22 - 7 * cellW, gy = y0;
    wchar_t mname[64] = {};
    GetDateFormatEx(LOCALE_NAME_USER_DEFAULT, DATE_YEARMONTH, &first, nullptr, mname, 64, nullptr);
    DrawLayout(L"\xE76B", g_fmtGlyphSm, gx, gy, 24, cellH, WithA(th.dim, alpha));
    DrawLayout(mname, g_fmtRow, gx + 24, gy, 7 * cellW - 48, cellH, WithA(th.text, alpha));
    DrawLayout(L"\xE76C", g_fmtGlyphSm, gx + 7 * cellW - 24, gy, 24, cellH, WithA(th.dim, alpha));
    g_pbtn.push_back({D2D1::RectF(gx, gy, gx + 24, gy + cellH), -1});
    g_pbtn.push_back({D2D1::RectF(gx + 7 * cellW - 24, gy, gx + 7 * cellW, gy + cellH), 1});
    for (int d = 0; d < 7; d++) {
        wchar_t dn[16] = {};
        int lcDay = (firstDow + d + 6) % 7;  // LOCALE_SABBREVDAYNAME1 = Monday
        GetLocaleInfoEx(LOCALE_NAME_USER_DEFAULT, LOCALE_SSHORTESTDAYNAME1 + lcDay, dn, 16);
        DrawLayout(dn, g_fmtSmallC, gx + d * cellW, gy + cellH, cellW, cellH - 6, WithA(th.dim, alpha));
    }
    for (int dday = 1; dday <= days; dday++) {
        int idx = lead + dday - 1, row = idx / 7, col = idx % 7;
        float cx = gx + col * cellW, cy = gy + cellH * 2 - 4 + row * cellH;
        bool today = g_calMonthOff == 0 && dday == st.wDay;
        if (today) {
            g_solid->SetColor(WithA(th.accent, alpha));
            g_rt->FillEllipse(D2D1::Ellipse({cx + cellW / 2, cy + cellH / 2}, 12, 12), g_solid);
        }
        DrawLayout(std::to_wstring(dday), g_fmtSmallC, cx, cy, cellW, cellH,
                   WithA(today ? C(0xFFFFFF) : th.text, alpha * (today ? 1.f : 0.85f)));
    }
}
static bool CalendarKey(WPARAM vk, bool shift) {
    if (shift) return false;
    if (vk == VK_LEFT) { g_calMonthOff--; return true; }
    if (vk == VK_RIGHT) { g_calMonthOff++; return true; }
    if (vk == VK_UP) { g_calMonthOff -= 12; return true; }
    if (vk == VK_DOWN) { g_calMonthOff += 12; return true; }
    return false;
}
static bool CalendarClick(float x, float y) {
    int b = PanelButtonAt(x, y);
    if (b == -1 || b == 1) g_calMonthOff += b;
    return true;
}

// ======================================================================================
// Timer and stopwatch
// ======================================================================================
static bool g_tmSet = false, g_tmRun = false;
static double g_tmEnd = 0, g_tmLeft = 0, g_tmDur = 0;
static std::wstring g_tmArg;  // what started it ("5m")
static void PlayChime();
static void TrayBalloon(const wchar_t* title, const std::wstring& text);

// "90s", "5m", "1h30m", "5:00", "1:30:00", "2.5" (minutes)
static double ParseDuration(const std::wstring& s0) {
    std::wstring s = Lower(Trim(s0));
    if (s.empty()) return -1;
    if (s.find(L':') != std::wstring::npos) {
        double total = 0;
        size_t p = 0;
        int parts = 0;
        while (p <= s.size()) {
            size_t e = s.find(L':', p);
            if (e == std::wstring::npos) e = s.size();
            std::wstring n = s.substr(p, e - p);
            if (n.empty() || n.find_first_not_of(L"0123456789") != std::wstring::npos) return -1;
            total = total * 60 + _wtoi(n.c_str());
            parts++;
            p = e + 1;
        }
        return parts <= 3 && total > 0 ? total : -1;
    }
    double total = 0;
    size_t i = 0;
    bool any = false;
    while (i < s.size()) {
        while (i < s.size() && s[i] == L' ') i++;
        size_t st = i;
        while (i < s.size() && (std::iswdigit(s[i]) || s[i] == L'.')) i++;
        if (st == i) return -1;
        double v = _wtof(s.substr(st, i - st).c_str());
        while (i < s.size() && s[i] == L' ') i++;
        size_t us = i;
        while (i < s.size() && std::iswalpha(s[i])) i++;
        std::wstring u = s.substr(us, i - us);
        if (u.empty() || u == L"m" || u == L"min" || u == L"mins" || u == L"minute" || u == L"minutes") total += v * 60;
        else if (u == L"s" || u == L"sec" || u == L"secs" || u == L"second" || u == L"seconds") total += v;
        else if (u == L"h" || u == L"hr" || u == L"hrs" || u == L"hour" || u == L"hours") total += v * 3600;
        else return -1;
        any = true;
    }
    return any && total > 0 ? total : -1;
}
static double TimerLeft() {
    if (!g_tmSet) return 0;
    return g_tmRun ? std::max(0.0, g_tmEnd - Now()) : g_tmLeft;
}
static void TimerStart(double secs, const std::wstring& arg) {
    g_tmSet = g_tmRun = true;
    g_tmDur = secs;
    g_tmEnd = Now() + secs;
    g_tmArg = Lower(arg);
    SetTimer(g_hwnd, TIMER_TICK, 250, nullptr);
}
static void TimerToggle() {
    if (!g_tmSet) return;
    if (g_tmRun) { g_tmLeft = std::max(0.0, g_tmEnd - Now()); g_tmRun = false; }
    else { g_tmEnd = Now() + g_tmLeft; g_tmRun = true; SetTimer(g_hwnd, TIMER_TICK, 250, nullptr); }
}
static void TimerReset() {
    g_tmSet = g_tmRun = false;
    g_tmArg.clear();
}
// From WM_TIMER while a timer runs.
static void TimerTick() {
    if (!g_tmSet || !g_tmRun) { KillTimer(g_hwnd, TIMER_TICK); return; }
    if (Now() < g_tmEnd) return;
    std::wstring what = Clock(g_tmDur);
    TimerReset();
    KillTimer(g_hwnd, TIMER_TICK);
    PlayChime();
    TrayBalloon(L"Time's up", what + L" timer finished");
    if (g_visible) Flash(L"Time's up!");
}
static void DrawRing(float cx, float cy, float r, float frac, float width, D2D1_COLOR_F track, D2D1_COLOR_F col) {
    g_solid->SetColor(track);
    g_rt->DrawEllipse(D2D1::Ellipse({cx, cy}, r, r), g_solid, width);
    frac = std::clamp(frac, 0.f, 1.f);
    if (frac <= 0.001f) return;
    ID2D1PathGeometry* p = nullptr;
    if (FAILED(g_d2d->CreatePathGeometry(&p))) return;
    ID2D1GeometrySink* s = nullptr;
    if (SUCCEEDED(p->Open(&s))) {
        float a1 = -kPI / 2 + frac * 2 * kPI;
        s->BeginFigure({cx, cy - r}, D2D1_FIGURE_BEGIN_HOLLOW);
        if (frac >= 0.999f) {
            s->AddArc(D2D1::ArcSegment({cx, cy + r}, {r, r}, 0, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
            s->AddArc(D2D1::ArcSegment({cx, cy - r}, {r, r}, 0, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_ARC_SIZE_SMALL));
        } else
            s->AddArc(D2D1::ArcSegment({cx + std::cos(a1) * r, cy + std::sin(a1) * r}, {r, r}, 0, D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                       frac > 0.5f ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
        s->EndFigure(D2D1_FIGURE_END_OPEN);
        s->Close();
        s->Release();
    }
    ID2D1StrokeStyle* round = nullptr;
    g_d2d->CreateStrokeStyle(D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND), nullptr, 0, &round);
    g_solid->SetColor(col);
    g_rt->DrawGeometry(p, g_solid, width, round);
    SafeRelease(round);
    p->Release();
}
static float TimerHeight() { return 172.f; }
static void DrawTimer(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    double arg = ParseDuration(g_panelArg);
    bool fresh = arg > 0 && (!g_tmSet || Lower(g_panelArg) != g_tmArg);
    PanelHeader(L"Timer", fresh ? L"↵ start" : g_tmSet ? (g_tmRun ? L"↵ pause   Del reset" : L"↵ resume   Del reset") : L"", top, alpha);
    float cx = Margin + 86, cy = top + 98, r = 50;
    double left = fresh ? arg : TimerLeft();
    float frac = fresh ? 1.f : g_tmSet ? (float)(left / std::max(1.0, g_tmDur)) : 0.f;
    DrawRing(cx, cy, r, frac, 6, WithA(th.dim, alpha * 0.2f), WithA(g_tmRun || fresh ? th.accent : th.dim, alpha));
    DrawLayout(g_tmRun ? L"\xE916" : L"\xE768", g_fmtGlyph, cx - 20, cy - 20, 40, 40, WithA(th.dim, alpha));
    float tx = Margin + 170;
    if (!g_tmSet && !fresh) {
        DrawLayout(L"0:00", g_fmtBig, tx, top + 46, 300, 54, WithA(th.dim, alpha));
        DrawLayout(L"Type a time after > timer, like 5m, 90s, 1h30m or 10:00", g_fmtRow, tx, top + 104, BarW - 190, 22,
                   WithA(th.dim, alpha));
        return;
    }
    DrawLayout(Clock(std::ceil(left)), g_fmtBig, tx, top + 46, 300, 54, WithA(th.text, alpha));
    std::wstring note;
    if (fresh) note = L"Press Enter to start";
    else if (g_tmRun) {
        SYSTEMTIME st;
        GetLocalTime(&st);
        FILETIME ft;
        SystemTimeToFileTime(&st, &ft);
        ULARGE_INTEGER u{{ft.dwLowDateTime, ft.dwHighDateTime}};
        u.QuadPart += (unsigned long long)(left * 1e7);
        ft = {u.LowPart, u.HighPart};
        FileTimeToSystemTime(&ft, &st);
        note = L"Ends at " + TimeText(st, false) + L" · you'll hear a chime";
    } else note = L"Paused";
    DrawLayout(note, g_fmtRow, tx, top + 104, BarW - 190, 22, WithA(th.dim, alpha));
}
static void TimerEnter(int, bool) {
    double arg = ParseDuration(g_panelArg);
    if (arg > 0 && (!g_tmSet || Lower(g_panelArg) != g_tmArg)) TimerStart(arg, g_panelArg);
    else TimerToggle();
}
static bool TimerKey(WPARAM vk, bool) {
    if (vk == VK_DELETE && g_caret == (int)g_text.size() && g_tmSet) { TimerReset(); return true; }
    return false;
}

static bool g_swRun = false;
static double g_swStart = 0, g_swAccum = 0;
static std::vector<double> g_swLaps;
static double StopwatchNow() { return g_swAccum + (g_swRun ? Now() - g_swStart : 0); }
static std::wstring SwText(double t, bool cs = true) {
    int ms = (int)(t * 100);
    wchar_t b[32];
    if (t >= 3600) swprintf(b, 32, L"%d:%02d:%02d", ms / 360000, ms / 6000 % 60, ms / 100 % 60);
    else if (cs) swprintf(b, 32, L"%02d:%02d.%02d", ms / 6000, ms / 100 % 60, ms % 100);
    else swprintf(b, 32, L"%02d:%02d", ms / 6000, ms / 100 % 60);
    return b;
}
static float StopwatchHeight() { return 172.f; }
static void DrawStopwatch(float top, float dt, float alpha, double now) {
    using namespace Lay;
    const Theme& th = g_th;
    PanelHeader(L"Stopwatch", g_swRun ? L"↵ pause   ⇧↵ lap   Del reset" : L"↵ start   Del reset", top, alpha);
    double t = StopwatchNow();
    float cx = Margin + 86, cy = top + 98, r = 50;
    DrawRing(cx, cy, r, (float)std::fmod(t, 60.0) / 60.f, 6, WithA(th.dim, alpha * 0.2f), WithA(g_swRun ? th.accent : th.dim, alpha));
    DrawLayout(g_swRun ? L"\xE916" : L"\xE768", g_fmtGlyph, cx - 20, cy - 20, 40, 40, WithA(th.dim, alpha));
    DrawLayout(SwText(t), g_fmtBig, Margin + 170, top + 46, 260, 54, WithA(t > 0 ? th.text : th.dim, alpha));
    DrawLayout(g_swRun ? L"Running" : t > 0 ? L"Paused" : L"Press Enter to start", g_fmtRow, Margin + 170, top + 104, 220, 22,
               WithA(th.dim, alpha));
    // laps, newest first
    float lx = Margin + BarW - 190;
    int n = (int)g_swLaps.size();
    for (int i = 0; i < std::min(n, 4); i++) {
        int k = n - 1 - i;
        double lap = g_swLaps[k] - (k ? g_swLaps[k - 1] : 0);
        float ly = top + 46 + i * 26;
        DrawLayout(L"Lap " + std::to_wstring(k + 1), g_fmtSmall, lx, ly, 60, 22, WithA(th.dim, alpha));
        DrawLayout(SwText(lap), g_fmtRowRight, lx + 50, ly, 120, 22, WithA(th.text, alpha * (i ? 0.75f : 1.f)));
    }
}
static void StopwatchEnter(int, bool shift) {
    if (shift) {
        if (g_swRun) g_swLaps.push_back(StopwatchNow());
        return;
    }
    if (g_swRun) { g_swAccum = StopwatchNow(); g_swRun = false; }
    else { g_swStart = Now(); g_swRun = true; }
}
static bool StopwatchKey(WPARAM vk, bool) {
    if (vk == VK_DELETE && g_caret == (int)g_text.size()) {
        g_swRun = false;
        g_swAccum = 0;
        g_swLaps.clear();
        return true;
    }
    return false;
}

// ======================================================================================
// Volume and brightness
// ======================================================================================
static IAudioEndpointVolume* EndpointVolume() {
    IMMDeviceEnumerator* en = nullptr;
    IMMDevice* dev = nullptr;
    IAudioEndpointVolume* v = nullptr;
    if (SUCCEEDED(CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL, IID_PPV_ARGS(&en))) &&
        SUCCEEDED(en->GetDefaultAudioEndpoint(eRender, eConsole, &dev)))
        dev->Activate(__uuidof(IAudioEndpointVolume), CLSCTX_ALL, nullptr, (void**)&v);
    SafeRelease(dev);
    SafeRelease(en);
    return v;
}
static int g_volShown = -1;  // cached for drawing, refreshed by the panel
static bool g_muteShown = false;
static double g_volReadAt = -10;
static void VolumeRead(bool force = false) {
    if (!force && Now() - g_volReadAt < 0.5) return;
    g_volReadAt = Now();
    if (IAudioEndpointVolume* v = EndpointVolume()) {
        float f = 0;
        BOOL m = FALSE;
        v->GetMasterVolumeLevelScalar(&f);
        v->GetMute(&m);
        g_volShown = (int)std::lround(f * 100);
        g_muteShown = m != FALSE;
        v->Release();
    } else g_volShown = -1;
}
static void VolumeSet(int pct) {
    pct = std::clamp(pct, 0, 100);
    if (IAudioEndpointVolume* v = EndpointVolume()) {
        v->SetMasterVolumeLevelScalar(pct / 100.f, nullptr);
        if (pct > 0) v->SetMute(FALSE, nullptr);
        v->Release();
    }
    VolumeRead(true);
}
static void VolumeMute() {
    if (IAudioEndpointVolume* v = EndpointVolume()) {
        BOOL m = FALSE;
        v->GetMute(&m);
        v->SetMute(!m, nullptr);
        v->Release();
    }
    VolumeRead(true);
}

// Brightness goes through WMI (built-in screens only), on its own thread.
static std::atomic<int> g_bright{-2};      // -2 not read yet, -1 can't be changed
static std::atomic<int> g_brightWant{-1};  // a change waiting to be sent
static std::atomic<bool> g_brightBusy{false};
static void BrightnessWorker() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IWbemLocator* loc = nullptr;
    IWbemServices* svc = nullptr;
    BSTR ns = SysAllocString(L"ROOT\\WMI");
    if (SUCCEEDED(CoCreateInstance(CLSID_WbemLocator, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&loc))) &&
        SUCCEEDED(loc->ConnectServer(ns, nullptr, nullptr, nullptr, 0, nullptr, nullptr, &svc))) {
        CoSetProxyBlanket(svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, nullptr, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
                          nullptr, EOAC_NONE);
        BSTR wql = SysAllocString(L"WQL");
        auto first = [&](const wchar_t* query) -> IWbemClassObject* {
            BSTR q = SysAllocString(query);
            IEnumWbemClassObject* en = nullptr;
            IWbemClassObject* obj = nullptr;
            ULONG got = 0;
            if (SUCCEEDED(svc->ExecQuery(wql, q, WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, nullptr, &en)) && en)
                en->Next(3000, 1, &obj, &got);
            SafeRelease(en);
            SysFreeString(q);
            return got ? obj : nullptr;
        };
        do {
            int want = g_brightWant.exchange(-1);
            if (want >= 0) {
                if (IWbemClassObject* inst = first(L"SELECT * FROM WmiMonitorBrightnessMethods WHERE Active=TRUE")) {
                    VARIANT path;
                    VariantInit(&path);
                    IWbemClassObject *cls = nullptr, *sig = nullptr, *in = nullptr;
                    BSTR clsName = SysAllocString(L"WmiMonitorBrightnessMethods"), method = SysAllocString(L"WmiSetBrightness");
                    if (SUCCEEDED(inst->Get(L"__PATH", 0, &path, nullptr, nullptr)) &&
                        SUCCEEDED(svc->GetObject(clsName, 0, nullptr, &cls, nullptr)) &&
                        SUCCEEDED(cls->GetMethod(method, 0, &sig, nullptr)) && SUCCEEDED(sig->SpawnInstance(0, &in))) {
                        VARIANT v;
                        VariantInit(&v);
                        v.vt = VT_I4;
                        v.lVal = 0;
                        in->Put(L"Timeout", 0, &v, 0);
                        v.vt = VT_UI1;
                        v.bVal = (BYTE)std::clamp(want, 0, 100);
                        in->Put(L"Brightness", 0, &v, 0);
                        svc->ExecMethod(path.bstrVal, method, 0, nullptr, in, nullptr, nullptr);
                    }
                    SafeRelease(in);
                    SafeRelease(sig);
                    SafeRelease(cls);
                    SysFreeString(clsName);
                    SysFreeString(method);
                    VariantClear(&path);
                    inst->Release();
                }
            }
            if (IWbemClassObject* o = first(L"SELECT CurrentBrightness FROM WmiMonitorBrightness WHERE Active=TRUE")) {
                VARIANT v;
                VariantInit(&v);
                if (SUCCEEDED(o->Get(L"CurrentBrightness", 0, &v, nullptr, nullptr)))
                    g_bright = v.vt == VT_UI1 ? v.bVal : v.vt == VT_I4 ? v.lVal : -1;
                VariantClear(&v);
                o->Release();
            } else g_bright = -1;
            PostMessageW(g_hwnd, WM_APP_PANEL, 0, 0);
        } while (g_brightWant >= 0);
        SysFreeString(wql);
    } else g_bright = -1;
    SafeRelease(svc);
    SafeRelease(loc);
    SysFreeString(ns);
    g_brightBusy = false;
    CoUninitialize();
    if (g_brightWant >= 0 && !g_brightBusy.exchange(true)) std::thread(BrightnessWorker).detach();
}
static void BrightnessKick() {
    if (!g_brightBusy.exchange(true)) std::thread(BrightnessWorker).detach();
}
static void BrightnessSet(int pct) {
    pct = std::clamp(pct, 0, 100);
    g_brightWant = pct;
    if (g_bright >= 0) g_bright = pct;  // shown at once
    BrightnessKick();
}

// The shared slider panel. which: 0 volume, 1 brightness.
static float SliderHeight() { return 128.f; }
static D2D1_RECT_F g_sliderTrack;
static void DrawSlider(int which, float top, float alpha) {
    using namespace Lay;
    const Theme& th = g_th;
    int v = which == 0 ? g_volShown : g_bright.load();
    bool muted = which == 0 && g_muteShown;
    double arg = -1;
    if (!g_panelArg.empty() && g_panelArg.find_first_not_of(L"0123456789%") == std::wstring::npos) arg = _wtoi(g_panelArg.c_str());
    PanelHeader(which == 0 ? L"Volume" : L"Brightness",
                arg >= 0 ? L"↵ set " + std::to_wstring((int)std::min(100.0, arg)) + L"%" : which == 0 ? L"← → adjust   ↵ mute" : L"← → adjust",
                top, alpha);
    float y0 = top + 44;
    if (v < 0) {
        const wchar_t* msg = which == 0 ? L"No speakers or headphones found"
                             : v == -2  ? L"Checking the screen…"
                                        : L"This screen's brightness can't be changed here; external monitors use their own buttons";
        DrawLayout(which == 0 ? L"\xE74F" : L"\xE706", g_fmtBigC, Margin + 14, y0, 70, 64, WithA(th.dim, alpha));
        DrawWrapped(msg, g_fmtRow, Margin + 90, y0 + 8, BarW - 110, 48, WithA(th.dim, alpha));
        g_sliderTrack = {};
        return;
    }
    const wchar_t* glyph = which == 1 ? L"\xE706" : muted || v == 0 ? L"\xE74F" : v < 33 ? L"\xE993" : v < 66 ? L"\xE994" : L"\xE995";
    DrawLayout(glyph, g_fmtBigC, Margin + 14, y0, 70, 64, WithA(muted ? th.dim : th.text, alpha));
    float tx = Margin + 92, tw = BarW - 92 - 116, ty = y0 + 30;
    g_sliderTrack = D2D1::RectF(tx, ty - 12, tx + tw, ty + 12);
    float f = v / 100.f;
    g_solid->SetColor(WithA(th.dim, alpha * 0.25f));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(tx, ty - 3, tx + tw, ty + 3), 3, 3), g_solid);
    g_solid->SetColor(WithA(muted ? th.dim : th.accent, alpha));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(tx, ty - 3, tx + tw * f, ty + 3), 3, 3), g_solid);
    g_rt->FillEllipse(D2D1::Ellipse({tx + tw * f, ty}, 9, 9), g_solid);
    g_solid->SetColor(WithA(C(0xFFFFFF), alpha));
    g_rt->FillEllipse(D2D1::Ellipse({tx + tw * f, ty}, 4, 4), g_solid);
    DrawLayout(muted ? L"Muted" : std::to_wstring(v) + L"%", g_fmtMid, Margin + BarW - 86, y0 + 14, 70, 32, WithA(th.text, alpha));
}
static bool SliderClick(int which, float x, float y) {
    const D2D1_RECT_F& r = g_sliderTrack;
    if (r.right <= r.left || y < r.top - 6 || y > r.bottom + 6 || x < r.left - 10 || x > r.right + 10) return true;
    int pct = (int)std::lround(std::clamp((x - r.left) / (r.right - r.left), 0.f, 1.f) * 100);
    if (which == 0) VolumeSet(pct);
    else BrightnessSet(pct);
    return true;
}
static bool SliderKey(int which, WPARAM vk, bool shift) {
    int step = shift ? 10 : 2;
    int cur = which == 0 ? g_volShown : g_bright.load();
    if (cur < 0) return false;
    int d = vk == VK_LEFT || vk == VK_DOWN ? -step : vk == VK_RIGHT || vk == VK_UP ? step : 0;
    if (!d) return false;
    if (which == 0) VolumeSet(cur + d);
    else BrightnessSet(cur + d);
    return true;
}
static void SliderEnter(int which) {
    std::wstring a = g_panelArg;
    if (!a.empty() && a.back() == L'%') a.pop_back();
    if (!a.empty() && a.find_first_not_of(L"0123456789") == std::wstring::npos) {
        int pct = std::min(100, _wtoi(a.c_str()));
        if (which == 0) VolumeSet(pct);
        else BrightnessSet(pct);
        Flash((which == 0 ? L"Volume " : L"Brightness ") + std::to_wstring(pct) + L"%");
        return;
    }
    if (which == 0) VolumeMute();
}

// ======================================================================================
// Status strip: what's playing, a running timer, the weather and the time, on the right
// of the empty bar. Each opens its panel when clicked.
// ======================================================================================
struct StripHit {
    D2D1_RECT_F r;
    std::wstring cmd;
};
static std::vector<StripHit> g_stripHits;
static bool g_stripOn = true;  // "> layout" can turn it off

static void DrawStatusStrip(float leftLimit, float rightEdge, float alpha, double now) {
    g_stripHits.clear();
    if (!g_stripOn || alpha < 0.02f) return;
    using namespace Lay;
    const Theme& th = g_th;
    const float cy = Margin + BarH / 2, h = 22;
    float x = rightEdge;
    auto item = [&](const std::wstring& text, const wchar_t* glyph, int wxIcon, const wchar_t* cmd, float maxW, D2D1_COLOR_F col) {
        float tw = std::min(maxW, TextWidth(text, g_fmtHint));
        float iw = glyph || wxIcon >= 0 ? 20.f : 0.f;
        float w = tw + iw;
        if (x - w < leftLimit + 10) return false;
        float x0 = x - w;
        if (wxIcon >= 0) DrawWeatherIcon(wxIcon, x0 + 8, cy, 20, alpha, now);
        else if (glyph) DrawLayout(glyph, g_fmtGlyphSm, x0, cy - h / 2, 16, h, WithA(th.accent, alpha));
        DrawLayout(text, g_fmtHintL, x0 + iw, cy - h / 2, tw + 2, h, WithA(col, alpha));
        g_stripHits.push_back({D2D1::RectF(x0 - 4, cy - 14, x + 4, cy + 14), cmd});
        x = x0 - 16;
        return true;
    };
    SYSTEMTIME st;
    GetLocalTime(&st);
    item(TimeText(st, false), nullptr, -1, L"clock", 120, th.dim);
    WeatherState w = WeatherNow();
    if (w.ok) item(Deg(w.temp), nullptr, WeatherIcon(w.code, w.day), L"weather", 60, th.dim);
    if (g_tmSet) item(Clock(std::ceil(TimerLeft())), L"\xE916", -1, L"timer", 80, g_tmRun ? th.text : th.dim);
    else if (g_swRun) item(SwText(StopwatchNow(), false), L"\xE916", -1, L"stopwatch", 80, th.text);
    wb::MediaInfo m = MediaNow(nullptr);
    if (m.has && m.playing && !m.title.empty()) item(m.title, L"\xE8D6", -1, L"media", 170, th.dim);
}
static int StripAt(float x, float y) {
    for (int i = 0; i < (int)g_stripHits.size(); i++) {
        auto& r = g_stripHits[i].r;
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return i;
    }
    return -1;
}

static void ReleaseWidgetBitmaps() {
    SafeRelease(g_artBmp);
    g_artBmpVer = -1;
}

// Starts the background readers when the bar opens.
static void WidgetsOnShow() {
    if (!g_mediaEvent) {
        g_mediaEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        std::thread(MediaWorker).detach();
    }
    MediaWake();
    SysStart();
    WeatherMaybeRefresh();
}
