// ---------------------------------------------------------------------------
// Fonts ("> font"): a catalog of free fonts shown as preview cards, grouped by category.
// Nothing is downloaded until the card's Download button is clicked (or Enter is pressed
// on it); the pictures on the cards are small images from this project's repository.
// Included by main.cpp after the text resources.
// ---------------------------------------------------------------------------
#include <winhttp.h>
#include <wincodec.h>
#include <functional>
#pragma comment(lib, "winhttp.lib")
#pragma comment(lib, "windowscodecs.lib")

static void Select(int idx);

static const wchar_t* kPreviewBase = L"https://raw.githubusercontent.com/mysisnotreal/CustomStartMenu/main/fonts/previews/";
static const wchar_t* kFontSample = L"The quick brown fox jumps over the lazy dog";
static const wchar_t* kLocalCat = L"Downloaded";  // fonts in the Fonts folder are listed first, under this

struct FontItem {
    std::wstring cat, name, lower, id, url, file;  // file: its name in the Fonts folder
    double size = 0;
};
static std::vector<FontItem> g_fontCat;  // the catalog, then fonts that are only on this PC
static size_t g_fontCatN = 0;            // how many come from the catalog
static std::wstring g_fontFilter;        // what was typed after "> font"
static std::vector<int> g_fontList;      // what the panel shows (indices into g_fontCat)
static std::set<std::wstring> g_fontLocal;  // lower-case file names in the Fonts folder
static CardGrid g_fontGrid{3, 64.f, 26.f};  // cards in rows of three, by category (cell item = position in g_fontList)
static std::vector<HitItem> g_fontBtnHits;
static D2D1_RECT_F g_fontDefaultHit{};

static std::mutex g_fontMx;  // guards the four below (worker threads write them)
static std::unordered_map<std::wstring, float> g_fontProgress;  // id -> 0..1 while downloading
static std::set<std::wstring> g_fontFailed;
struct PreviewEntry {
    bool ready = false, failed = false;
    std::vector<UINT32> px;
    int w = 0, h = 0;
    bool mask = true;  // gray coverage (drawn in the theme's text color) rather than a color picture
    ID2D1Bitmap* bmp = nullptr;
    double shownAt = 0;
};
static std::unordered_map<std::wstring, PreviewEntry> g_previews;
static std::deque<std::wstring> g_previewQueue;
static std::condition_variable g_previewCv;
static bool g_previewQuit = false;
static std::vector<std::thread> g_previewThreads;
static std::unordered_map<std::wstring, IDWriteTextFormat*> g_localFontFmt;  // live previews of fonts not in the catalog

// ---- names -------------------------------------------------------------------
// "SF-Pro-Display-BlackItalic.otf" -> "SF Pro Display Black Italic": drop the extension, split
// on - and _, split joined words (camelCase, acronyms, a trailing number) and spell out
// style abbreviations. The catalog's names were made the same way.
static void ReplaceAll(std::wstring& s, const std::wstring& a, const std::wstring& b) {
    for (size_t p = s.find(a); p != std::wstring::npos; p = s.find(a, p + b.size())) s.replace(p, a.size(), b);
}
static std::wstring CleanFontName(std::wstring s) {
    size_t dot = s.find_last_of(L'.');
    if (dot != std::wstring::npos) {
        std::wstring ext = Lower(s.substr(dot));
        if (ext == L".otf" || ext == L".ttf") s.resize(dot);
    }
    if (s.size() > 2 && s.compare(s.size() - 2, 2, L"It") == 0 &&
        (std::iswlower(s[s.size() - 3]) || s[s.size() - 3] == L'-'))
        s += L"alic";
    std::wstring r;
    for (size_t i = 0; i < s.size(); i++) {
        wchar_t c = s[i];
        if (c == L'-' || c == L'_' || c == L' ') {
            if (!r.empty() && r.back() != L' ') r += L' ';
            continue;
        }
        if (i > 0 && !r.empty() && r.back() != L' ') {
            wchar_t p = s[i - 1];
            bool digitsToEnd = false;
            if (std::iswdigit(c)) {
                size_t j = i;
                while (j < s.size() && std::iswdigit(s[j])) j++;
                digitsToEnd = j == s.size() || s[j] == L'-' || s[j] == L'_' || s[j] == L' ';
            }
            if ((std::iswlower(p) && std::iswupper(c)) ||
                (std::iswupper(p) && std::iswupper(c) && i + 1 < s.size() && std::iswlower(s[i + 1])) ||
                (std::iswlower(p) && digitsToEnd))
                r += L' ';
        }
        r += c;
    }
    r = Trim(r);
    ReplaceAll(r, L"Deja Vu", L"DejaVu");
    ReplaceAll(r, L"Math Jax", L"MathJax");
    ReplaceAll(r, L"Semibold", L"Semi Bold");
    ReplaceAll(r, L"Extralight", L"Extra Light");
    ReplaceAll(r, L"Ultralight", L"Ultra Light");
    ReplaceAll(r, L"Regular Italic", L"Italic");
    return r;
}

