// fontpreview: draws the card pictures for "> font" with DirectWrite, the same text engine
// the app uses, so every script is shaped the way Windows shows it.
//
//   fontpreview.exe list.tsv
//
// Each line of list.tsv (UTF-8) is: font file \t output .png \t sample script \t flags
// Monochrome fonts are saved as 8-bit grayscale (the app uses the gray level as coverage
// and colors it with the theme); color fonts (flag "c") are saved in full color.
#define NOMINMAX
#include <windows.h>
#include <d2d1_1.h>
#include <dwrite_3.h>
#include <wincodec.h>

#include <algorithm>
#include <cstdio>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#pragma comment(lib, "d2d1.lib")
#pragma comment(lib, "dwrite.lib")
#pragma comment(lib, "windowscodecs.lib")
#pragma comment(lib, "ole32.lib")

constexpr UINT W = 384, H = 128;  // 2x the card picture (192 x 64)
constexpr float Pad = 18.f, Size = 34.f;

static const wchar_t* SampleFor(const std::wstring& s) {
    if (s == L"arab") return L"أبجد هوز حطي كلمن سعفص قرشت";
    if (s == L"urdu") return L"اردو ایک خوبصورت زبان ہے";
    if (s == L"hebr") return L"שלום עולם, אבגדהוזחטי";
    if (s == L"beng") return L"আমার সোনার বাংলা";
    if (s == L"deva") return L"नमस्ते दुनिया, कखगघ";
    if (s == L"taml") return L"வணக்கம் உலகம்";
    if (s == L"thai") return L"สวัสดีชาวโลก กขฃค";
    if (s == L"armn") return L"Բարեւ աշխարհ, ԱԲԳԴ";
    if (s == L"geor") return L"გამარჯობა მსოფლიო";
    if (s == L"cjk") return L"你好世界 こんにちは 안녕하세요";
    if (s == L"emoji") return L"😀 🎉 🚀 🍕 🐱 🌈 ⚽ 🎧";
    if (s == L"symb") return L"☀ ☂ ☃ ♞ ✈ ⚓ ♫ ✿ ☯ ⌘";
    if (s == L"math") return L"x² + y² = r²  ∑ ∫ √π ∞ ≠ ∂";
    return L"The quick brown fox jumps over the lazy dog";
}

static std::wstring Widen(const std::string& s) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s.c_str(), (int)s.size(), nullptr, 0);
    std::wstring w(n, 0);
    MultiByteToWideChar(CP_UTF8, 0, s.c_str(), (int)s.size(), w.data(), n);
    return w;
}

static IDWriteFactory5* dw;
static ID2D1Factory1* d2d;
static IWICImagingFactory* wic;

static bool Save(IWICBitmapSource* src, const std::wstring& path) {
    IWICStream* st = nullptr;
    IWICBitmapEncoder* enc = nullptr;
    IWICBitmapFrameEncode* fr = nullptr;
    bool ok = SUCCEEDED(wic->CreateStream(&st)) && SUCCEEDED(st->InitializeFromFilename(path.c_str(), GENERIC_WRITE)) &&
              SUCCEEDED(wic->CreateEncoder(GUID_ContainerFormatPng, nullptr, &enc)) &&
              SUCCEEDED(enc->Initialize(st, WICBitmapEncoderNoCache)) && SUCCEEDED(enc->CreateNewFrame(&fr, nullptr)) &&
              SUCCEEDED(fr->Initialize(nullptr)) && SUCCEEDED(fr->WriteSource(src, nullptr)) &&
              SUCCEEDED(fr->Commit()) && SUCCEEDED(enc->Commit());
    if (fr) fr->Release();
    if (enc) enc->Release();
    if (st) st->Release();
    return ok;
}

