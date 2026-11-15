// ---------------------------------------------------------------------------
// Caret ("> caret"): its color, a glow, and little effects when typing, deleting and moving.
// Cards like the theme picker, one section per setting; every card plays its effect.
// The selected card is also tried out on the real caret. Included by main.cpp after fonts.inl.
// ---------------------------------------------------------------------------
enum { CS_COLOR, CS_GLOW, CS_TYPING, CS_DELETING, CS_MOVING, CS_COUNT };
static const wchar_t* kCaretSecTitle[CS_COUNT] = {L"Color", L"Glow", L"When typing", L"When deleting", L"When moving"};
static const wchar_t* kCaretReg[CS_COUNT] = {L"CaretColor", L"CaretGlow", L"CaretTyping", L"CaretDeleting", L"CaretMoving"};
struct CaretColorDef {
    const wchar_t* name;
    int rgb;  // -1 the theme's accent, -2 the text color, -3 rainbow
};
static const CaretColorDef kCaretColors[] = {
    {L"Theme", -1},       {L"Text", -2},       {L"Ice", 0x9AD8FF},  {L"Blue", 0x3B82F6},
    {L"Violet", 0x8B5CF6}, {L"Pink", 0xEC4899}, {L"Red", 0xEF4444},  {L"Orange", 0xF97316},
    {L"Gold", 0xFACC15},  {L"Lime", 0x84CC16}, {L"Mint", 0x2DD4BF}, {L"Rainbow", -3},
};
static const wchar_t* kCaretGlows[] = {L"Off", L"Soft", L"Strong", L"Pulse"};
static const wchar_t* kCaretTypings[] = {L"None", L"Sparkle", L"Confetti", L"Bubbles"};
static const wchar_t* kCaretDeletings[] = {L"None", L"Explosion", L"Shatter", L"Dust"};
static const wchar_t* kCaretMovings[] = {L"Glide", L"Trail", L"Comet", L"Plain"};
static const int kCaretOptN[CS_COUNT] = {(int)(sizeof(kCaretColors) / sizeof(kCaretColors[0])), 4, 4, 4, 4};

static const wchar_t* CaretOptName(int sec, int v) {
    switch (sec) {
    case CS_COLOR: return kCaretColors[v].name;
    case CS_GLOW: return kCaretGlows[v];
    case CS_TYPING: return kCaretTypings[v];
    case CS_DELETING: return kCaretDeletings[v];
    default: return kCaretMovings[v];
    }
}

static int g_caretSet[CS_COUNT];  // what's chosen (registry DWORDs)
struct CaretOpt {
    int sec, val;
};
static std::vector<CaretOpt> g_caretItems;  // the cards shown
static CardGrid g_caretGrid{4, 56.f, 24.f};

static void LoadCaretSettings() {
    for (int s = 0; s < CS_COUNT; s++) g_caretSet[s] = std::clamp((int)ReadDword(kCaretReg[s], 0), 0, kCaretOptN[s] - 1);
}

// The setting in effect: the selected card is tried out on the real caret while "> caret" is open.
static int CaretOptEff(int sec) {
    if (g_mode == Mode::Caret && g_sel >= 0 && g_sel < (int)g_caretItems.size() && g_caretItems[g_sel].sec == sec)
        return g_caretItems[g_sel].val;
    return g_caretSet[sec];
}

static D2D1_COLOR_F Hue(double h) {
    h = h - std::floor(h);
    float r = Clamp01(std::fabs(h * 6 - 3) - 1), g = Clamp01(2 - std::fabs(h * 6 - 2)), b = Clamp01(2 - std::fabs(h * 6 - 4));
    return D2D1::ColorF(0.25f + 0.75f * r, 0.25f + 0.75f * g, 0.25f + 0.75f * b, 1.f);
}
static D2D1_COLOR_F CaretColorOf(int v, double now) {
    int rgb = kCaretColors[std::clamp(v, 0, kCaretOptN[CS_COLOR] - 1)].rgb;
    if (rgb == -1) return g_th.accent;
    if (rgb == -2) return g_th.text;
    if (rgb == -3) return Hue(now * 0.22);
    return C((UINT32)rgb);
}
static D2D1_COLOR_F CaretColorNow(double now) { return CaretColorOf(CaretOptEff(CS_COLOR), now); }
static D2D1_COLOR_F LerpWhite(D2D1_COLOR_F c, float t) { return LerpC(c, C(0xFFFFFF), t); }