// ---- cache ---------------------------------------------------------------------
static std::wstring PreviewDir() { return KnownFolderPath(FOLDERID_UserProgramFiles) + L"\\CustomStartMenu\\Cache\\FontPreviews"; }

// ---- catalog ------------------------------------------------------------------
static std::wstring Utf8ToWide(const char* s, size_t n) {
    int len = MultiByteToWideChar(CP_UTF8, 0, s, (int)n, nullptr, 0);
    std::wstring w(len, L'\0');
    MultiByteToWideChar(CP_UTF8, 0, s, (int)n, &w[0], len);
    return w;
}

static void LoadFontCatalog() {
    HRSRC r = FindResourceW(nullptr, MAKEINTRESOURCEW(2), RT_RCDATA);
    HGLOBAL g = r ? LoadResource(nullptr, r) : nullptr;
    const char* data = g ? (const char*)LockResource(g) : nullptr;
    if (!data) return;
    std::wstring all = Utf8ToWide(data, SizeofResource(nullptr, r));
    size_t pos = 0;
    while (pos < all.size()) {
        size_t end = all.find(L'\n', pos);
        if (end == std::wstring::npos) end = all.size();
        std::wstring line = all.substr(pos, end - pos);
        pos = end + 1;
        if (!line.empty() && line.back() == L'\r') line.pop_back();
        std::vector<std::wstring> f;
        size_t a = 0;
        for (;;) {
            size_t t = line.find(L'\t', a);
            f.push_back(line.substr(a, t == std::wstring::npos ? std::wstring::npos : t - a));
            if (t == std::wstring::npos) break;
            a = t + 1;
        }
        if (f.size() < 5) continue;
        FontItem it;
        it.cat = f[0];
        it.name = f[1];
        it.lower = Lower(it.name + L" " + it.cat);
        it.id = f[2];
        it.url = f[3];
        it.file = it.url.substr(it.url.find_last_of(L'/') + 1);
        it.size = _wtof(f[4].c_str());
        g_fontCat.push_back(it);
    }
    g_fontCatN = g_fontCat.size();
}

static bool IsFontFile(const std::wstring& name) {
    std::wstring n = Lower(name);
    return n.size() > 4 && (n.compare(n.size() - 4, 4, L".otf") == 0 || n.compare(n.size() - 4, 4, L".ttf") == 0);
}

// Looks at the Fonts folder: which catalog fonts are downloaded, and which fonts were put
// there by hand (listed with the downloaded ones).
static void ScanLocalFonts() {
    g_fontLocal.clear();
    g_fontCat.resize(g_fontCatN);
    std::wstring dir = FontsDir();
    std::vector<std::wstring> files;
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) && IsFontFile(fd.cFileName)) files.push_back(fd.cFileName);
        } while (FindNextFileW(h, &fd));
        FindClose(h);
    }
    std::sort(files.begin(), files.end(), [](const std::wstring& a, const std::wstring& b) { return Lower(a) < Lower(b); });
    std::set<std::wstring> known;
    for (size_t i = 0; i < g_fontCatN; i++) known.insert(Lower(g_fontCat[i].file));
    for (auto& f : files) {
        g_fontLocal.insert(Lower(f));
        if (known.count(Lower(f))) continue;
        FontItem it;
        it.cat = kLocalCat;
        it.name = CleanFontName(f);
        it.lower = Lower(it.name + L" " + it.cat);
        it.id = L"local:" + Lower(f);
        it.file = f;
        g_fontCat.push_back(it);
    }
}

static bool FontIsLocal(const FontItem& it) { return g_fontLocal.count(Lower(it.file)) > 0; }
static bool FontInUse(const FontItem& it) { return !g_fontFile.empty() && Lower(it.file) == Lower(g_fontFile); }

static std::wstring FontDisplayName(const std::wstring& file) {
    if (file.empty()) return L"Default";
    for (auto& it : g_fontCat)
        if (Lower(it.file) == Lower(file)) return it.name;
    return CleanFontName(file);
}

