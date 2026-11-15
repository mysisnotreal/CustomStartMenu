// ---------------------------------------------------------------------------
// Typed straight into the bar (no ">"): math ("12*7.5"), units ("5 km in mi"), currency
// ("20 usd in eur") and web shortcuts ("g cats", "yt lofi"). The answer shows on the right
// of the bar; Enter copies it (or opens the search).
// ---------------------------------------------------------------------------

// ---- math ---------------------------------------------------------------------------------
struct CalcParser {
    const std::wstring& s;
    size_t i = 0;
    bool ok = true;
    bool sawOp = false;  // "2048" on its own is not a sum
    explicit CalcParser(const std::wstring& str) : s(str) {}
    void Ws() { while (i < s.size() && std::iswspace(s[i])) i++; }
    bool Eat(wchar_t c) {
        Ws();
        if (i < s.size() && s[i] == c) { i++; return true; }
        return false;
    }
    double Expr() {
        double v = Term();
        for (;;) {
            if (Eat(L'+')) { sawOp = true; v += Term(); }
            else if (Eat(L'-') || Eat(L'−')) { sawOp = true; v -= Term(); }
            else return v;
        }
    }
    double Term() {
        double v = Power();
        for (;;) {
            Ws();
            if (Eat(L'*') || Eat(L'×') || Eat(L'x')) { sawOp = true; v *= Power(); }
            else if (Eat(L'/') || Eat(L'÷')) { sawOp = true; v /= Power(); }
            else if (Eat(L'%')) {
                // "50 % 3" is a remainder; "20%" alone is a percent
                Ws();
                if (i < s.size() && (std::iswdigit(s[i]) || s[i] == L'(' || s[i] == L'.')) { sawOp = true; v = std::fmod(v, Power()); }
                else { sawOp = true; v /= 100.0; }
            } else if (i < s.size() && (s[i] == L'(' || std::iswalpha(s[i])) && !IsWordAhead(L"in") && !IsWordAhead(L"to")) {
                sawOp = true;  // "2(3+4)", "2pi"
                v *= Power();
            } else return v;
        }
    }
    bool IsWordAhead(const wchar_t* w) {
        size_t n = wcslen(w);
        return s.compare(i, n, w) == 0 && (i + n >= s.size() || !std::iswalpha(s[i + n]));
    }
    double Power() {
        double b = Unary();
        if (Eat(L'^')) { sawOp = true; return std::pow(b, Power()); }
        return b;
    }
    double Unary() {
        if (Eat(L'-') || Eat(L'−')) return -Unary();
        if (Eat(L'+')) return Unary();
        return Atom();
    }
    double Atom() {
        Ws();
        if (Eat(L'(')) {
            double v = Expr();
            if (!Eat(L')')) ok = false;
            return v;
        }
        if (i < s.size() && (std::iswdigit(s[i]) || s[i] == L'.')) {
            size_t st = i;
            while (i < s.size() && (std::iswdigit(s[i]) || s[i] == L'.' || s[i] == L',' || s[i] == L'_')) i++;
            std::wstring num;
            for (size_t k = st; k < i; k++)
                if (s[k] != L',' && s[k] != L'_') num.push_back(s[k]);
            if (i < s.size() && (s[i] == L'e' || s[i] == L'E') && i + 1 < s.size() &&
                (std::iswdigit(s[i + 1]) || ((s[i + 1] == L'-' || s[i + 1] == L'+') && i + 2 < s.size() && std::iswdigit(s[i + 2])))) {
                num.push_back(L'e');
                i++;
                if (s[i] == L'-' || s[i] == L'+') num.push_back(s[i++]);
                while (i < s.size() && std::iswdigit(s[i])) num.push_back(s[i++]);
            }
            wchar_t* end = nullptr;
            double v = wcstod(num.c_str(), &end);
            if (!end || *end) ok = false;
            return v;
        }
        if (i < s.size() && std::iswalpha(s[i])) {
            size_t st = i;
            while (i < s.size() && std::iswalpha(s[i])) i++;
            std::wstring w = Lower(s.substr(st, i - st));
            if (w == L"pi" || w == L"π") return 3.14159265358979323846;
            if (w == L"e") return 2.71828182845904523536;
            static const struct { const wchar_t* n; double (*f)(double); } fns[] = {
                {L"sqrt", [](double x) { return std::sqrt(x); }}, {L"abs", [](double x) { return std::fabs(x); }},
                {L"sin", [](double x) { return std::sin(x); }},   {L"cos", [](double x) { return std::cos(x); }},
                {L"tan", [](double x) { return std::tan(x); }},   {L"asin", [](double x) { return std::asin(x); }},
                {L"acos", [](double x) { return std::acos(x); }}, {L"atan", [](double x) { return std::atan(x); }},
                {L"ln", [](double x) { return std::log(x); }},    {L"log", [](double x) { return std::log10(x); }},
                {L"exp", [](double x) { return std::exp(x); }},   {L"round", [](double x) { return std::round(x); }},
                {L"floor", [](double x) { return std::floor(x); }}, {L"ceil", [](double x) { return std::ceil(x); }},
            };
            for (auto& f : fns)
                if (w == f.n) {
                    sawOp = true;
                    return f.f(Atom());
                }
            ok = false;
            return 0;
        }
        if (i < s.size() && s[i] == L'π') { i++; return 3.14159265358979323846; }
        ok = false;
        return 0;
    }
};