// ---- the list ---------------------------------------------------------------------
static void BuildCaretList(const std::wstring& filter) {
    std::vector<std::wstring> words;
    for (size_t a = 0; a < filter.size();) {
        size_t b = filter.find(L' ', a);
        if (b == std::wstring::npos) b = filter.size();
        if (b > a) words.push_back(filter.substr(a, b - a));
        a = b + 1;
    }
    g_caretItems.clear();
    std::vector<std::wstring> cats;
    for (int s = 0; s < CS_COUNT; s++)
        for (int v = 0; v < kCaretOptN[s]; v++) {
            std::wstring hay = Lower(std::wstring(CaretOptName(s, v)) + L" " + kCaretSecTitle[s]);
            bool ok = true;
            for (auto& w : words)
                if (hay.find(w) == std::wstring::npos) { ok = false; break; }
            if (!ok) continue;
            g_caretItems.push_back({s, v});
            cats.push_back(kCaretSecTitle[s]);
        }
    g_caretGrid.Layout(cats);
}

static int CaretCount() { return (int)g_caretItems.size(); }
static float CaretPanelH() { return g_caretGrid.PanelH(); }

static void CaretEnter() {
    g_sel = 0;
    for (int k = 0; k < (int)g_caretItems.size(); k++)
        if (g_caretItems[k].sec == CS_COLOR && g_caretItems[k].val == g_caretSet[CS_COLOR]) g_sel = k;
    g_caretGrid.ScrollTo(0, true);
    g_caretGrid.EnsureVisible(g_sel, true);
}

static void CaretApply(int k) {
    if (k < 0 || k >= (int)g_caretItems.size()) return;
    const CaretOpt& o = g_caretItems[k];
    g_caretSet[o.sec] = o.val;
    WriteDword(kCaretReg[o.sec], (DWORD)o.val);
}
static void CaretApplySel() { CaretApply(g_sel); }

// "Pink · Sparkle" (or "Pink · 3 effects") for the commands list.
static std::wstring CaretSummary() {
    std::vector<std::wstring> extra;
    if (g_caretSet[CS_GLOW]) extra.push_back(std::wstring(kCaretGlows[g_caretSet[CS_GLOW]]) + L" glow");
    for (int sec : {CS_TYPING, CS_DELETING, CS_MOVING})
        if (g_caretSet[sec]) extra.push_back(CaretOptName(sec, g_caretSet[sec]));
    std::wstring s = CaretOptName(CS_COLOR, g_caretSet[CS_COLOR]);
    if (extra.size() == 1) s += L" · " + extra[0];
    else if (extra.size() > 1) s += L" · " + std::to_wstring(extra.size()) + L" effects";
    return s;
}

// ---- effects ----------------------------------------------------------------------------
// Every burst is a pure function of its age, so the cards can play them on a loop too.
enum { BK_NONE, BK_SPARKLE, BK_CONFETTI, BK_BUBBLES, BK_EXPLOSION, BK_SHATTER, BK_DUST, BK_COMET, BK_GHOST };
struct Burst {
    int kind;
    float x, y;
    double t;      // when it started
    UINT32 seed;
    float w;       // width it spreads over (deleted text)
    D2D1_COLOR_F col;
    float dir;     // comet: which way the tail goes
};
static std::vector<Burst> g_bursts;
struct CaretEvent {
    bool typed;
    int index, count;
};
static std::vector<CaretEvent> g_caretEvents;  // from editing, turned into bursts when the text is drawn
static UINT32 g_burstSeed = 1;
static float g_fxLastX = -1e9f;  // where the last trail / comet bit was dropped
static ID2D1PathGeometry *g_starGeo, *g_shardGeo;

static void CaretTyped(int index, int count) { g_caretEvents.push_back({true, index, count}); }
static void CaretDeleted(int index, int count) { g_caretEvents.push_back({false, index, count}); }
static void ClearCaretFx() {
    g_bursts.clear();
    g_caretEvents.clear();
    g_fxLastX = -1e9f;
}

static float BurstLife(int kind) {
    switch (kind) {
    case BK_SPARKLE: return 0.65f;
    case BK_CONFETTI: return 0.95f;
    case BK_BUBBLES: return 1.0f;
    case BK_EXPLOSION: return 0.6f;
    case BK_SHATTER: return 0.85f;
    case BK_DUST: return 0.75f;
    case BK_COMET: return 0.4f;
    case BK_GHOST: return 0.22f;
    default: return 0.f;
    }
}