static bool Render(const std::wstring& font, const std::wstring& out, const std::wstring& script, bool color) {
    IDWriteFontSetBuilder1* b = nullptr;
    IDWriteFontFile* file = nullptr;
    IDWriteFontSet* set = nullptr;
    IDWriteFontCollection1* coll = nullptr;
    if (FAILED(dw->CreateFontSetBuilder(&b)) || FAILED(dw->CreateFontFileReference(font.c_str(), nullptr, &file)) ||
        FAILED(b->AddFontFile(file)) || FAILED(b->CreateFontSet(&set)) ||
        FAILED(dw->CreateFontCollectionFromFontSet(set, &coll)) || coll->GetFontFamilyCount() == 0) {
        fwprintf(stderr, L"cannot load %s\n", font.c_str());
        return false;
    }
    // the face closest to Regular (variable fonts list every named instance)
    IDWriteFontFamily* fam = nullptr;
    coll->GetFontFamily(0, &fam);
    IDWriteFont* best = nullptr;
    int bestScore = 1 << 30;
    for (UINT i = 0; i < fam->GetFontCount(); i++) {
        IDWriteFont* f = nullptr;
        if (FAILED(fam->GetFont(i, &f))) continue;
        int score = abs((int)f->GetWeight() - 400) + (f->GetStyle() != DWRITE_FONT_STYLE_NORMAL ? 50 : 0) +
                    abs((int)f->GetStretch() - 5) * 20;
        if (!best || score < bestScore) { if (best) best->Release(); best = f; bestScore = score; }
        else f->Release();
    }
    if (fam->GetFontCount() == 1 || !best) {  // one face: draw it as it is
        if (best) best->Release();
        fam->GetFont(0, &best);
    }
    IDWriteLocalizedStrings* names = nullptr;
    fam->GetFamilyNames(&names);
    UINT32 idx = 0;
    BOOL ex = FALSE;
    names->FindLocaleName(L"en-us", &idx, &ex);
    if (!ex) idx = 0;
    wchar_t family[256] = {};
    names->GetString(idx, family, 256);

    IDWriteTextFormat* fmt = nullptr;
    dw->CreateTextFormat(family, coll, best->GetWeight(), best->GetStyle(), best->GetStretch(), Size, L"en-us", &fmt);
    fmt->SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
    bool rtl = script == L"arab" || script == L"urdu" || script == L"hebr";
    if (rtl) fmt->SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
    const wchar_t* text = SampleFor(script);
    IDWriteTextLayout* lay = nullptr;
    dw->CreateTextLayout(text, (UINT32)wcslen(text), fmt, W - 2 * Pad, H, &lay);

    IWICBitmap* bmp = nullptr;
    wic->CreateBitmap(W, H, GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad, &bmp);
    ID2D1RenderTarget* rt = nullptr;
    d2d->CreateWicBitmapRenderTarget(bmp, D2D1::RenderTargetProperties(D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED), 96, 96), &rt);
    ID2D1SolidColorBrush* brush = nullptr;
    rt->CreateSolidColorBrush(D2D1::ColorF(1, 1, 1, 1), &brush);
    rt->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    rt->BeginDraw();
    rt->Clear(D2D1::ColorF(0, 0, 0, 0));
    // only whole lines: as many as fit, centered vertically
    UINT32 nLines = 0;
    lay->GetLineMetrics(nullptr, 0, &nLines);
    std::vector<DWRITE_LINE_METRICS> lm(nLines);
    lay->GetLineMetrics(lm.data(), nLines, &nLines);
    float fitH = 0;
    for (UINT32 i = 0; i < nLines; i++) {
        if (i > 0 && fitH + lm[i].height > H - 2 * Pad + 10) break;
        fitH += lm[i].height;
    }
    float y = std::max(2.f, (H - fitH) / 2);
    rt->PushAxisAlignedClip(D2D1::RectF(0, y, (float)W, y + fitH), D2D1_ANTIALIAS_MODE_ALIASED);
    rt->DrawTextLayout(D2D1::Point2F(Pad, y), lay, brush,
                       color ? D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT : D2D1_DRAW_TEXT_OPTIONS_NONE);
    rt->PopAxisAlignedClip();
    HRESULT hr = rt->EndDraw();

    bool ok = SUCCEEDED(hr);
    if (ok && color) {
        IWICFormatConverter* cv = nullptr;
        ok = SUCCEEDED(wic->CreateFormatConverter(&cv)) &&
             SUCCEEDED(cv->Initialize(bmp, GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, nullptr, 0, WICBitmapPaletteTypeCustom)) &&
             Save(cv, out);
        if (cv) cv->Release();
    } else if (ok) {
        std::vector<BYTE> px(W * H * 4), gray(W * H);
        bmp->CopyPixels(nullptr, W * 4, (UINT)px.size(), px.data());
        for (UINT i = 0; i < W * H; i++) gray[i] = px[i * 4 + 3];
        IWICBitmap* g = nullptr;
        ok = SUCCEEDED(wic->CreateBitmapFromMemory(W, H, GUID_WICPixelFormat8bppGray, W, (UINT)gray.size(), gray.data(), &g)) &&
             Save(g, out);
        if (g) g->Release();
    }
    brush->Release(); rt->Release(); bmp->Release(); lay->Release(); fmt->Release(); names->Release();
    best->Release(); fam->Release(); coll->Release(); set->Release(); file->Release(); b->Release();
    if (!ok) fwprintf(stderr, L"failed %s\n", out.c_str());
    return ok;
}

int wmain(int argc, wchar_t** argv) {
    if (argc < 2) { fwprintf(stderr, L"usage: fontpreview list.tsv\n"); return 2; }
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory5), (IUnknown**)&dw);
    D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, &d2d);
    CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&wic));
    std::ifstream in(argv[1], std::ios::binary);
    std::string line;
    int done = 0, failed = 0;
    while (std::getline(in, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) continue;
        std::vector<std::wstring> f;
        std::stringstream ss(line);
        std::string part;
        while (std::getline(ss, part, '\t')) f.push_back(Widen(part));
        while (f.size() < 4) f.push_back(L"");
        if (Render(f[0], f[1], f[2], f[3].find(L'c') != std::wstring::npos)) done++;
        else failed++;
    }
    wprintf(L"%d rendered, %d failed\n", done, failed);
    return failed ? 1 : 0;
}