// "> font NAME": every word has to appear in the font's name or category. Fonts that are
// already on this PC come first, under "Downloaded"; the rest keep their categories.
static void BuildFontList(const std::wstring& filter) {
    g_fontFilter = filter;
    ScanLocalFonts();
    std::vector<std::wstring> words;
    for (size_t a = 0; a < filter.size();) {
        size_t b = filter.find(L' ', a);
        if (b == std::wstring::npos) b = filter.size();
        if (b > a) words.push_back(filter.substr(a, b - a));
        a = b + 1;
    }
    std::vector<int> local, rest;
    for (int i = 0; i < (int)g_fontCat.size(); i++) {
        const FontItem& it = g_fontCat[i];
        bool here = FontIsLocal(it);
        bool ok = true;
        for (auto& w : words)
            if (it.lower.find(w) == std::wstring::npos && !(here && std::wstring(L"downloaded").find(w) == 0)) {
                ok = false;
                break;
            }
        if (ok) (here ? local : rest).push_back(i);
    }
    g_fontList = local;
    g_fontList.insert(g_fontList.end(), rest.begin(), rest.end());
    std::vector<std::wstring> cats;
    for (int i : g_fontList) cats.push_back(FontIsLocal(g_fontCat[i]) ? kLocalCat : g_fontCat[i].cat);
    g_fontGrid.Layout(cats);
}

static float FontPanelH() { return g_fontGrid.PanelH(); }
static void FontScrollTo(float off, bool instant) { g_fontGrid.ScrollTo(off, instant); }

// ---- downloads ------------------------------------------------------------------
// GET over HTTPS. With a path, the body goes to that file (via a temporary file, so a
// broken download never leaves half a font behind); otherwise into `out`.
static bool HttpGet(const std::wstring& url, std::string* out, const std::wstring& path,
                    const std::function<void(float)>& progress) {
    URL_COMPONENTS uc = {sizeof(uc)};
    wchar_t host[256] = {}, urlPath[2048] = {};
    uc.lpszHostName = host;
    uc.dwHostNameLength = 256;
    uc.lpszUrlPath = urlPath;
    uc.dwUrlPathLength = 2048;
    if (!WinHttpCrackUrl(url.c_str(), 0, 0, &uc)) return false;
    bool ok = false;
    HINTERNET s = WinHttpOpen(L"CustomStart/1.3", WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_NO_PROXY_NAME,
                              WINHTTP_NO_PROXY_BYPASS, 0);
    HINTERNET c = s ? WinHttpConnect(s, host, uc.nPort, 0) : nullptr;
    HINTERNET r = c ? WinHttpOpenRequest(c, L"GET", urlPath, nullptr, WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES,
                                         uc.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0)
                    : nullptr;
    if (r && WinHttpSendRequest(r, WINHTTP_NO_ADDITIONAL_HEADERS, 0, WINHTTP_NO_REQUEST_DATA, 0, 0, 0) &&
        WinHttpReceiveResponse(r, nullptr)) {
        DWORD status = 0, sz = sizeof(status);
        WinHttpQueryHeaders(r, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, nullptr, &status, &sz, nullptr);
        double total = 0;
        wchar_t len[32] = {};
        DWORD lsz = sizeof(len);
        if (WinHttpQueryHeaders(r, WINHTTP_QUERY_CONTENT_LENGTH, nullptr, len, &lsz, nullptr)) total = _wtof(len);
        if (status == 200) {
            std::wstring tmp = path + L".part";
            HANDLE f = path.empty() ? INVALID_HANDLE_VALUE
                                    : CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
            if (path.empty() || f != INVALID_HANDLE_VALUE) {
                ok = true;
                double got = 0;
                std::vector<char> buf(64 * 1024);
                for (;;) {
                    DWORD n = 0;
                    if (!WinHttpReadData(r, buf.data(), (DWORD)buf.size(), &n)) { ok = false; break; }
                    if (n == 0) break;
                    got += n;
                    if (out) out->append(buf.data(), n);
                    if (f != INVALID_HANDLE_VALUE) {
                        DWORD w = 0;
                        if (!WriteFile(f, buf.data(), n, &w, nullptr) || w != n) { ok = false; break; }
                    }
                    if (progress && total > 0) progress((float)std::min(1.0, got / total));
                }
                if (total > 0 && got < total) ok = false;
                if (f != INVALID_HANDLE_VALUE) {
                    CloseHandle(f);
                    if (ok) ok = MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) != FALSE;
                    if (!ok) DeleteFileW(tmp.c_str());
                }
            }
        }
    }
    if (r) WinHttpCloseHandle(r);
    if (c) WinHttpCloseHandle(c);
    if (s) WinHttpCloseHandle(s);
    return ok;
}