static float Rnd(UINT32 seed, int i, int k) {
    UINT32 h = seed * 0x9E3779B1u ^ ((UINT32)i * 0x85EBCA77u) ^ ((UINT32)k * 0xC2B2AE3Du);
    h ^= h >> 15;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12;
    h *= 0x297A2D39u;
    h ^= h >> 15;
    return (h & 0xFFFFFF) / 16777216.f;
}

static ID2D1PathGeometry* MakePoly(const D2D1_POINT_2F* p, int n) {
    ID2D1PathGeometry* geo = nullptr;
    ID2D1GeometrySink* sink = nullptr;
    if (SUCCEEDED(g_d2d->CreatePathGeometry(&geo)) && SUCCEEDED(geo->Open(&sink))) {
        sink->BeginFigure(p[0], D2D1_FIGURE_BEGIN_FILLED);
        sink->AddLines(p + 1, n - 1);
        sink->EndFigure(D2D1_FIGURE_END_CLOSED);
        sink->Close();
    }
    SafeRelease(sink);
    return geo;
}
static void EnsureFxGeometry() {
    if (!g_starGeo) {  // a four-pointed twinkle
        D2D1_POINT_2F p[8];
        for (int j = 0; j < 8; j++) {
            float a = j * 0.785398f - 1.570796f, r = (j % 2) ? 0.3f : 1.f;
            p[j] = {r * std::cos(a), r * std::sin(a)};
        }
        g_starGeo = MakePoly(p, 8);
    }
    if (!g_shardGeo) {
        D2D1_POINT_2F p[3] = {{0, -1}, {0.9f, 0.7f}, {-0.8f, 0.55f}};
        g_shardGeo = MakePoly(p, 3);
    }
}
static void ReleaseFxGeometry() {
    SafeRelease(g_starGeo);
    SafeRelease(g_shardGeo);
}

static void FillShape(ID2D1Geometry* g, float x, float y, float size, float deg, D2D1_COLOR_F c, const Matrix3x2F& base) {
    if (!g || size <= 0.05f) return;
    g_rt->SetTransform(Matrix3x2F::Scale(size, size) * Matrix3x2F::Rotation(deg) * Matrix3x2F::Translation(x, y) * base);
    g_solid->SetColor(c);
    g_rt->FillGeometry(g, g_solid);
    g_rt->SetTransform(base);
}
static void Dot(float x, float y, float r, D2D1_COLOR_F c) {
    if (r <= 0.05f || c.a <= 0.003f) return;
    g_solid->SetColor(c);
    g_rt->FillEllipse(D2D1::Ellipse({x, y}, r, r), g_solid);
}

static const UINT32 kConfetti[] = {0xFF5E7E, 0xFFD166, 0x4ADE80, 0x60A5FA, 0xC084FC};
static const UINT32 kFire[] = {0xFFF1A8, 0xFFD166, 0xFF8C42, 0xFF4D4D};