// 0.1+0.2 -> "0.3"; big and tiny numbers in e-notation.
static std::wstring FormatNumber(double v, int sig = 12) {
    if (std::isnan(v)) return L"not a number";
    if (std::isinf(v)) return v > 0 ? L"∞" : L"-∞";
    if (v == 0) return L"0";
    wchar_t buf[64];
    double a = std::fabs(v);
    if (a >= 1e15 || a < 1e-7) swprintf(buf, 64, L"%.10g", v);
    else {
        swprintf(buf, 64, L"%.10f", v);
        // keep 12 significant digits, then trim zeros
        int intDigits = a >= 1 ? (int)std::floor(std::log10(a)) + 1 : 1;
        int dec = std::clamp(sig - intDigits, 0, 10);
        swprintf(buf, 64, L"%.*f", dec, v);
        std::wstring s = buf;
        if (s.find(L'.') != std::wstring::npos) {
            while (!s.empty() && s.back() == L'0') s.pop_back();
            if (!s.empty() && s.back() == L'.') s.pop_back();
        }
        if (s == L"-0") s = L"0";
        return s;
    }
    return buf;
}

static bool EvalMath(const std::wstring& text, double& out) {
    std::wstring t = Trim(text);
    if (t.empty()) return false;
    bool forced = t[0] == L'=';
    if (forced) t = t.substr(1);
    bool digit = false;
    for (wchar_t c : t) digit |= std::iswdigit(c) != 0;
    if (!digit && t.find(L"pi") == std::wstring::npos) return false;
    CalcParser p(t);
    double v = p.Expr();
    p.Ws();
    if (!p.ok || p.i != t.size() || (!p.sawOp && !forced)) return false;
    out = v;
    return true;
}

