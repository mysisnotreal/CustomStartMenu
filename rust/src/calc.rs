// Typed straight into the bar (no ">"): math ("12*7.5"), units ("5 km in mi"), currency
// ("20 usd in eur") and web shortcuts ("g cats", "yt lofi"). The answer shows on the right
// of the bar; Enter copies it (or opens the search). Same as cpp/calc.inl.

use super::panels::*;
use super::*;
use std::sync::atomic::AtomicBool;

// ---- math ---------------------------------------------------------------------------------
struct Parser {
    s: Vec<char>,
    i: usize,
    ok: bool,
    saw_op: bool, // "2048" on its own is not a sum
}
impl Parser {
    fn new(s: &str) -> Self {
        Parser { s: s.chars().collect(), i: 0, ok: true, saw_op: false }
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> f64 {
        let mut v = self.term();
        loop {
            if self.eat('+') {
                self.saw_op = true;
                v += self.term();
            } else if self.eat('-') || self.eat('−') {
                self.saw_op = true;
                v -= self.term();
            } else {
                return v;
            }
        }
    }
    fn word_ahead(&self, w: &str) -> bool {
        let wc: Vec<char> = w.chars().collect();
        let n = wc.len();
        self.i + n <= self.s.len() && self.s[self.i..self.i + n] == wc[..] && (self.i + n >= self.s.len() || !self.s[self.i + n].is_alphabetic())
    }
    fn term(&mut self) -> f64 {
        let mut v = self.power();
        loop {
            self.ws();
            if self.eat('*') || self.eat('×') || self.eat('x') {
                self.saw_op = true;
                v *= self.power();
            } else if self.eat('/') || self.eat('÷') {
                self.saw_op = true;
                v /= self.power();
            } else if self.eat('%') {
                // "50 % 3" is a remainder; "20%" alone is a percent
                self.ws();
                self.saw_op = true;
                if matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '(' || c == '.') {
                    v %= self.power();
                } else {
                    v /= 100.0;
                }
            } else if matches!(self.peek(), Some(c) if c == '(' || c.is_alphabetic()) && !self.word_ahead("in") && !self.word_ahead("to") {
                self.saw_op = true; // "2(3+4)", "2pi"
                v *= self.power();
            } else {
                return v;
            }
        }
    }
    fn power(&mut self) -> f64 {
        let b = self.unary();
        if self.eat('^') {
            self.saw_op = true;
            return b.powf(self.power());
        }
        b
    }
    fn unary(&mut self) -> f64 {
        if self.eat('-') || self.eat('−') {
            return -self.unary();
        }
        if self.eat('+') {
            return self.unary();
        }
        self.atom()
    }
    fn atom(&mut self) -> f64 {
        self.ws();
        if self.eat('(') {
            let v = self.expr();
            if !self.eat(')') {
                self.ok = false;
            }
            return v;
        }
        let s = &self.s;
        if matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
            let mut num = String::new();
            while self.i < s.len() && (s[self.i].is_ascii_digit() || matches!(s[self.i], '.' | ',' | '_')) {
                if s[self.i] != ',' && s[self.i] != '_' {
                    num.push(s[self.i]);
                }
                self.i += 1;
            }
            let i = self.i;
            if i + 1 < s.len()
                && (s[i] == 'e' || s[i] == 'E')
                && (s[i + 1].is_ascii_digit() || ((s[i + 1] == '-' || s[i + 1] == '+') && i + 2 < s.len() && s[i + 2].is_ascii_digit()))
            {
                num.push('e');
                self.i += 1;
                if s[self.i] == '-' || s[self.i] == '+' {
                    num.push(s[self.i]);
                    self.i += 1;
                }
                while self.i < s.len() && s[self.i].is_ascii_digit() {
                    num.push(s[self.i]);
                    self.i += 1;
                }
            }
            return match num.parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    self.ok = false;
                    0.0
                }
            };
        }
        if self.peek() == Some('π') {
            self.i += 1;
            return std::f64::consts::PI;
        }
        if matches!(self.peek(), Some(c) if c.is_alphabetic()) {
            let st = self.i;
            while self.i < s.len() && s[self.i].is_alphabetic() {
                self.i += 1;
            }
            let w: String = s[st..self.i].iter().collect::<String>().to_lowercase();
            match w.as_str() {
                "pi" | "π" => return std::f64::consts::PI,
                "e" => return std::f64::consts::E,
                _ => {}
            }
            let f: Option<fn(f64) -> f64> = match w.as_str() {
                "sqrt" => Some(f64::sqrt),
                "abs" => Some(f64::abs),
                "sin" => Some(f64::sin),
                "cos" => Some(f64::cos),
                "tan" => Some(f64::tan),
                "asin" => Some(f64::asin),
                "acos" => Some(f64::acos),
                "atan" => Some(f64::atan),
                "ln" => Some(f64::ln),
                "log" => Some(f64::log10),
                "exp" => Some(f64::exp),
                "round" => Some(f64::round),
                "floor" => Some(f64::floor),
                "ceil" => Some(f64::ceil),
                _ => None,
            };
            if let Some(f) = f {
                self.saw_op = true;
                let x = self.atom();
                return f(x);
            }
            self.ok = false;
            return 0.0;
        }
        self.ok = false;
        0.0
    }
}