// k: size (1 on the bar, a little less on the cards); alpha: fades the whole burst.
static void DrawBurst(const Burst& b, double now, float k, float alpha) {
    float life = BurstLife(b.kind);
    float age = (float)(now - b.t);
    if (age < 0 || age >= life) return;
    float u = age / life, e = 1 - (1 - u) * (1 - u) * (1 - u);  // eased progress
    D2D1_MATRIX_3X2_F raw;
    g_rt->GetTransform(&raw);
    Matrix3x2F base = Matrix3x2F::ReinterpretBaseType(&raw)[0];
    EnsureFxGeometry();
    switch (b.kind) {
    case BK_SPARKLE: {
        if (u < 0.3f) Dot(b.x, b.y, (3 + u * 12) * k, WithA(LerpWhite(b.col, 0.6f), alpha * (1 - u / 0.3f) * 0.45f));
        for (int i = 0; i < 7; i++) {
            float ang = Rnd(b.seed, i, 0) * 6.2831853f, d = (10 + Rnd(b.seed, i, 1) * 24) * e * k;
            float tw = 0.75f + 0.25f * std::sin(age * 30 + i * 1.7f);
            float sz = (2.4f + Rnd(b.seed, i, 2) * 2.8f) * k * (1 - u) * tw;
            FillShape(g_starGeo, b.x + std::cos(ang) * d, b.y + std::sin(ang) * d * 0.85f, sz,
                      Rnd(b.seed, i, 3) * 90 + age * 160, WithA(LerpWhite(b.col, 0.45f), alpha * (1 - u * u)), base);
        }
        break;
    }
    case BK_CONFETTI:
        for (int i = 0; i < 10; i++) {
            float vx = (Rnd(b.seed, i, 0) - 0.5f) * 130, vy = -(60 + Rnd(b.seed, i, 1) * 90);
            float px = b.x + vx * age * k, py = b.y + (vy * age + 0.5f * 330 * age * age) * k;
            float rot = Rnd(b.seed, i, 2) * 360 + age * (Rnd(b.seed, i, 3) - 0.5f) * 900;
            float flip = 0.3f + 0.7f * std::fabs(std::cos(age * 9 + i));
            float a = alpha * (u < 0.7f ? 1.f : (1 - u) / 0.3f);
            g_rt->SetTransform(Matrix3x2F::Scale(k, k * flip) * Matrix3x2F::Rotation(rot) * Matrix3x2F::Translation(px, py) * base);
            g_solid->SetColor(C(kConfetti[(i + b.seed) % 5], a));
            g_rt->FillRectangle(D2D1::RectF(-1.3f, -2.4f, 1.3f, 2.4f), g_solid);
            g_rt->SetTransform(base);
        }
        break;
    case BK_BUBBLES:
        for (int i = 0; i < 5; i++) {
            float uu = Clamp01((age - Rnd(b.seed, i, 0) * 0.25f) / 0.75f);
            if (uu <= 0 || uu >= 1) continue;
            float eu = 1 - (1 - uu) * (1 - uu);
            float px = b.x + ((Rnd(b.seed, i, 2) - 0.5f) * 14 + std::sin(uu * 6 + i) * 3) * k;
            float py = b.y - (6 + Rnd(b.seed, i, 1) * 30) * eu * k;
            float r = (1.6f + Rnd(b.seed, i, 3) * 3.4f) * k * (0.6f + 0.4f * uu);
            g_solid->SetColor(WithA(LerpWhite(b.col, 0.3f), alpha * (1 - uu) * 0.9f));
            g_rt->DrawEllipse(D2D1::Ellipse({px, py}, r, r), g_solid, 1.f);
            Dot(px - r * 0.35f, py - r * 0.35f, std::max(0.5f, r * 0.22f), WithA(C(0xFFFFFF), alpha * (1 - uu) * 0.8f));
        }
        break;
    case BK_EXPLOSION: {
        float sc = 1 + 0.08f * std::min(b.w / 7.f, 8.f);
        if (u < 0.2f) Dot(b.x, b.y, (5 + 22 * u) * k * sc, C(0xFFF1A8, alpha * (1 - u / 0.2f) * 0.85f));
        g_solid->SetColor(C(0xFF8C42, alpha * (1 - u) * 0.9f));
        float rr = (4 + 24 * e) * k * sc;
        g_rt->DrawEllipse(D2D1::Ellipse({b.x, b.y}, rr, rr * 0.8f), g_solid, (3 * (1 - u) + 0.4f) * k);
        for (int i = 0; i < 14; i++) {
            float ang = Rnd(b.seed, i, 0) * 6.2831853f, d = (8 + Rnd(b.seed, i, 1) * 34) * e * k * sc;
            float ox = (Rnd(b.seed, i, 5) - 0.5f) * b.w;
            float r = (1.4f + Rnd(b.seed, i, 2) * 2.4f) * (1 - u) * k;
            Dot(b.x + ox + std::cos(ang) * d, b.y + std::sin(ang) * d * 0.8f, r, C(kFire[i % 4], alpha * (1 - u)));
        }
        break;
    }
    case BK_SHATTER:
        for (int i = 0; i < 9; i++) {
            float vx = (Rnd(b.seed, i, 0) - 0.5f) * 140, vy = -(30 + Rnd(b.seed, i, 1) * 80);
            float px = b.x + (Rnd(b.seed, i, 5) - 0.5f) * b.w + vx * age * k;
            float py = b.y + (Rnd(b.seed, i, 6) - 0.5f) * 10 * k + (vy * age + 0.5f * 380 * age * age) * k;
            FillShape(g_shardGeo, px, py, (2.4f + Rnd(b.seed, i, 4) * 3.4f) * k,
                      Rnd(b.seed, i, 2) * 360 + age * (Rnd(b.seed, i, 3) - 0.5f) * 720, WithA(b.col, alpha * (1 - u * u)), base);
        }
        break;
    case BK_DUST:
        for (int i = 0; i < 16; i++) {
            float px = b.x + ((Rnd(b.seed, i, 0) - 0.5f) * (b.w + 6) + (Rnd(b.seed, i, 2) - 0.5f) * 30 * age) * k;
            float py = b.y + ((Rnd(b.seed, i, 1) - 0.5f) * 14 - (6 + Rnd(b.seed, i, 3) * 22) * age) * k;
            Dot(px, py, (0.8f + Rnd(b.seed, i, 4) * 1.6f) * k * (1 - u * 0.5f), WithA(b.col, alpha * (1 - u) * 0.85f));
        }
        break;
    case BK_COMET: {
        float px = b.x + b.dir * (4 + Rnd(b.seed, 0, 0) * 14) * e * k, py = b.y + (Rnd(b.seed, 0, 1) - 0.5f) * 12 * e * k;
        Dot(px, py, (1 + Rnd(b.seed, 0, 2) * 1.8f) * (1 - u) * k, WithA(LerpWhite(b.col, 0.3f), alpha * (1 - u)));
        break;
    }
    case BK_GHOST: {
        float h = Lay::CaretH * k;
        g_solid->SetColor(WithA(b.col, alpha * 0.45f * (1 - u)));
        g_rt->FillRoundedRectangle(
            D2D1::RoundedRect(D2D1::RectF(b.x - Lay::CaretW / 2, b.y - h / 2, b.x + Lay::CaretW / 2, b.y + h / 2), 1, 1), g_solid);
        break;
    }
    }
}