// ---- units ----------------------------------------------------------------------------------
struct UnitDef {
    const wchar_t* names;  // space separated, the first is shown
    int kind;              // only the same kind converts
    double factor;         // to the kind's base unit
};
enum { U_LEN, U_MASS, U_VOL, U_TEMP, U_SPEED, U_DATA, U_TIME, U_AREA };
static const UnitDef kUnits[] = {
    {L"mm millimeter millimeters millimetre millimetres", U_LEN, 0.001},
    {L"cm centimeter centimeters centimetre centimetres", U_LEN, 0.01},
    {L"m meter meters metre metres", U_LEN, 1},
    {L"km kilometer kilometers kilometre kilometres", U_LEN, 1000},
    {L"in inch inches \"", U_LEN, 0.0254},
    {L"ft foot feet '", U_LEN, 0.3048},
    {L"yd yard yards", U_LEN, 0.9144},
    {L"mi mile miles", U_LEN, 1609.344},
    {L"nmi nauticalmile nauticalmiles", U_LEN, 1852},
    {L"mg milligram milligrams", U_MASS, 0.001},
    {L"g gram grams", U_MASS, 1},
    {L"kg kilogram kilograms kilo kilos", U_MASS, 1000},
    {L"t tonne tonnes ton tons", U_MASS, 1e6},
    {L"oz ounce ounces", U_MASS, 28.349523125},
    {L"lb lbs pound pounds", U_MASS, 453.59237},
    {L"st stone stones", U_MASS, 6350.29318},
    {L"ml milliliter milliliters millilitre millilitres", U_VOL, 0.001},
    {L"cl centiliter centiliters", U_VOL, 0.01},
    {L"l liter liters litre litres", U_VOL, 1},
    {L"tsp teaspoon teaspoons", U_VOL, 0.00492892159375},
    {L"tbsp tablespoon tablespoons", U_VOL, 0.01478676478125},
    {L"floz fl-oz fluidounce fluidounces", U_VOL, 0.0295735295625},
    {L"cup cups", U_VOL, 0.2365882365},
    {L"pt pint pints", U_VOL, 0.473176473},
    {L"qt quart quarts", U_VOL, 0.946352946},
    {L"gal gallon gallons", U_VOL, 3.785411784},
    {L"°c c celsius", U_TEMP, 0},
    {L"°f f fahrenheit", U_TEMP, 1},
    {L"k kelvin", U_TEMP, 2},
    {L"km/h kmh kph", U_SPEED, 1 / 3.6},
    {L"mph", U_SPEED, 0.44704},
    {L"m/s mps", U_SPEED, 1},
    {L"kn knot knots", U_SPEED, 0.514444},
    {L"bit bits", U_DATA, 0.125},
    {L"b byte bytes", U_DATA, 1},
    {L"kb kilobyte kilobytes", U_DATA, 1e3},
    {L"mb megabyte megabytes", U_DATA, 1e6},
    {L"gb gigabyte gigabytes", U_DATA, 1e9},
    {L"tb terabyte terabytes", U_DATA, 1e12},
    {L"kib", U_DATA, 1024.0},
    {L"mib", U_DATA, 1048576.0},
    {L"gib", U_DATA, 1073741824.0},
    {L"tib", U_DATA, 1099511627776.0},
    {L"ms millisecond milliseconds", U_TIME, 0.001},
    {L"s sec secs second seconds", U_TIME, 1},
    {L"min mins minute minutes", U_TIME, 60},
    {L"h hr hrs hour hours", U_TIME, 3600},
    {L"d day days", U_TIME, 86400},
    {L"wk week weeks", U_TIME, 604800},
    {L"yr year years", U_TIME, 31557600},
    {L"m2 m² sqm", U_AREA, 1},
    {L"km2 km²", U_AREA, 1e6},
    {L"ft2 ft² sqft", U_AREA, 0.09290304},
    {L"acre acres", U_AREA, 4046.8564224},
    {L"ha hectare hectares", U_AREA, 10000},
};
static int FindUnit(const std::wstring& w) {
    for (int i = 0; i < (int)(sizeof(kUnits) / sizeof(kUnits[0])); i++) {
        std::wstring names = kUnits[i].names;
        size_t p = 0;
        while (p < names.size()) {
            size_t e = names.find(L' ', p);
            if (e == std::wstring::npos) e = names.size();
            if (names.compare(p, e - p, w) == 0) return i;
            p = e + 1;
        }
    }
    return -1;
}
static std::wstring UnitName(int u) {
    std::wstring n = kUnits[u].names;
    return n.substr(0, n.find(L' '));
}
static double ToBaseTemp(double v, int u) { return u == 0 ? v : u == 1 ? (v - 32) * 5 / 9 : v - 273.15; }
static double FromBaseTemp(double c, int u) { return u == 0 ? c : u == 1 ? c * 9 / 5 + 32 : c + 273.15; }

// ---- currency (rates from open.er-api.com, refreshed twice a day) -----------------------
static std::mutex g_ratesMx;
static std::unordered_map<std::wstring, double> g_rates;  // per 1 USD
static double g_ratesAt = 0;          // unix time
static std::atomic<bool> g_ratesLoading{false};
static const wchar_t* kCurrencySymbols[][2] = {{L"$", L"usd"}, {L"€", L"eur"}, {L"£", L"gbp"}, {L"¥", L"jpy"}, {L"₹", L"inr"}, {L"₩", L"krw"}, {L"₽", L"rub"}, {L"₺", L"try"}, {L"₪", L"ils"}, {L"₱", L"php"}, {L"₫", L"vnd"}, {L"฿", L"thb"}};
static const wchar_t* kCurrencyCodes =
    L" usd eur gbp jpy cny cad aud nzd chf sek nok dkk pln czk huf ron bgn try rub uah inr pkr bdt lkr idr myr sgd thb vnd php"
    L" krw hkd twd aed sar qar kwd ils egp zar ngn kes mad mxn brl ars clp cop pen uyu isk ";