static void StartFontDownload(int item) {
    if (item < 0 || item >= (int)g_fontCat.size() || g_fontCat[item].url.empty()) return;
    FontItem it = g_fontCat[item];
    {
        std::lock_guard<std::mutex> lk(g_fontMx);
        if (g_fontProgress.count(it.id)) return;
        g_fontProgress[it.id] = 0.f;
        g_fontFailed.erase(it.id);
    }
    HWND hwnd = g_hwnd;
    std::thread([it, hwnd]() {
        std::wstring dir = FontsDir();
        SHCreateDirectoryExW(nullptr, dir.c_str(), nullptr);
        bool ok = HttpGet(it.url, nullptr, dir + L"\\" + it.file, [&](float p) {
            std::lock_guard<std::mutex> lk(g_fontMx);
            g_fontProgress[it.id] = p;
        });
        PostMessageW(hwnd, WM_APP_FONT, ok, (LPARAM) new std::wstring(it.id));
    }).detach();
}

// ---- preview pictures -------------------------------------------------------------
static bool ReadFileBytes(const std::wstring& path, std::string& out) {
    HANDLE f = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    LARGE_INTEGER sz{};
    bool ok = GetFileSizeEx(f, &sz) && sz.QuadPart > 0 && sz.QuadPart < (64 << 20);
    if (ok) {
        out.resize((size_t)sz.QuadPart);
        DWORD n = 0;
        ok = ReadFile(f, &out[0], (DWORD)out.size(), &n, nullptr) && n == out.size();
    }
    CloseHandle(f);
    return ok;
}
static void WriteFileBytes(const std::wstring& path, const std::string& data) {
    HANDLE f = CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
    if (f == INVALID_HANDLE_VALUE) return;
    DWORD n = 0;
    WriteFile(f, data.data(), (DWORD)data.size(), &n, nullptr);
    CloseHandle(f);
}

// PNG -> premultiplied BGRA. A grayscale picture is coverage: it becomes white with that
// alpha, and is drawn as an opacity mask in the theme's text color.
static bool DecodePreview(IWICImagingFactory* wic, const std::string& png, PreviewEntry& e) {
    IWICStream* st = nullptr;
    IWICBitmapDecoder* dec = nullptr;
    IWICBitmapFrameDecode* fr = nullptr;
    IWICFormatConverter* cv = nullptr;
    bool ok = false;
    if (SUCCEEDED(wic->CreateStream(&st)) &&
        SUCCEEDED(st->InitializeFromMemory((BYTE*)png.data(), (DWORD)png.size())) &&
        SUCCEEDED(wic->CreateDecoderFromStream(st, nullptr, WICDecodeMetadataCacheOnDemand, &dec)) &&
        SUCCEEDED(dec->GetFrame(0, &fr))) {
        WICPixelFormatGUID pf{};
        fr->GetPixelFormat(&pf);
        e.mask = IsEqualGUID(pf, GUID_WICPixelFormat8bppGray) != FALSE;
        UINT w = 0, h = 0;
        fr->GetSize(&w, &h);
        if (w && h && w <= 2048 && h <= 1024 && SUCCEEDED(wic->CreateFormatConverter(&cv)) &&
            SUCCEEDED(cv->Initialize(fr, GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, nullptr, 0,
                                     WICBitmapPaletteTypeCustom))) {
            e.px.resize((size_t)w * h);
            if (SUCCEEDED(cv->CopyPixels(nullptr, w * 4, (UINT)(e.px.size() * 4), (BYTE*)e.px.data()))) {
                if (e.mask)
                    for (auto& p : e.px) {
                        UINT32 g = p & 0xFF;
                        p = (g << 24) | (g << 16) | (g << 8) | g;
                    }
                e.w = (int)w;
                e.h = (int)h;
                ok = true;
            }
        }
    }
    if (cv) cv->Release();
    if (fr) fr->Release();
    if (dec) dec->Release();
    if (st) st->Release();
    return ok;
}

// Newest requests first, so the cards on screen load before ones scrolled past.
static void PreviewWorker() {
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    IWICImagingFactory* wic = nullptr;
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic));
    std::wstring dir = PreviewDir();
    for (;;) {
        std::wstring id;
        {
            std::unique_lock<std::mutex> lk(g_fontMx);
            g_previewCv.wait(lk, [] { return g_previewQuit || !g_previewQueue.empty(); });
            if (g_previewQuit) break;
            id = g_previewQueue.back();
            g_previewQueue.pop_back();
        }
        std::string png;
        std::wstring file = dir + L"\\" + id + L".png";
        PreviewEntry e;
        bool ok = wic && ReadFileBytes(file, png) && DecodePreview(wic, png, e);
        if (!ok && wic) {
            png.clear();
            ok = HttpGet(kPreviewBase + id + L".png", &png, L"", nullptr) && DecodePreview(wic, png, e);
            if (ok) {
                SHCreateDirectoryExW(nullptr, dir.c_str(), nullptr);
                WriteFileBytes(file, png);
            }
        }
        std::lock_guard<std::mutex> lk(g_fontMx);
        PreviewEntry& slot = g_previews[id];
        if (ok) {
            slot.px = std::move(e.px);
            slot.w = e.w;
            slot.h = e.h;
            slot.mask = e.mask;
            slot.ready = true;
        } else slot.failed = true;
    }
    if (wic) wic->Release();
    CoUninitialize();
}