// A soft light around the caret.
static void DrawCaretGlow(float x, float cy, float h, D2D1_COLOR_F col, float op, int glow, double now, float k) {
    if (!glow || op <= 0.01f) return;
    float a = glow == 1 ? 0.32f : glow == 2 ? 0.6f : 0.2f + 0.4f * (0.5f + 0.5f * (float)std::sin(now * 6.2831853 / 1.4));
    float rx = (glow == 2 ? 16.f : 11.f) * k, ry = h * (glow == 2 ? 0.95f : 0.8f);
    D2D1_GRADIENT_STOP stops[3] = {{0.f, WithA(col, a * op)}, {0.45f, WithA(col, a * op * 0.35f)}, {1.f, WithA(col, 0)}};
    ID2D1GradientStopCollection* coll = nullptr;
    ID2D1RadialGradientBrush* br = nullptr;
    if (SUCCEEDED(g_rt->CreateGradientStopCollection(stops, 3, &coll)) &&
        SUCCEEDED(g_rt->CreateRadialGradientBrush(D2D1::RadialGradientBrushProperties({x, cy}, {0, 0}, rx, ry), coll, &br)))
        g_rt->FillEllipse(D2D1::Ellipse({x, cy}, rx, ry), br);
    SafeRelease(br);
    SafeRelease(coll);
}

static int TypingKind(int v) { return v == 1 ? BK_SPARKLE : v == 2 ? BK_CONFETTI : v == 3 ? BK_BUBBLES : BK_NONE; }
static int DeletingKind(int v) { return v == 1 ? BK_EXPLOSION : v == 2 ? BK_SHATTER : v == 3 ? BK_DUST : BK_NONE; }
static D2D1_COLOR_F DeleteColor(int kind, D2D1_COLOR_F caret) {
    return kind == BK_SHATTER ? g_th.text : kind == BK_DUST ? g_th.dim : caret;
}