static bool IsCurrency(const std::wstring& w) {
    if (w.size() != 3) return false;
    {
        std::lock_guard<std::mutex> lk(g_ratesMx);
        if (g_rates.count(w)) return true;
    }
    return std::wstring(kCurrencyCodes).find(L" " + w + L" ") != std::wstring::npos;
}
static double UnixNow() {
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER u{{ft.dwLowDateTime, ft.dwHighDateTime}};
    return (double)(u.QuadPart - 116444736000000000ULL) / 1e7;
}
static void ParseRates(const std::string& js) {
    size_t r = js.find("\"rates\"");
    if (r == std::string::npos) return;
    std::unordered_map<std::wstring, double> m;
    size_t p = js.find('{', r);
    while (p != std::string::npos) {
        size_t q1 = js.find('"', p);
        if (q1 == std::string::npos) break;
        size_t q2 = js.find('"', q1 + 1);
        size_t colon = js.find(':', q2);
        if (q2 == std::string::npos || colon == std::string::npos) break;
        std::string code = js.substr(q1 + 1, q2 - q1 - 1);
        double v = atof(js.c_str() + colon + 1);
        if (code.size() == 3 && v > 0) m[Lower(Utf8ToW(code))] = v;
        size_t next = js.find_first_of(",}", colon);
        if (next == std::string::npos || js[next] == '}') break;
        p = next;
    }
    if (m.size() > 10) {
        std::lock_guard<std::mutex> lk(g_ratesMx);
        g_rates = m;
    }
}
static void LoadRates() {  // starts a refresh when they're missing or old
    if (g_ratesLoading) return;
    double age;
    {
        std::lock_guard<std::mutex> lk(g_ratesMx);
        if (g_rates.empty()) {
            // the copy saved last time
            std::wstring cache = AppDir() + L"\\rates.json";
            HANDLE f = CreateFileW(cache.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING, 0, nullptr);
            if (f != INVALID_HANDLE_VALUE) {
                std::string js;
                DWORD sz = GetFileSize(f, nullptr), got = 0;
                if (sz < (1u << 20)) {
                    js.resize(sz);
                    ReadFile(f, &js[0], sz, &got, nullptr);
                }
                FILETIME wt;
                GetFileTime(f, nullptr, nullptr, &wt);
                CloseHandle(f);
                ULARGE_INTEGER u{{wt.dwLowDateTime, wt.dwHighDateTime}};
                g_ratesAt = (double)(u.QuadPart - 116444736000000000ULL) / 1e7;
                g_ratesMx.unlock();
                ParseRates(js);
                g_ratesMx.lock();
            }
        }
        age = UnixNow() - g_ratesAt;
    }
    if (age < 12 * 3600 && !g_rates.empty()) return;
    g_ratesLoading = true;
    std::thread([] {
        std::string js;
        if (HttpGet(L"https://open.er-api.com/v6/latest/USD", &js, L"", nullptr) && js.find("\"rates\"") != std::string::npos) {
            ParseRates(js);
            {
                std::lock_guard<std::mutex> lk(g_ratesMx);
                g_ratesAt = UnixNow();
            }
            std::wstring cache = AppDir() + L"\\rates.json";
            HANDLE f = CreateFileW(cache.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, 0, nullptr);
            if (f != INVALID_HANDLE_VALUE) {
                DWORD w = 0;
                WriteFile(f, js.data(), (DWORD)js.size(), &w, nullptr);
                CloseHandle(f);
            }
        }
        g_ratesLoading = false;
        PostMessageW(g_hwnd, WM_APP_PANEL, 1, 0);
    }).detach();
}