// printf's %.<prec>g
pub fn g_fmt(v: f64, prec: i32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= prec {
        let m = format!("{:.*e}", (prec - 1).max(0) as usize, v);
        let (mant, e) = m.split_once('e').unwrap_or((&m, "0"));
        let mut mant = mant.to_string();
        if mant.contains('.') {
            mant = mant.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        let e: i32 = e.parse().unwrap_or(0);
        format!("{}e{}{:02}", mant, if e < 0 { '-' } else { '+' }, e.abs())
    } else {
        let s = format!("{:.*}", (prec - 1 - exp).max(0) as usize, v);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

// 0.1+0.2 -> "0.3"; big and tiny numbers in e-notation.
pub fn format_number(v: f64, sig: i32) -> String {
    if v.is_nan() {
        return "not a number".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "∞".into() } else { "-∞".into() };
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-7..1e15).contains(&a) {
        return g_fmt(v, 10);
    }
    let int_digits = if a >= 1.0 { a.log10().floor() as i32 + 1 } else { 1 };
    let dec = (sig - int_digits).clamp(0, 10) as usize;
    let mut s = format!("{:.*}", dec, v);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

fn eval_math(text: &str) -> Option<f64> {
    let mut t = text.trim();
    if t.is_empty() {
        return None;
    }
    let forced = t.starts_with('=');
    if forced {
        t = &t[1..];
    }
    if !t.chars().any(|c| c.is_ascii_digit()) && !t.contains("pi") {
        return None;
    }
    let mut p = Parser::new(t);
    let v = p.expr();
    p.ws();
    if !p.ok || p.i != p.s.len() || (!p.saw_op && !forced) {
        return None;
    }
    Some(v)
}

// ---- units ----------------------------------------------------------------------------------
const U_TEMP: i32 = 3;
// names (space separated, the first is shown), kind (only the same kind converts), factor to
// the kind's base unit
const UNITS: &[(&str, i32, f64)] = &[
    ("mm millimeter millimeters millimetre millimetres", 0, 0.001),
    ("cm centimeter centimeters centimetre centimetres", 0, 0.01),
    ("m meter meters metre metres", 0, 1.0),
    ("km kilometer kilometers kilometre kilometres", 0, 1000.0),
    ("in inch inches \"", 0, 0.0254),
    ("ft foot feet '", 0, 0.3048),
    ("yd yard yards", 0, 0.9144),
    ("mi mile miles", 0, 1609.344),
    ("nmi nauticalmile nauticalmiles", 0, 1852.0),
    ("mg milligram milligrams", 1, 0.001),
    ("g gram grams", 1, 1.0),
    ("kg kilogram kilograms kilo kilos", 1, 1000.0),
    ("t tonne tonnes ton tons", 1, 1e6),
    ("oz ounce ounces", 1, 28.349523125),
    ("lb lbs pound pounds", 1, 453.59237),
    ("st stone stones", 1, 6350.29318),
    ("ml milliliter milliliters millilitre millilitres", 2, 0.001),
    ("cl centiliter centiliters", 2, 0.01),
    ("l liter liters litre litres", 2, 1.0),
    ("tsp teaspoon teaspoons", 2, 0.00492892159375),
    ("tbsp tablespoon tablespoons", 2, 0.01478676478125),
    ("floz fl-oz fluidounce fluidounces", 2, 0.0295735295625),
    ("cup cups", 2, 0.2365882365),
    ("pt pint pints", 2, 0.473176473),
    ("qt quart quarts", 2, 0.946352946),
    ("gal gallon gallons", 2, 3.785411784),
    ("°c c celsius", U_TEMP, 0.0),
    ("°f f fahrenheit", U_TEMP, 1.0),
    ("k kelvin", U_TEMP, 2.0),
    ("km/h kmh kph", 4, 1.0 / 3.6),
    ("mph", 4, 0.44704),
    ("m/s mps", 4, 1.0),
    ("kn knot knots", 4, 0.514444),
    ("bit bits", 5, 0.125),
    ("b byte bytes", 5, 1.0),
    ("kb kilobyte kilobytes", 5, 1e3),
    ("mb megabyte megabytes", 5, 1e6),
    ("gb gigabyte gigabytes", 5, 1e9),
    ("tb terabyte terabytes", 5, 1e12),
    ("kib", 5, 1024.0),
    ("mib", 5, 1048576.0),
    ("gib", 5, 1073741824.0),
    ("tib", 5, 1099511627776.0),
    ("ms millisecond milliseconds", 6, 0.001),
    ("s sec secs second seconds", 6, 1.0),
    ("min mins minute minutes", 6, 60.0),
    ("h hr hrs hour hours", 6, 3600.0),
    ("d day days", 6, 86400.0),
    ("wk week weeks", 6, 604800.0),
    ("yr year years", 6, 31557600.0),
    ("m2 m² sqm", 7, 1.0),
    ("km2 km²", 7, 1e6),
    ("ft2 ft² sqft", 7, 0.09290304),
    ("acre acres", 7, 4046.8564224),
    ("ha hectare hectares", 7, 10000.0),
];
fn find_unit(w: &str) -> Option<usize> {
    UNITS.iter().position(|u| u.0.split(' ').any(|n| n == w))
}
fn unit_name(u: usize) -> &'static str {
    UNITS[u].0.split(' ').next().unwrap_or("")
}
fn to_base_temp(v: f64, u: i32) -> f64 {
    match u {
        0 => v,
        1 => (v - 32.0) * 5.0 / 9.0,
        _ => v - 273.15,
    }
}
fn from_base_temp(c: f64, u: i32) -> f64 {
    match u {
        0 => c,
        1 => c * 9.0 / 5.0 + 32.0,
        _ => c + 273.15,
    }
}

// ---- currency (rates from open.er-api.com, refreshed twice a day) -----------------------
struct Rates {
    map: HashMap<String, f64>, // per 1 USD
    at: f64,                   // unix time
}
static RATES: Mutex<Option<Rates>> = Mutex::new(None);
static RATES_LOADING: AtomicBool = AtomicBool::new(false);
const SYMBOLS: [(&str, &str); 12] = [
    ("$", "usd"),
    ("€", "eur"),
    ("£", "gbp"),
    ("¥", "jpy"),
    ("₹", "inr"),
    ("₩", "krw"),
    ("₽", "rub"),
    ("₺", "try"),
    ("₪", "ils"),
    ("₱", "php"),
    ("₫", "vnd"),
    ("฿", "thb"),
];
const CODES: &str = " usd eur gbp jpy cny cad aud nzd chf sek nok dkk pln czk huf ron bgn try rub uah inr pkr bdt lkr idr myr sgd thb vnd php \
krw hkd twd aed sar qar kwd ils egp zar ngn kes mad mxn brl ars clp cop pen uyu isk ";

fn with_rates<R>(f: impl FnOnce(&mut Rates) -> R) -> R {
    let mut g = RATES.lock().unwrap();
    f(g.get_or_insert_with(|| Rates { map: HashMap::new(), at: 0.0 }))
}
fn is_currency(w: &str) -> bool {
    if w.chars().count() != 3 {
        return false;
    }
    with_rates(|r| r.map.contains_key(w)) || CODES.contains(&format!(" {} ", w))
}
pub fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}
fn parse_rates(js: &str) {
    let Some(r) = js.find("\"rates\"") else { return };
    let mut m = HashMap::new();
    let mut p = js[r..].find('{').map(|x| x + r);
    while let Some(pp) = p {
        let Some(q1) = js[pp..].find('"').map(|x| x + pp) else { break };
        let Some(q2) = js[q1 + 1..].find('"').map(|x| x + q1 + 1) else { break };
        let Some(colon) = js[q2..].find(':').map(|x| x + q2) else { break };
        let code = &js[q1 + 1..q2];
        let rest = &js[colon + 1..];
        let end = rest.find([',', '}']).unwrap_or(rest.len());
        let v: f64 = rest[..end].trim().parse().unwrap_or(0.0);
        if code.len() == 3 && v > 0.0 {
            m.insert(code.to_lowercase(), v);
        }
        if end >= rest.len() || rest.as_bytes()[end] == b'}' {
            break;
        }
        p = Some(colon + 1 + end);
    }
    if m.len() > 10 {
        with_rates(|r| r.map = m);
    }
}
// Starts a refresh when they're missing or old.
fn load_rates() {
    if RATES_LOADING.load(Relaxed) {
        return;
    }
    let empty = with_rates(|r| r.map.is_empty());
    if empty {
        // the copy saved last time
        let cache = format!("{}\\rates.json", app_dir());
        if let Ok(md) = std::fs::metadata(&cache) {
            if let Ok(js) = std::fs::read_to_string(&cache) {
                parse_rates(&js);
                let at = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                with_rates(|r| r.at = at);
            }
        }
    }
    let (age, empty) = with_rates(|r| (unix_now() - r.at, r.map.is_empty()));
    if age < 12.0 * 3600.0 && !empty {
        return;
    }
    RATES_LOADING.store(true, Relaxed);
    std::thread::spawn(|| {
        let mut buf = Vec::new();
        if fonts::http_get("https://open.er-api.com/v6/latest/USD", Some(&mut buf), None, &mut |_| {}) {
            let js = String::from_utf8_lossy(&buf).into_owned();
            if js.contains("\"rates\"") {
                parse_rates(&js);
                with_rates(|r| r.at = unix_now());
                let _ = std::fs::write(format!("{}\\rates.json", app_dir()), js.as_bytes());
            }
        }
        RATES_LOADING.store(false, Relaxed);
        unsafe {
            let _ = PostMessageW(hwnd(), WM_APP_PANEL, WPARAM(1), LPARAM(0));
        }
    });
}

// "5 km in mi", "100f to c", "20 usd in eur", "$20 to eur". Returns the number alone (copied
// by Enter) and what the bar shows.
fn convert(text: &str) -> Option<(String, String)> {
    let t = text.trim().to_lowercase();
    // "5 in to cm": try every " in " / " to " until one splits into two known units
    for w in [" in ", " to ", " as ", " -> ", "->"] {
        let mut from = 0;
        while let Some(p) = t[from..].find(w).map(|x| x + from) {
            if p > 0 {
                if let Some(r) = convert_parts(t[..p].trim(), t[p + w.len()..].trim()) {
                    return Some(r);
                }
            }
            from = p + 1;
            while !t.is_char_boundary(from) {
                from += 1;
            }
        }
    }
    None
}
fn convert_parts(left: &str, to: &str) -> Option<(String, String)> {
    if left.is_empty() || to.is_empty() {
        return None;
    }
    let mut left = left.to_string();
    let mut to = to.to_string();
    for (sym, code) in SYMBOLS {
        // "$20" -> "20 usd"
        if let Some(rest) = left.strip_prefix(sym) {
            left = format!("{} {}", rest.trim(), code);
        }
        if to == sym {
            to = code.to_string();
        }
    }
    // split the left side into the amount and its unit: the unit is the trailing letters
    let lc: Vec<char> = left.chars().collect();
    let mut k = lc.len();
    while k > 0 && !lc[k - 1].is_ascii_digit() && lc[k - 1] != ')' && lc[k - 1] != '.' {
        k -= 1;
    }
    let amount_s: String = lc[..k].iter().collect::<String>().trim().to_string();
    let mut from: String = lc[k..].iter().collect::<String>().trim().to_string();
    if from.is_empty() {
        return None;
    }
    let mut amount = 1.0;
    if !amount_s.is_empty() {
        let mut p = Parser::new(&amount_s);
        amount = p.expr();
        p.ws();
        if !p.ok || p.i != p.s.len() {
            return None;
        }
    }
    if from.chars().count() > 1 && from.ends_with('.') {
        from.pop();
    }
    if let (Some(uf), Some(ut)) = (find_unit(&from), find_unit(&to)) {
        if UNITS[uf].1 == UNITS[ut].1 {
            let v = if UNITS[uf].1 == U_TEMP {
                from_base_temp(to_base_temp(amount, UNITS[uf].2 as i32), UNITS[ut].2 as i32)
            } else {
                amount * UNITS[uf].2 / UNITS[ut].2
            };
            let r = format_number(v, 6);
            let shown = format!("= {} {}", r, unit_name(ut));
            return Some((r, shown));
        }
    }
    if is_currency(&from) && is_currency(&to) {
        load_rates();
        let rates = with_rates(|r| (r.map.get(&from).copied(), r.map.get(&to).copied()));
        let (Some(a), Some(b)) = rates else {
            let msg = if RATES_LOADING.load(Relaxed) { "= …" } else { "No rate for that" };
            return Some((String::new(), msg.into()));
        };
        let v = amount / a * b;
        let r = if v.abs() >= 100.0 { format!("{:.2}", v) } else { g_fmt(v, 4) };
        let shown = format!("= {} {}", r, to.to_uppercase());
        return Some((r, shown));
    }
    None
}

// ---- web shortcuts ---------------------------------------------------------------------------
struct WebShortcut {
    key: String,
    name: String,
    url: String, // has %s where the search goes
}
static mut WEB: Vec<WebShortcut> = Vec::new();
static mut WEB_AT: f64 = -1e9;
fn web() -> &'static mut Vec<WebShortcut> {
    unsafe { &mut *std::ptr::addr_of_mut!(WEB) }
}
fn searches_file() -> String {
    format!("{}\\Searches.txt", app_dir())
}
fn load_web_shortcuts() {
    unsafe {
        if now() - WEB_AT < 5.0 {
            return;
        }
        WEB_AT = now();
    }
    let w = web();
    w.clear();
    for (k, n, u) in [
        ("g", "Google", "https://www.google.com/search?q=%s"),
        ("yt", "YouTube", "https://www.youtube.com/results?search_query=%s"),
        ("w", "Wikipedia", "https://en.wikipedia.org/w/index.php?search=%s"),
        ("gh", "GitHub", "https://github.com/search?q=%s"),
        ("r", "Reddit", "https://www.reddit.com/search/?q=%s"),
        ("a", "Amazon", "https://www.amazon.com/s?k=%s"),
        ("maps", "Maps", "https://www.google.com/maps/search/%s"),
        ("img", "Images", "https://www.google.com/search?tbm=isch&q=%s"),
        ("ddg", "DuckDuckGo", "https://duckduckgo.com/?q=%s"),
        ("b", "Bing", "https://www.bing.com/search?q=%s"),
        ("tr", "Translate", "https://translate.google.com/?sl=auto&text=%s"),
    ] {
        w.push(WebShortcut { key: k.into(), name: n.into(), url: u.into() });
    }
    // Searches.txt: "key  Name  https://...%s" per line (the name is optional)
    let t = games::read_text_file(&searches_file());
    for line in t.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (Some(sp), Some(u)) = (line.find([' ', '\t']), line.find("://")) else { continue };
        let us = line[..u].rfind([' ', '\t']).unwrap_or(sp);
        let key = line[..sp].to_lowercase();
        let url = line[us + 1..].trim().to_string();
        let mut name = if us > sp { line[sp..us].trim().to_string() } else { String::new() };
        if name.is_empty() {
            name = key.clone();
        }
        w.retain(|o| o.key != key);
        w.push(WebShortcut { key, name, url });
    }
}