// The real caret, called by DrawTextArea once the text is drawn (outside its clip, so glows and
// bits can spill over the edges). tx: where the text starts on screen; op: blink; h: height.
static void DrawCaretAndFx(IDWriteTextLayout* lay, int fullLen, float tx, float cy, float op, float h, double now) {
    using namespace Lay;
    D2D1_COLOR_F col = CaretColorNow(now);
    for (const CaretEvent& ev : g_caretEvents) {
        int idx = std::clamp(ev.index, 0, fullLen);
        if (ev.typed) {
            int kind = TypingKind(CaretOptEff(CS_TYPING));
            if (kind) g_bursts.push_back({kind, tx + CaretXFor(lay, std::min(idx + ev.count, fullLen), fullLen), cy, now, g_burstSeed++, 0, col, 0});
        } else {
            int kind = DeletingKind(CaretOptEff(CS_DELETING));
            float w = std::min(ev.count, 12) * 7.f;  // the deleted text was about this wide, right of idx
            if (kind)
                g_bursts.push_back({kind, tx + CaretXFor(lay, idx, fullLen) + w / 2, cy, now, g_burstSeed++, w,
                                    DeleteColor(kind, col), 0});
        }
    }
    g_caretEvents.clear();

    float cx = tx + g_caretX;
    int moving = CaretOptEff(CS_MOVING);
    bool isMoving = std::fabs(g_caretX - g_trailX) > 1.5f;
    if (!isMoving) g_fxLastX = cx;
    else if (std::fabs(cx - g_fxLastX) > 3.f) {
        if (moving == 1) g_bursts.push_back({BK_GHOST, g_fxLastX, cy, now, g_burstSeed++, 0, col, 0});
        if (moving == 2)
            for (int i = 0; i < 2; i++)
                g_bursts.push_back({BK_COMET, cx, cy, now, g_burstSeed++, 0, col, g_trailX > g_caretX ? 1.f : -1.f});
        g_fxLastX = cx;
    }

    if (op > 0.003f) {
        DrawCaretGlow(cx, cy, h, col, op, CaretOptEff(CS_GLOW), now, 1.f);
        float x0 = tx + std::min(g_caretX, g_trailX) - CaretW / 2;
        float x1 = tx + std::max(g_caretX, g_trailX) + CaretW / 2;
        if ((moving == 0 || moving == 2) && x1 - x0 > CaretW + 0.6f) {  // the soft smear behind a gliding caret
            g_solid->SetColor(WithA(col, op * 0.28f));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, cy - h * 0.38f, x1, cy + h * 0.38f), 1, 1), g_solid);
        }
        g_solid->SetColor(WithA(col, op));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(cx - CaretW / 2, cy - h / 2, cx + CaretW / 2, cy + h / 2), 1, 1),
                                   g_solid);
    }

    if (g_bursts.size() > 400) g_bursts.erase(g_bursts.begin(), g_bursts.end() - 400);
    for (const Burst& b : g_bursts) DrawBurst(b, now, 1.f, 1.f);
    g_bursts.erase(std::remove_if(g_bursts.begin(), g_bursts.end(),
                                  [&](const Burst& b) { return now - b.t >= BurstLife(b.kind); }),
                   g_bursts.end());
}

// ---- the cards ---------------------------------------------------------------------------
static float TextWidth(const std::wstring& s) {
    if (s.empty()) return 0.f;
    IDWriteTextLayout* l = nullptr;
    float w = 0;
    if (SUCCEEDED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), g_fmtText, 1000.f, 40.f, &l))) {
        DWRITE_TEXT_METRICS m;
        l->GetMetrics(&m);
        w = m.widthIncludingTrailingWhitespace;
        l->Release();
    }
    return w;
}

static void MiniCaret(float x, float cy, D2D1_COLOR_F col, float a) {
    g_solid->SetColor(WithA(col, a));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x - 1, cy - 9, x + 1, cy + 9), 1, 1), g_solid);
}