// "5 km in mi", "100f to c", "20 usd in eur", "$20 to eur". result is the number alone
// (copied by Enter), shown is what the bar shows.
static bool ConvertParts(std::wstring left, std::wstring to, std::wstring& result, std::wstring& shown);
static bool Convert(const std::wstring& text, std::wstring& result, std::wstring& shown) {
    std::wstring t = Lower(Trim(text));
    // "5 in to cm": try every " in " / " to " until one splits into two known units
    for (const wchar_t* w : {L" in ", L" to ", L" as ", L" -> ", L"->"}) {
        size_t n = wcslen(w);
        for (size_t p = t.find(w); p != std::wstring::npos; p = t.find(w, p + 1))
            if (p > 0 && ConvertParts(Trim(t.substr(0, p)), Trim(t.substr(p + n)), result, shown)) return true;
    }
    return false;
}
static bool ConvertParts(std::wstring left, std::wstring to, std::wstring& result, std::wstring& shown) {
    if (left.empty() || to.empty()) return false;
    for (auto& s : kCurrencySymbols) {  // "$20" -> "20 usd"
        if (StartsWith(left, s[0])) left = Trim(left.substr(wcslen(s[0]))) + L" " + s[1];
        if (to == s[0]) to = s[1];
    }
    // split the left side into the amount and its unit: the unit is the trailing letters
    size_t k = left.size();
    while (k > 0 && !std::iswdigit(left[k - 1]) && left[k - 1] != L')' && left[k - 1] != L'.') k--;
    std::wstring amountS = Trim(left.substr(0, k)), from = Trim(left.substr(k));
    if (from.empty()) return false;
    double amount = 1;
    if (!amountS.empty()) {
        CalcParser p(amountS);
        amount = p.Expr();
        p.Ws();
        if (!p.ok || p.i != amountS.size()) return false;
    }
    if (from.size() > 1 && from.back() == L'.') from.pop_back();
    int uf = FindUnit(from), ut = FindUnit(to);
    if (uf >= 0 && ut >= 0 && kUnits[uf].kind == kUnits[ut].kind) {
        double v;
        if (kUnits[uf].kind == U_TEMP) v = FromBaseTemp(ToBaseTemp(amount, (int)kUnits[uf].factor), (int)kUnits[ut].factor);
        else v = amount * kUnits[uf].factor / kUnits[ut].factor;
        result = FormatNumber(v, 6);
        shown = L"= " + result + L" " + UnitName(ut);
        return true;
    }
    if (IsCurrency(from) && IsCurrency(to)) {
        LoadRates();
        std::lock_guard<std::mutex> lk(g_ratesMx);
        auto a = g_rates.find(from), b = g_rates.find(to);
        if (a == g_rates.end() || b == g_rates.end()) {
            result.clear();
            shown = g_ratesLoading ? L"= …" : L"No rate for that";
            return true;
        }
        double v = amount / a->second * b->second;
        wchar_t buf[64];
        swprintf(buf, 64, std::fabs(v) >= 100 ? L"%.2f" : L"%.4g", v);
        result = buf;
        std::wstring up = to;
        for (auto& c : up) c = (wchar_t)towupper(c);
        shown = L"= " + result + L" " + up;
        return true;
    }
    return false;
}

// ---- web shortcuts ---------------------------------------------------------------------------
struct WebShortcut {
    std::wstring key, name, url;  // url has %s where the search goes
};
static std::vector<WebShortcut> g_webShortcuts;
static double g_webLoadedAt = -1e9;
static std::wstring SearchesFile() { return AppDir() + L"\\Searches.txt"; }
static void LoadWebShortcuts() {
    if (Now() - g_webLoadedAt < 5) return;
    g_webLoadedAt = Now();
    g_webShortcuts = {
        {L"g", L"Google", L"https://www.google.com/search?q=%s"},
        {L"yt", L"YouTube", L"https://www.youtube.com/results?search_query=%s"},
        {L"w", L"Wikipedia", L"https://en.wikipedia.org/w/index.php?search=%s"},
        {L"gh", L"GitHub", L"https://github.com/search?q=%s"},
        {L"r", L"Reddit", L"https://www.reddit.com/search/?q=%s"},
        {L"a", L"Amazon", L"https://www.amazon.com/s?k=%s"},
        {L"maps", L"Maps", L"https://www.google.com/maps/search/%s"},
        {L"img", L"Images", L"https://www.google.com/search?tbm=isch&q=%s"},
        {L"ddg", L"DuckDuckGo", L"https://duckduckgo.com/?q=%s"},
        {L"b", L"Bing", L"https://www.bing.com/search?q=%s"},
        {L"tr", L"Translate", L"https://translate.google.com/?sl=auto&text=%s"},
    };
    // Searches.txt: "key  Name  https://...%s" per line (the name is optional)
    std::wstring t = ReadTextFile(SearchesFile());
    size_t p = 0;
    while (p < t.size()) {
        size_t e = t.find(L'\n', p);
        if (e == std::wstring::npos) e = t.size();
        std::wstring line = Trim(t.substr(p, e - p));
        p = e + 1;
        if (line.empty() || line[0] == L'#') continue;
        size_t sp = line.find_first_of(L" \t");
        size_t u = line.find(L"://");
        if (sp == std::wstring::npos || u == std::wstring::npos) continue;
        size_t us = line.find_last_of(L" \t", u);
        WebShortcut w;
        w.key = Lower(line.substr(0, sp));
        w.url = Trim(line.substr(us + 1));
        w.name = Trim(line.substr(sp, us - sp));
        if (w.name.empty()) w.name = w.key;
        g_webShortcuts.erase(std::remove_if(g_webShortcuts.begin(), g_webShortcuts.end(),
                                            [&](const WebShortcut& o) { return o.key == w.key; }),
                             g_webShortcuts.end());
        g_webShortcuts.push_back(w);
    }
}