static void StartPreviewWorkers() {
    for (int i = 0; i < 4; i++) g_previewThreads.emplace_back(PreviewWorker);
}
static void StopPreviewWorkers() {
    {
        std::lock_guard<std::mutex> lk(g_fontMx);
        g_previewQuit = true;
    }
    g_previewCv.notify_all();
    for (auto& t : g_previewThreads) t.join();
    g_previewThreads.clear();
}

// The card picture, once it's loaded (asks for it the first time).
static PreviewEntry* GetPreview(const std::wstring& id) {
    std::lock_guard<std::mutex> lk(g_fontMx);
    auto it = g_previews.find(id);
    if (it == g_previews.end()) {
        g_previews[id];
        g_previewQueue.push_back(id);
        g_previewCv.notify_one();
        return nullptr;
    }
    PreviewEntry& e = it->second;
    if (!e.ready) return nullptr;
    if (!e.bmp && g_rt) {
        D2D1_BITMAP_PROPERTIES bp =
            D2D1::BitmapProperties(D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
        g_rt->CreateBitmap(D2D1::SizeU(e.w, e.h), e.px.data(), e.w * 4, bp, &e.bmp);
        if (e.shownAt == 0) e.shownAt = Now();
    }
    return e.bmp ? &e : nullptr;
}

// Pictures that failed (offline) are tried again the next time the fonts are opened.
static void RetryFailedPreviews() {
    std::lock_guard<std::mutex> lk(g_fontMx);
    for (auto it = g_previews.begin(); it != g_previews.end();)
        it = it->second.failed ? g_previews.erase(it) : std::next(it);
}

static void ReleaseFontBitmaps() {
    std::lock_guard<std::mutex> lk(g_fontMx);
    for (auto& kv : g_previews) SafeRelease(kv.second.bmp);
}

// A font that's only on this PC is previewed with the font itself.
static IDWriteTextFormat* LocalFontFormat(const std::wstring& file) {
    auto it = g_localFontFmt.find(file);
    if (it != g_localFontFmt.end()) return it->second;
    IDWriteTextFormat* fmt = nullptr;
    IDWriteFactory5* f5 = nullptr;
    IDWriteFontSetBuilder1* b = nullptr;
    IDWriteFontFile* ff = nullptr;
    IDWriteFontSet* set = nullptr;
    IDWriteFontCollection1* coll = nullptr;
    std::wstring path = FontsDir() + L"\\" + file;
    if (SUCCEEDED(g_dw->QueryInterface(IID_PPV_ARGS(&f5))) && SUCCEEDED(f5->CreateFontSetBuilder(&b)) &&
        SUCCEEDED(f5->CreateFontFileReference(path.c_str(), nullptr, &ff)) && SUCCEEDED(b->AddFontFile(ff)) &&
        SUCCEEDED(b->CreateFontSet(&set)) && SUCCEEDED(f5->CreateFontCollectionFromFontSet(set, &coll)) &&
        coll->GetFontFamilyCount() > 0) {
        IDWriteFontFamily* fam = nullptr;
        IDWriteLocalizedStrings* names = nullptr;
        if (SUCCEEDED(coll->GetFontFamily(0, &fam)) && SUCCEEDED(fam->GetFamilyNames(&names))) {
            wchar_t n[256] = {};
            names->GetString(0, n, 256);
            if (SUCCEEDED(g_dw->CreateTextFormat(n, coll, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                                                 DWRITE_FONT_STRETCH_NORMAL, 17.f, L"en-us", &fmt))) {
                fmt->SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                fmt->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }
        }
        SafeRelease(names);
        SafeRelease(fam);
    }
    SafeRelease(coll);
    SafeRelease(set);
    SafeRelease(ff);
    SafeRelease(b);
    SafeRelease(f5);
    g_localFontFmt[file] = fmt;
    return fmt;
}

// ---- using a font ----------------------------------------------------------------
static void ReleaseTextResources() {
    SafeRelease(g_placeholderLayout);
    for (auto** f : {&g_fmtText, &g_fmtHint, &g_fmtRow, &g_fmtRowRight, &g_fmtGlyph, &g_fmtHeader, &g_fmtCard,
                     &g_fmtTile, &g_fmtMenu, &g_fmtGlyphSm})
        SafeRelease(*f);
    SafeRelease(g_fontColl);
    g_fontFamily.clear();
}

// Switches every text in the bar to `file` from the Fonts folder ("" = the Windows font).
static void ApplyFont(const std::wstring& file) {
    WriteString(L"Font", file);
    ReleaseTextResources();
    CreateTextResources();
}

static void FontActivate(int item) {
    if (item < 0 || item >= (int)g_fontCat.size()) return;
    const FontItem& it = g_fontCat[item];
    if (FontIsLocal(it)) {
        if (!FontInUse(it)) ApplyFont(it.file);
    } else StartFontDownload(item);
}

static void OpenFontFolder() {
    std::wstring dir = FontsDir();
    SHCreateDirectoryExW(nullptr, dir.c_str(), nullptr);
    AllowSetForegroundWindow(ASFW_ANY);
    g_prevFg = nullptr;
    Hide();
    std::thread([dir]() {
        CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        ShellExecuteW(nullptr, L"open", L"explorer.exe", (L"\"" + dir + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
        CoUninitialize();
    }).detach();
}

// ---- drawing -----------------------------------------------------------------------
static void DrawProgressRing(D2D1_POINT_2F c, float r, float p, D2D1_COLOR_F track, D2D1_COLOR_F fill) {
    g_solid->SetColor(track);
    g_rt->DrawEllipse(D2D1::Ellipse(c, r, r), g_solid, 2.f);
    p = std::clamp(p, 0.02f, 0.999f);
    ID2D1PathGeometry* geo = nullptr;
    ID2D1GeometrySink* sink = nullptr;
    if (SUCCEEDED(g_d2d->CreatePathGeometry(&geo)) && SUCCEEDED(geo->Open(&sink))) {
        float a = p * 6.2831853f;
        sink->BeginFigure({c.x, c.y - r}, D2D1_FIGURE_BEGIN_HOLLOW);
        sink->AddArc(D2D1::ArcSegment({c.x + r * std::sin(a), c.y - r * std::cos(a)}, D2D1::SizeF(r, r), 0,
                                      D2D1_SWEEP_DIRECTION_CLOCKWISE, p > 0.5f ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
        sink->EndFigure(D2D1_FIGURE_END_OPEN);
        sink->Close();
        g_solid->SetColor(fill);
        g_rt->DrawGeometry(geo, g_solid, 2.f);
    }
    SafeRelease(sink);
    SafeRelease(geo);
}

static void DrawFontGrid(float top, float dt, float alpha, double now) {
    using namespace Lay;
    CardGrid& G = g_fontGrid;
    const float CardW = G.CardW(), PrevH = G.prevH, NameH = G.nameH;
    const Theme& th = g_th;
    g_fontBtnHits.clear();
    g_fontDefaultHit = {};

    // header: title on the left; "Use default font" (or the keys) on the right
    std::wstring title = g_fontList.size() == g_fontCat.size() ? L"Fonts"
                         : std::to_wstring(g_fontList.size()) + (g_fontList.size() == 1 ? L" font" : L" fonts");
    DrawLayout(title, g_fmtHeader, Margin + GridPad + 2, top + GridPad - 4, 200, GridHeaderH, WithA(th.dim, alpha));
    if (!g_fontFile.empty()) {
        const wchar_t* t = L"Use default font";
        float w = 128, x = Margin + BarW - GridPad - w, y = top + GridPad - 1;
        D2D1_RECT_F r = D2D1::RectF(x, y, x + w, y + GridHeaderH - 6);
        bool hot = g_lastMouse.x >= 0 && g_fontDefaultHot;
        g_solid->SetColor(WithA(th.text, alpha * (hot ? 0.14f : 0.08f)));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, 11, 11), g_solid);
        DrawLayout(t, g_fmtCard, x, y, w, GridHeaderH - 6, WithA(th.text, alpha * 0.9f));
        g_fontDefaultHit = r;
    } else {
        DrawLayout(L"↵ download / use", g_fmtRowRight, Margin + BarW - GridPad - 260, top + GridPad - 4, 258,
                   GridHeaderH, WithA(th.dim, alpha * 0.85f));
    }

    float cTop = top + CardGrid::TopH, cBottom = top + g_panelH;
    G.Animate(dt);
    g_rt->PushAxisAlignedClip(D2D1::RectF(Margin, cTop, Margin + BarW, cBottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

    if (g_fontList.empty())
        DrawLayout(L"No fonts match", g_fmtRow, Margin + GridPad + 2, cTop, 300, 40, WithA(th.dim, alpha));

    for (auto& h : G.headers) {
        float y = cTop + h.second - G.off;
        if (y > cBottom || y + G.headH < cTop) continue;
        DrawLayout(h.first, g_fmtHeader, Margin + GridPad + 2, y, 400, G.headH - 2, WithA(th.dim, alpha * 0.9f));
    }

    std::unordered_map<std::wstring, float> progress;
    std::set<std::wstring> failed;
    {
        std::lock_guard<std::mutex> lk(g_fontMx);
        progress = g_fontProgress;
        failed = g_fontFailed;
    }
    for (int k = 0; k < (int)G.cells.size(); k++) {
        const GridCell& c = G.cells[k];
        int item = g_fontList[c.item];
        float x = c.x, y = cTop + c.y - G.off;
        G.cellSel[k] = Damp(G.cellSel[k], k == g_sel ? 1.f : 0.f, 16.f, dt);
        if (y > cBottom || y + G.RowH() < cTop) continue;
        const FontItem& it = g_fontCat[item];
        float sel = G.cellSel[k];
        float cardA = alpha * Clamp01(g_contentA * 1.3);
        D2D1_RECT_F card = D2D1::RectF(x, y, x + CardW, y + PrevH);
        D2D1_RECT_F hit = D2D1::RectF(x, std::max(y, cTop), x + CardW, std::min(y + PrevH + NameH, cBottom));
        if (hit.bottom > hit.top) g_hits.push_back({hit, k});

        g_solid->SetColor(WithA(th.text, cardA * (0.05f + 0.04f * sel)));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(card, 9, 9), g_solid);

        // the picture: the font's own letters
        bool local = FontIsLocal(it);
        if (item >= (int)g_fontCatN) {
            if (IDWriteTextFormat* f = LocalFontFormat(it.file)) {
                g_rt->PushAxisAlignedClip(card, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                DrawLayout(kFontSample, f, x + 10, y + 2, CardW - 20, PrevH - 4, WithA(th.text, cardA));
                g_rt->PopAxisAlignedClip();
            }
        } else if (PreviewEntry* e = GetPreview(it.id)) {
            float fade = Clamp01((now - e->shownAt) / 0.25);
            D2D1_RECT_F src = D2D1::RectF(0, 0, (float)e->w, (float)e->h);
            if (e->mask) {
                g_solid->SetColor(WithA(th.text, cardA * fade));
                g_rt->SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
                g_rt->FillOpacityMask(e->bmp, g_solid, D2D1_OPACITY_MASK_CONTENT_GRAPHICS, &card, &src);
                g_rt->SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            } else {
                g_rt->DrawBitmap(e->bmp, card, cardA * fade, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, src);
            }
        } else {  // loading: two soft lines where the text will be
            float pulse = 0.05f + 0.03f * (float)std::sin(now * 4 + k * 0.6);
            g_solid->SetColor(WithA(th.text, cardA * pulse));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 10, y + 18, x + CardW - 24, y + 28), 5, 5), g_solid);
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + 10, y + 36, x + CardW * 0.6f, y + 46), 5, 5), g_solid);
        }

        if (sel > 0.01f) {
            g_solid->SetColor(WithA(th.accent, cardA * sel));
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x - 2, y - 2, x + CardW + 2, y + PrevH + 2), 11, 11),
                                       g_solid, 2.f);
        }

        // top-right corner: in use, downloading, or the Download button
        D2D1_POINT_2F bc = {x + CardW - 15, y + 15};
        auto pg = progress.find(it.id);
        if (FontInUse(it)) {
            g_solid->SetColor(WithA(th.accent, cardA));
            g_rt->FillEllipse(D2D1::Ellipse(bc, 9, 9), g_solid);
            DrawLayout(L"\xE73E", g_fmtGlyphSm, bc.x - 8, bc.y - 8, 16, 16, WithA(C(0xFFFFFF), cardA));
        } else if (pg != progress.end()) {
            DrawProgressRing(bc, 8, pg->second, WithA(th.text, cardA * 0.15f), WithA(th.accent, cardA));
        } else if (!local) {
            bool hot = g_fontBtnHot == k;
            g_solid->SetColor(hot ? WithA(th.accent, cardA) : WithA(th.text, cardA * (0.10f + 0.06f * sel)));
            g_rt->FillEllipse(D2D1::Ellipse(bc, 11, 11), g_solid);
            bool fail = failed.count(it.id) > 0;
            DrawLayout(fail ? L"\xE72C" : L"\xE896", g_fmtGlyphSm, bc.x - 8, bc.y - 8, 16, 16,
                       hot ? WithA(C(0xFFFFFF), cardA) : WithA(fail ? C(0xE0605A) : th.text, cardA * 0.9f));
            D2D1_RECT_F br = D2D1::RectF(bc.x - 13, bc.y - 13, bc.x + 13, bc.y + 13);
            if (br.top >= cTop && br.bottom <= cBottom) g_fontBtnHits.push_back({br, k});
        }

        D2D1_COLOR_F nc = LerpC(th.dim, th.text, std::max(sel, FontInUse(it) ? 1.f : 0.f));
        DrawLayout(it.name, g_fmtCard, x + 4, y + PrevH + 2, CardW - 8, NameH - 4, WithA(nc, cardA));
    }
    g_rt->PopAxisAlignedClip();

    // scroll bar
    float view = cBottom - cTop;
    if (G.MaxOff() > 0.5f)
        DrawScrollBar(cTop + 4, view - 8, G.off / G.MaxOff(), view / (G.contentH + 6), alpha, dt);
}