// Called by update_state for text without ">". Returns the hint to show, or "".
pub fn inline_answer(text: &str, app_starts_with_text: bool) -> String {
    let p = ps();
    p.inline_copy.clear();
    p.inline_url.clear();
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Some((r, shown)) = convert(t) {
        p.inline_copy = r;
        return shown;
    }
    if let Some(v) = eval_math(t) {
        p.inline_copy = format_number(v, 12);
        return format!("= {}", p.inline_copy);
    }
    if app_starts_with_text {
        return String::new();
    }
    if let Some(sp) = t.find(' ') {
        load_web_shortcuts();
        let (k, q) = (t[..sp].to_lowercase(), t[sp + 1..].trim());
        for w in web().iter() {
            if w.key == k && !q.is_empty() {
                p.inline_url = w.url.replacen("%s", &url_encode(q), 1);
                return format!("{}: {}", w.name, q);
            }
        }
    }
    String::new()
}

pub fn inline_activate() -> bool {
    let p = ps();
    if !p.inline_url.is_empty() {
        let url = p.inline_url.clone();
        st().prev_fg = HWND::default();
        hide();
        unsafe { ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        return true;
    }
    if !p.inline_copy.is_empty() {
        let c = p.inline_copy.clone();
        if set_clipboard_text(&c) {
            flash(&format!("Copied {}", c));
        }
        return true;
    }
    false
}

// "> searches": the shortcuts, and the file to add your own.
pub fn searches_fill(_: bool) {
    load_web_shortcuts();
    let r = rows();
    r.clear();
    r.push(PRow {
        glyph: "\u{E70F}".into(),
        title: "Add your own…".into(),
        detail: "Searches.txt".into(),
        action: "Edit  ↵".into(),
        tag: -1,
        ..Default::default()
    });
    let a = ps().arg.to_lowercase();
    for (i, w) in web().iter().enumerate() {
        if !a.is_empty() && !w.key.contains(&a) && !w.name.to_lowercase().contains(&a) {
            continue;
        }
        r.push(PRow {
            glyph: "\u{E721}".into(),
            title: w.name.clone(),
            detail: format!("{} …", w.key),
            action: format!("Type “{} ”  ↵", w.key),
            tag: i as i32,
            ..Default::default()
        });
    }
}
pub fn searches_enter(sel: i32, _: bool) {
    let Some(row) = rows().get(sel.max(0) as usize) else { return };
    let tag = row.tag;
    if tag < 0 {
        let f = searches_file();
        if !games::file_exists(&f) {
            let seed = "# Your own web searches: a shortcut, a name, and the address with %s where the search goes.\r\n\
# Then type the shortcut and a search in the bar, e.g. \"so how to center a div\".\r\n\
so  Stack Overflow  https://stackoverflow.com/search?q=%s\r\n";
            let _ = std::fs::write(&f, seed);
        }
        st().prev_fg = HWND::default();
        hide();
        unsafe {
            ShellExecuteW(HWND::default(), w!("open"), w!("notepad.exe"), &HSTRING::from(format!("\"{}\"", f)), PCWSTR::null(), SW_SHOWNORMAL)
        };
        unsafe { WEB_AT = -1e9 };
        return;
    }
    if let Some(w) = web().get(tag as usize) {
        set_text(&ws(&format!("{} ", w.key)));
    }
}