// What Enter does for typed text that isn't an app: the inline answer.

// Called by UpdateState for text without ">". Returns the hint to show, or "".
static std::wstring InlineAnswer(const std::wstring& text, bool appStartsWithText) {
    g_inlineCopy.clear();
    g_inlineUrl.clear();
    std::wstring t = Trim(text);
    if (t.empty()) return L"";
    std::wstring result, shown;
    if (Convert(t, result, shown)) {
        g_inlineCopy = result;
        return shown;
    }
    double v;
    if (EvalMath(t, v)) {
        g_inlineCopy = FormatNumber(v);
        return L"= " + g_inlineCopy;
    }
    if (appStartsWithText) return L"";
    size_t sp = t.find(L' ');
    if (sp != std::wstring::npos) {
        LoadWebShortcuts();
        std::wstring k = Lower(t.substr(0, sp)), q = Trim(t.substr(sp + 1));
        for (auto& w : g_webShortcuts)
            if (w.key == k && !q.empty()) {
                std::wstring url = w.url;
                size_t at = url.find(L"%s");
                if (at != std::wstring::npos) url.replace(at, 2, UrlEncode(q));
                g_inlineUrl = url;
                return w.name + L": " + q;
            }
    }
    return L"";
}

static bool InlineActivate() {
    if (!g_inlineUrl.empty()) {
        std::wstring url = g_inlineUrl;
        g_prevFg = nullptr;
        Hide();
        ShellExecuteW(nullptr, L"open", url.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        return true;
    }
    if (!g_inlineCopy.empty()) {
        if (SetClipboardText(g_inlineCopy)) Flash(L"Copied " + g_inlineCopy);
        return true;
    }
    return false;
}

// "> searches": the shortcuts, and the file to add your own.
static void SearchesFill(bool) {
    LoadWebShortcuts();
    g_prows.clear();
    PRow f;
    f.glyph = L"\xE70F";
    f.title = L"Add your own…";
    f.detail = L"Searches.txt";
    f.action = L"Edit  ↵";
    f.tag = -1;
    g_prows.push_back(f);
    std::wstring a = Lower(g_panelArg);
    for (int i = 0; i < (int)g_webShortcuts.size(); i++) {
        auto& w = g_webShortcuts[i];
        if (!a.empty() && w.key.find(a) == std::wstring::npos && Lower(w.name).find(a) == std::wstring::npos) continue;
        PRow r;
        r.glyph = L"\xE721";
        r.title = w.name;
        r.detail = w.key + L" …";
        r.action = L"Type “" + w.key + L" ”  ↵";
        r.tag = i;
        g_prows.push_back(r);
    }
}
static void SearchesEnter(int sel, bool) {
    if (sel < 0 || sel >= (int)g_prows.size()) return;
    int tag = g_prows[sel].tag;
    if (tag < 0) {
        std::wstring f = SearchesFile();
        if (!FileExists(f)) {
            std::string seed =
                "# Your own web searches: a shortcut, a name, and the address with %s where the search goes.\r\n"
                "# Then type the shortcut and a search in the bar, e.g. \"so how to center a div\".\r\n"
                "so  Stack Overflow  https://stackoverflow.com/search?q=%s\r\n";
            HANDLE h = CreateFileW(f.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_NEW, 0, nullptr);
            if (h != INVALID_HANDLE_VALUE) {
                DWORD w = 0;
                WriteFile(h, seed.data(), (DWORD)seed.size(), &w, nullptr);
                CloseHandle(h);
            }
        }
        g_prevFg = nullptr;
        Hide();
        ShellExecuteW(nullptr, L"open", L"notepad.exe", (L"\"" + f + L"\"").c_str(), nullptr, SW_SHOWNORMAL);
        g_webLoadedAt = -1e9;
        return;
    }
    if (tag < (int)g_webShortcuts.size()) SetText(g_webShortcuts[tag].key + L" ");
}