static int FontButtonAt(float x, float y) {
    for (const HitItem& h : g_fontBtnHits)
        if (x >= h.r.left && x <= h.r.right && y >= h.r.top && y <= h.r.bottom) return h.index;
    return -1;
}
static bool FontDefaultAt(float x, float y) {
    const D2D1_RECT_F& r = g_fontDefaultHit;
    return r.right > r.left && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
}

// ---- glue for main.cpp ----------------------------------------------------------------
static int FontCount() { return (int)g_fontList.size(); }

// Opening the fonts: the one in use is selected and scrolled to.
static void FontEnter() {
    RetryFailedPreviews();
    g_sel = 0;
    for (int k = 0; k < (int)g_fontList.size(); k++)
        if (FontInUse(g_fontCat[g_fontList[k]])) g_sel = k;
    g_fontGrid.ScrollTo(0, true);
    g_fontGrid.EnsureVisible(g_sel, true);
}

static void FontActivateSel() {
    if (g_sel >= 0 && g_sel < (int)g_fontList.size()) FontActivate(g_fontList[g_sel]);
}

static bool FontKey(WPARAM vk, bool shift) {
    int k = g_fontGrid.Key(vk, shift, g_sel);
    if (k < 0) return false;
    Select(k);
    g_fontGrid.EnsureVisible(g_sel, false);
    return true;
}