// What a card shows: a few letters and a caret playing that option.
static void DrawCaretDemo(const CaretOpt& o, float x, float y, float w, float h, double now, int k) {
    const Theme& th = g_th;
    const float tx = x + 14, cy = y + h / 2, kk = 0.85f;
    static const std::wstring word = L"Hello";
    auto text = [&](const std::wstring& s, float a) {
        if (!s.empty()) DrawLayout(s, g_fmtText, tx, cy - 28, w - 20, 56, WithA(th.text, a));
    };
    int colorV = o.sec == CS_COLOR ? o.val : g_caretSet[CS_COLOR];
    D2D1_COLOR_F col = CaretColorOf(colorV, now + (o.sec == CS_COLOR ? 0 : k * 0.05));
    int glow = o.sec == CS_GLOW ? o.val : o.sec == CS_COLOR ? g_caretSet[CS_GLOW] : 0;
    float blink = 0.55f + 0.45f * (float)std::cos(std::fmod(now + k * 0.13, 1.1) / 1.1 * 6.2831853);
    switch (o.sec) {
    case CS_COLOR:
    case CS_GLOW: {
        float cx = tx + TextWidth(L"Abc") + 1;
        text(L"Abc", 0.9f);
        DrawCaretGlow(cx, cy, 18, col, 1.f, glow, now, kk);
        MiniCaret(cx, cy, col, glow ? 1.f : std::max(blink, 0.35f));
        break;
    }
    case CS_TYPING: {
        const double P = 2.6;
        double ph = std::fmod(now + k * 0.37, P);
        int shown = 0;
        for (int j = 0; j < 5; j++)
            if (ph >= 0.15 + j * 0.3) shown = j + 1;
        text(word.substr(0, shown), 0.9f);
        int kind = TypingKind(o.val);
        for (int j = 0; j < shown && kind; j++) {
            double age = ph - (0.15 + j * 0.3);
            DrawBurst({kind, tx + TextWidth(word.substr(0, j + 1)) + 1, cy, now - age, (UINT32)(j * 7 + k * 31 + 1), 0, col, 0}, now, kk, 1.f);
        }
        MiniCaret(tx + TextWidth(word.substr(0, shown)) + 1, cy, col, 1.f);
        break;
    }
    case CS_DELETING: {
        const double P = 2.6;
        double ph = std::fmod(now + k * 0.37, P);
        int gone = 0;
        for (int j = 0; j < 5; j++)
            if (ph >= 0.5 + j * 0.3) gone = j + 1;
        text(word.substr(0, 5 - gone), 0.9f * Clamp01(ph / 0.2));
        int kind = DeletingKind(o.val);
        for (int j = 0; j < gone && kind; j++) {
            double age = ph - (0.5 + j * 0.3);
            float x0 = TextWidth(word.substr(0, 4 - j)), x1 = TextWidth(word.substr(0, 5 - j));
            DrawBurst({kind, tx + (x0 + x1) / 2 + 1, cy, now - age, (UINT32)(j * 7 + k * 31 + 1), x1 - x0, DeleteColor(kind, col), 0}, now,
                      kk, 1.f);
        }
        MiniCaret(tx + TextWidth(word.substr(0, 5 - gone)) + 1, cy, col, 1.f);
        break;
    }
    default: {  // moving: the caret jumps between the two ends of the word and glides there
        text(word, 0.9f);
        float wEnd = TextWidth(word) + 1;
        const double J = 1.0;
        double tt = now + k * 0.29;
        long long jump = (long long)std::floor(tt / J);
        float tau = (float)(tt - jump * J);
        float a = (jump % 2) ? 1.f : wEnd, b = (jump % 2) ? wEnd : 1.f;
        auto at = [&](float t, float speed) { return a + (b - a) * (1 - std::exp(-speed * std::max(0.f, t))); };
        float cx = tx + at(tau, 32.f), trail = tx + at(tau, 13.f);
        if (o.val == 0 || o.val == 2) {
            float x0 = std::min(cx, trail) - 1, x1 = std::max(cx, trail) + 1;
            if (x1 - x0 > 2.6f) {
                g_solid->SetColor(WithA(col, 0.28f));
                g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x0, cy - 7, x1, cy + 7), 1, 1), g_solid);
            }
        }
        if (o.val == 1)
            for (int i = 1; i <= 7; i++) {
                float te = tau - i * 0.03f;
                if (te < 0 || std::fabs(at(te, 32.f) - at(te + 0.03f, 32.f)) < 0.5f) continue;
                DrawBurst({BK_GHOST, tx + at(te, 32.f), cy, now - i * 0.03f, 0, 0, col, 0}, now, kk, 1.f);
            }
        if (o.val == 2)
            for (int i = 0; i < 16; i++) {
                float te = tau - i * 0.025f;
                if (te < 0 || std::fabs(b - a) * 32 * std::exp(-32 * te) < 40) continue;
                for (int p = 0; p < 2; p++)
                    DrawBurst({BK_COMET, tx + at(te, 32.f), cy, now - i * 0.025f, (UINT32)(jump * 97 + i * 2 + p), 0, col,
                               b > a ? -1.f : 1.f},
                              now, kk, 1.f);
            }
        MiniCaret(cx, cy, col, 1.f);
        break;
    }
    }
}

static void DrawCaretGrid(float top, float dt, float alpha, double now) {
    using namespace Lay;
    CardGrid& G = g_caretGrid;
    const float cw = G.CardW(), PrevH = G.prevH, NameH = G.nameH;
    const Theme& th = g_th;
    DrawLayout(L"Caret", g_fmtHeader, Margin + GridPad + 2, top + GridPad - 4, 200, GridHeaderH, WithA(th.dim, alpha));
    DrawLayout(L"← → try it   ↵ apply", g_fmtRowRight, Margin + BarW - GridPad - 260, top + GridPad - 4, 258, GridHeaderH,
               WithA(th.dim, alpha * 0.85f));

    float cTop = top + CardGrid::TopH, cBottom = top + g_panelH;
    G.Animate(dt);
    g_rt->PushAxisAlignedClip(D2D1::RectF(Margin, cTop, Margin + BarW, cBottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    if (g_caretItems.empty()) DrawLayout(L"Nothing matches", g_fmtRow, Margin + GridPad + 2, cTop, 300, 40, WithA(th.dim, alpha));
    for (auto& hd : G.headers) {
        float y = cTop + hd.second - G.off;
        if (y > cBottom || y + G.headH < cTop) continue;
        DrawLayout(hd.first, g_fmtHeader, Margin + GridPad + 2, y, 400, G.headH - 2, WithA(th.dim, alpha * 0.9f));
    }

    D2D1_MATRIX_3X2_F baseRaw;
    g_rt->GetTransform(&baseRaw);
    Matrix3x2F base = Matrix3x2F::ReinterpretBaseType(&baseRaw)[0];
    for (int k = 0; k < (int)G.cells.size(); k++) {
        const GridCell& c = G.cells[k];
        const CaretOpt& o = g_caretItems[c.item];
        float x = c.x, y = cTop + c.y - G.off;
        G.cellSel[k] = Damp(G.cellSel[k], k == g_sel ? 1.f : 0.f, 16.f, dt);
        if (y > cBottom || y + G.RowH() < cTop) continue;
        float sel = G.cellSel[k];
        float cardA = alpha * Clamp01(g_contentA * 1.3 - (k % G.cols) * 0.04);
        D2D1_RECT_F card = D2D1::RectF(x, y, x + cw, y + PrevH);
        D2D1_RECT_F hit = D2D1::RectF(x, std::max(y, cTop), x + cw, std::min(y + PrevH + NameH, cBottom));
        if (hit.bottom > hit.top) g_hits.push_back({hit, k});

        float sk = 1.f + 0.035f * sel;
        Matrix3x2F m = Matrix3x2F::Scale(sk, sk, {x + cw / 2, y + PrevH / 2}) * base;
        g_rt->SetTransform(m);
        g_rt->PushLayer(D2D1::LayerParameters(card, nullptr, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1::IdentityMatrix(), cardA),
                        nullptr);
        D2D1_ROUNDED_RECT rr = D2D1::RoundedRect(card, 9, 9);
        FillAcrylic(rr, th, card.top, card.bottom);
        if (ID2D1LinearGradientBrush* b = MakeGradient({0, card.top}, {0, card.bottom}, th.borderTop, th.borderBottom)) {
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + .5f, y + .5f, x + cw - .5f, y + PrevH - .5f), 8.5f, 8.5f), b, 1.f);
            b->Release();
        }
        g_rt->PushAxisAlignedClip(card, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        DrawCaretDemo(o, x, y, cw, PrevH, now, k);
        g_rt->PopAxisAlignedClip();
        g_rt->PopLayer();

        if (sel > 0.01f) {
            g_solid->SetColor(WithA(th.accent, cardA * sel));
            g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x - 3, y - 3, x + cw + 3, y + PrevH + 3), 12, 12), g_solid, 2.f);
        }
        if (g_caretSet[o.sec] == o.val) {  // the one in use
            D2D1_POINT_2F bc = {x + cw - 13, y + 13};
            g_solid->SetColor(WithA(th.accent, cardA));
            g_rt->FillEllipse(D2D1::Ellipse(bc, 8, 8), g_solid);
            DrawLayout(L"\xE73E", g_fmtGlyphSm, bc.x - 8, bc.y - 8, 16, 16, WithA(C(0xFFFFFF), cardA));
        }
        g_rt->SetTransform(base);

        D2D1_COLOR_F nc = LerpC(th.dim, th.text, std::max(sel, g_caretSet[o.sec] == o.val ? 0.7f : 0.f));
        DrawLayout(CaretOptName(o.sec, o.val), g_fmtCard, x, y + PrevH + 2, cw, NameH - 4, WithA(nc, cardA));
    }
    g_rt->PopAxisAlignedClip();

    float view = cBottom - cTop;
    if (G.MaxOff() > 0.5f) DrawScrollBar(cTop + 4, view - 8, G.off / G.MaxOff(), view / (G.contentH + 6), alpha, dt);
}

// ---- glue for main.cpp ------------------------------------------------------------------
static bool CaretKey(WPARAM vk, bool shift) {
    int k = g_caretGrid.Key(vk, shift, g_sel);
    if (k < 0) return false;
    Select(k);
    g_caretGrid.EnsureVisible(g_sel, false);
    return true;
}
static void CaretClick(float x, float y) {
    int k = HitItemAt(x, y);
    if (k >= 0 && k < (int)g_caretItems.size()) {
        g_sel = k;
        CaretApply(k);
    }
}
static void CaretWheel(int steps) { g_caretGrid.ScrollTo(g_caretGrid.offTarget + steps * 64.f, false); }