// A click in the panel: "Use default font", a Download button, or a card (a font that's
// already on this PC is used right away; others wait for their Download button).
static void FontClick(float x, float y) {
    if (FontDefaultAt(x, y)) { ApplyFont(L""); return; }
    int b = FontButtonAt(x, y);
    if (b >= 0 && b < (int)g_fontList.size()) {
        g_sel = b;
        StartFontDownload(g_fontList[b]);
        return;
    }
    int k = HitItemAt(x, y);
    if (k >= 0 && k < (int)g_fontList.size()) {
        g_sel = k;
        if (FontIsLocal(g_fontCat[g_fontList[k]])) FontActivate(g_fontList[k]);
    }
}

static void FontWheel(int steps) { g_fontGrid.ScrollTo(g_fontGrid.offTarget + steps * 64.f, false); }

static void FontHover(float x, float y) {
    g_fontBtnHot = g_mode == Mode::Fonts ? FontButtonAt(x, y) : -1;
    g_fontDefaultHot = g_mode == Mode::Fonts && FontDefaultAt(x, y);
}
static bool FontHandAt(float x, float y) {
    return g_mode == Mode::Fonts && (FontButtonAt(x, y) >= 0 || FontDefaultAt(x, y));
}

// A download finished: use the font, and refresh the cards (keeping the scroll position).
static void FontDownloaded(const std::wstring& id, bool ok) {
    {
        std::lock_guard<std::mutex> lk(g_fontMx);
        g_fontProgress.erase(id);
        if (!ok) g_fontFailed.insert(id);
    }
    std::wstring file;
    for (size_t i = 0; i < g_fontCatN; i++)
        if (g_fontCat[i].id == id) file = g_fontCat[i].file;
    if (ok && !file.empty()) ApplyFont(file);
    // The new font moves up to "Downloaded"; the selection follows it there when it was on it.
    if (g_mode == Mode::Fonts) {
        int selItem = g_sel >= 0 && g_sel < (int)g_fontList.size() ? g_fontList[g_sel] : -1;
        float off = g_fontGrid.off, target = g_fontGrid.offTarget;
        BuildFontList(g_fontFilter);
        g_sel = 0;
        for (int k = 0; k < (int)g_fontList.size(); k++)
            if (g_fontList[k] == selItem) g_sel = k;
        g_fontGrid.off = std::min(off, g_fontGrid.MaxOff());
        g_fontGrid.ScrollTo(target, false);
        g_fontGrid.EnsureVisible(g_sel, false);
    }
}
