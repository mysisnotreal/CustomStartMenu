// CSMInstaller — setup & updater for Custom Start (CSM).
//
// * Translucent (acrylic) window, drawn with Direct2D.
// * First run: animated "hello" in the PC's language -> language -> code backend -> install.
// * Everything is pulled from GitHub: manifest.json on the main branch describes the latest
//   version and its changelog; binaries come from the matching GitHub release.
// * When a newer version exists, Setup opens straight to "Update to V<x>?" with a changelog
//   (added in green, removed in red).
//
// Command line:  --check-update   stay hidden unless an update is available
//                --uninstall      open the uninstall page
//
// The English "hello" lettering path comes from the CodePen "Apple hello" by steefmaster
// (https://codepen.io/steefmaster/pen/MWvdyGb), MIT licensed per CodePen's public pen terms.

#ifndef UNICODE
#define UNICODE
#endif
#ifndef _UNICODE
#define _UNICODE
#endif
#define NOMINMAX

#include <windows.h>
#include <windowsx.h>
#include <dwmapi.h>
#include <d2d1.h>
#include <dwrite_3.h>
#include <winhttp.h>
#include <shlobj.h>
#include <shobjidl.h>
#include <shellapi.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "shell32.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "advapi32.lib")
#pragma comment(lib, "uuid.lib")
#pragma comment(lib, "d2d1.lib")
#pragma comment(lib, "dwrite.lib")
#pragma comment(lib, "dwmapi.lib")
#pragma comment(lib, "winhttp.lib")
#pragma comment(lib, "version.lib")

using D2D1::ColorF;
using D2D1::Matrix3x2F;

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------
static const wchar_t* kManifestUrl =
    L"https://raw.githubusercontent.com/mysisnotreal/CustomStartMenu/main/manifest.json";
// The same file through the GitHub API: not behind raw.githubusercontent's few-minute cache, so a
// release is seen the moment it's published. The raw URL above is the fallback (API rate limits).
static const wchar_t* kManifestApiUrl =
    L"https://api.github.com/repos/mysisnotreal/CustomStartMenu/contents/manifest.json?ref=main";
static const wchar_t* kReleaseBase = L"https://github.com/mysisnotreal/CustomStartMenu/releases/download/v";
static const wchar_t* kSettingsKey = L"Software\\CustomStartMenu";
static const wchar_t* kRunKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\Run";
static const wchar_t* kUninstallKey = L"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CustomStartMenu";
static const wchar_t* kAppClass = L"CustomStartMenu.CommandBar";
#define WM_CSM_QUIT (WM_APP + 5)  // understood by both app backends

// ---------------------------------------------------------------------------
// Localization
// ---------------------------------------------------------------------------
enum Str {
    S_HELLO, S_CONTINUE, S_LANG_TITLE, S_LANG_SUB, S_BACKEND_TITLE, S_BACKEND_SUB, S_RUST_DESC, S_CPP_DESC,
    S_INSTALL, S_INSTALLING, S_DOWNLOADING, S_FINISHING, S_DONE_TITLE, S_DONE_SUB, S_FINISH, S_UPDATE_TITLE,
    S_UPDATE_SUB, S_UPDATE, S_NOT_NOW, S_UPTODATE, S_VERSION_LINE, S_MODIFY, S_UNINSTALL, S_CLOSE,
    S_UNINSTALL_TITLE, S_UNINSTALL_SUB, S_CANCEL, S_UNINSTALLED, S_OFFLINE_TITLE, S_RETRY, S_CHECKING, S_BACK,
    S_INSTALL_FAILED, S_RECOMMENDED, S_SETUP, S_PERF_TITLE, S_PERF_SUB, S_HWACCEL, S_HWACCEL_DESC,
    S_HWACCEL_NOTE, S_COUNT
};

struct Lang {
    const wchar_t* code;
    const wchar_t* native;
    const wchar_t* s[S_COUNT];
};

static const Lang kLangs[] = {
    {L"en", L"English", {
        L"hello", L"Continue", L"Choose your language", L"This is used for Setup and Custom Start.",
        L"Choose a code backend", L"Both look and work the same. You can switch later.",
        L"Memory-safe build written in Rust", L"The original native build in C++", L"Install",
        L"Installing Custom Start", L"Downloading from GitHub…", L"Finishing up…", L"You're all set",
        L"Press the Windows key to open Custom Start.", L"Finish", L"Update to V%s?",
        L"You have V%s. Here's what changed:", L"Update", L"Not now", L"Custom Start is up to date",
        L"Version %s · %s backend", L"Modify", L"Uninstall", L"Close", L"Uninstall Custom Start?",
        L"Your normal Start menu will come back.", L"Cancel", L"Custom Start was removed",
        L"Couldn't reach GitHub", L"Try again", L"Checking for updates…", L"Back",
        L"Installation failed", L"Recommended", L"Custom Start Setup",
        L"Performance", L"Keep Custom Start light while you play.", L"Hardware acceleration", L"Draws on your graphics card, so Custom Start stays smooth and never slows down your games.", L"Only turn this off if Custom Start looks wrong on your PC."}},
    {L"es", L"Español", {
        L"hola", L"Continuar", L"Elige tu idioma", L"Se usa en la instalación y en Custom Start.",
        L"Elige el backend de código", L"Ambos se ven y funcionan igual. Puedes cambiarlo después.",
        L"Versión con seguridad de memoria escrita en Rust", L"La versión nativa original en C++",
        L"Instalar", L"Instalando Custom Start", L"Descargando desde GitHub…", L"Terminando…",
        L"Todo listo", L"Pulsa la tecla Windows para abrir Custom Start.", L"Finalizar",
        L"¿Actualizar a V%s?", L"Tienes V%s. Esto es lo que cambió:", L"Actualizar", L"Ahora no",
        L"Custom Start está actualizado", L"Versión %s · backend %s", L"Modificar",
        L"Desinstalar", L"Cerrar", L"¿Desinstalar Custom Start?", L"Volverá tu menú Inicio normal.",
        L"Cancelar", L"Custom Start se ha eliminado", L"No se pudo conectar con GitHub", L"Reintentar",
        L"Buscando actualizaciones…", L"Atrás", L"La instalación falló", L"Recomendado",
        L"Instalación de Custom Start",
        L"Rendimiento", L"Mantén Custom Start ligero mientras juegas.", L"Aceleración por hardware", L"Dibuja con tu tarjeta gráfica, así Custom Start va fluido y nunca ralentiza tus juegos.", L"Desactívala solo si Custom Start se ve mal en tu PC."}},
    {L"fr", L"Français", {
        L"bonjour", L"Continuer", L"Choisissez votre langue", L"Utilisée pour l'installation et Custom Start.",
        L"Choisissez le moteur de code", L"Les deux sont identiques. Vous pourrez changer plus tard.",
        L"Version sûre en mémoire écrite en Rust", L"La version native d'origine en C++",
        L"Installer", L"Installation de Custom Start", L"Téléchargement depuis GitHub…",
        L"Finalisation…", L"Tout est prêt", L"Appuyez sur la touche Windows pour ouvrir Custom Start.",
        L"Terminer", L"Mettre à jour vers V%s ?", L"Vous avez V%s. Voici les changements :",
        L"Mettre à jour", L"Plus tard", L"Custom Start est à jour", L"Version %s · moteur %s",
        L"Modifier", L"Désinstaller", L"Fermer", L"Désinstaller Custom Start ?",
        L"Votre menu Démarrer habituel reviendra.", L"Annuler", L"Custom Start a été supprimé",
        L"Impossible de joindre GitHub", L"Réessayer", L"Recherche de mises à jour…", L"Retour",
        L"L'installation a échoué", L"Recommandé", L"Installation de Custom Start",
        L"Performances", L"Gardez Custom Start léger pendant que vous jouez.", L"Accélération matérielle", L"Utilise votre carte graphique : Custom Start reste fluide et ne ralentit jamais vos jeux.", L"Ne la désactivez que si Custom Start s'affiche mal sur votre PC."}},
    {L"de", L"Deutsch", {
        L"hallo", L"Weiter", L"Wählen Sie Ihre Sprache", L"Wird für Setup und Custom Start verwendet.",
        L"Wählen Sie ein Code-Backend", L"Beide sehen gleich aus und funktionieren gleich. Später änderbar.",
        L"Speichersichere Version in Rust", L"Die ursprüngliche native Version in C++", L"Installieren",
        L"Custom Start wird installiert", L"Wird von GitHub heruntergeladen…", L"Wird abgeschlossen…",
        L"Alles bereit", L"Drücken Sie die Windows-Taste, um Custom Start zu öffnen.", L"Fertig",
        L"Auf V%s aktualisieren?", L"Sie haben V%s. Das hat sich geändert:", L"Aktualisieren",
        L"Nicht jetzt", L"Custom Start ist aktuell", L"Version %s · %s-Backend", L"Ändern",
        L"Deinstallieren", L"Schließen", L"Custom Start deinstallieren?",
        L"Ihr normales Startmenü kehrt zurück.", L"Abbrechen", L"Custom Start wurde entfernt",
        L"GitHub nicht erreichbar", L"Erneut versuchen", L"Suche nach Updates…", L"Zurück",
        L"Installation fehlgeschlagen", L"Empfohlen", L"Custom Start-Setup",
        L"Leistung", L"Custom Start bleibt leicht, während du spielst.", L"Hardwarebeschleunigung", L"Zeichnet über deine Grafikkarte, damit Custom Start flüssig bleibt und deine Spiele nie ausbremst.", L"Schalte sie nur aus, wenn Custom Start auf deinem PC falsch aussieht."}},
    {L"it", L"Italiano", {
        L"ciao", L"Continua", L"Scegli la lingua", L"Usata per l'installazione e per Custom Start.",
        L"Scegli il backend del codice", L"Entrambi sono identici. Puoi cambiare in seguito.",
        L"Versione memory-safe scritta in Rust", L"La versione nativa originale in C++", L"Installa",
        L"Installazione di Custom Start", L"Download da GitHub…", L"Completamento…", L"Tutto pronto",
        L"Premi il tasto Windows per aprire Custom Start.", L"Fine", L"Aggiornare a V%s?",
        L"Hai V%s. Ecco cosa è cambiato:", L"Aggiorna", L"Non ora", L"Custom Start è aggiornato",
        L"Versione %s · backend %s", L"Modifica", L"Disinstalla", L"Chiudi", L"Disinstallare Custom Start?",
        L"Tornerà il normale menu Start.", L"Annulla", L"Custom Start è stato rimosso",
        L"Impossibile raggiungere GitHub", L"Riprova", L"Ricerca aggiornamenti…", L"Indietro",
        L"Installazione non riuscita", L"Consigliato", L"Installazione di Custom Start",
        L"Prestazioni", L"Mantieni Custom Start leggero mentre giochi.", L"Accelerazione hardware", L"Disegna con la scheda grafica, così Custom Start resta fluido e non rallenta mai i tuoi giochi.", L"Disattivala solo se Custom Start non si vede bene sul tuo PC."}},
    {L"pt", L"Português", {
        L"olá", L"Continuar", L"Escolha seu idioma", L"Usado na instalação e no Custom Start.",
        L"Escolha o backend de código", L"Ambos são iguais. Você pode trocar depois.",
        L"Versão com segurança de memória em Rust", L"A versão nativa original em C++",
        L"Instalar", L"Instalando o Custom Start", L"Baixando do GitHub…", L"Finalizando…",
        L"Tudo pronto", L"Pressione a tecla Windows para abrir o Custom Start.", L"Concluir",
        L"Atualizar para V%s?", L"Você tem V%s. Veja o que mudou:", L"Atualizar", L"Agora não",
        L"O Custom Start está atualizado", L"Versão %s · backend %s", L"Modificar", L"Desinstalar",
        L"Fechar", L"Desinstalar o Custom Start?", L"Seu menu Iniciar normal voltará.", L"Cancelar",
        L"O Custom Start foi removido", L"Não foi possível acessar o GitHub", L"Tentar novamente",
        L"Procurando atualizações…", L"Voltar", L"Falha na instalação", L"Recomendado",
        L"Instalação do Custom Start",
        L"Desempenho", L"Mantenha o Custom Start leve enquanto você joga.", L"Aceleração de hardware", L"Desenha com a placa de vídeo, para o Custom Start ficar fluido e nunca deixar seus jogos lentos.", L"Desative apenas se o Custom Start aparecer errado no seu PC."}},
    {L"ru", L"Русский", {
        L"привет", L"Продолжить",
        L"Выберите язык",
        L"Используется в установщике и в Custom Start.",
        L"Выберите движок",
        L"Оба работают одинаково. Можно сменить позже.",
        L"Безопасная версия на Rust",
        L"Исходная нативная версия на C++",
        L"Установить", L"Установка Custom Start",
        L"Загрузка с GitHub…", L"Завершение…",
        L"Всё готово",
        L"Нажмите клавишу Windows, чтобы открыть Custom Start.",
        L"Готово", L"Обновить до V%s?",
        L"У вас V%s. Что изменилось:",
        L"Обновить", L"Не сейчас",
        L"Custom Start обновлён", L"Версия %s · движок %s",
        L"Изменить", L"Удалить", L"Закрыть",
        L"Удалить Custom Start?",
        L"Вернётся обычное меню «Пуск».",
        L"Отмена", L"Custom Start удалён",
        L"Не удалось подключиться к GitHub",
        L"Повторить", L"Проверка обновлений…",
        L"Назад", L"Ошибка установки",
        L"Рекомендуется", L"Установка Custom Start",
        L"Производительность", L"Custom Start не будет мешать во время игры.", L"Аппаратное ускорение", L"Отрисовка на видеокарте: Custom Start работает плавно и не замедляет игры.", L"Отключайте, только если Custom Start отображается неправильно."}},
    {L"ja", L"日本語", {
        L"こんにちは", L"続行", L"言語を選択",
        L"セットアップと Custom Start で使用されます。",
        L"コードのバックエンドを選択",
        L"どちらも見た目と動作は同じです。後で変更できます。",
        L"Rust で書かれたメモリ安全なビルド",
        L"C++ で書かれたオリジナルのネイティブビルド",
        L"インストール", L"Custom Start をインストールしています",
        L"GitHub からダウンロードしています…",
        L"仕上げています…", L"準備完了",
        L"Windows キーを押すと Custom Start が開きます。", L"完了",
        L"V%s に更新しますか?", L"現在は V%s です。変更点:",
        L"更新", L"後で", L"Custom Start は最新です",
        L"バージョン %s · %s バックエンド", L"変更",
        L"アンインストール", L"閉じる",
        L"Custom Start をアンインストールしますか?",
        L"通常のスタート メニューに戻ります。",
        L"キャンセル", L"Custom Start を削除しました",
        L"GitHub に接続できません", L"再試行",
        L"更新を確認しています…", L"戻る",
        L"インストールに失敗しました", L"おすすめ",
        L"Custom Start セットアップ",
        L"パフォーマンス", L"ゲーム中も Custom Start を軽く保ちます。", L"ハードウェア アクセラレーション", L"グラフィックス カードで描画するので、Custom Start は滑らかに動き、ゲームを遅くしません。", L"Custom Start の表示がおかしい場合のみオフにしてください。"}},
    {L"zh", L"中文", {
        L"你好", L"继续", L"选择你的语言", L"用于安装程序和 Custom Start。",
        L"选择代码后端", L"两者外观和功能相同，之后可以更改。",
        L"用 Rust 编写的内存安全版本", L"用 C++ 编写的原生版本",
        L"安装", L"正在安装 Custom Start", L"正在从 GitHub 下载…",
        L"即将完成…", L"一切就绪", L"按 Windows 键打开 Custom Start。",
        L"完成", L"更新到 V%s？", L"你的版本是 V%s。更新内容：",
        L"更新", L"以后再说", L"Custom Start 已是最新版本",
        L"版本 %s · %s 后端", L"修改", L"卸载", L"关闭",
        L"卸载 Custom Start？", L"将恢复正常的开始菜单。", L"取消",
        L"Custom Start 已移除", L"无法连接到 GitHub", L"重试",
        L"正在检查更新…", L"返回", L"安装失败", L"推荐",
        L"Custom Start 安装程序",
        L"性能", L"玩游戏时让 Custom Start 保持轻量。", L"硬件加速", L"使用显卡绘制，Custom Start 保持流畅，不会拖慢你的游戏。", L"仅在 Custom Start 显示异常时关闭。"}},
    {L"ko", L"한국어", {
        L"안녕하세요", L"계속", L"언어 선택",
        L"설치 프로그램과 Custom Start에 사용됩니다.",
        L"코드 백엔드 선택",
        L"두 가지 모두 동일하게 작동합니다. 나중에 바꿀 수 있습니다.",
        L"Rust로 작성된 메모리 안전 빌드",
        L"C++로 작성된 오리지널 네이티브 빌드", L"설치",
        L"Custom Start 설치 중", L"GitHub에서 다운로드 중…",
        L"마무리하는 중…", L"모든 준비 완료",
        L"Windows 키를 눌러 Custom Start를 여세요.", L"마침",
        L"V%s(으)로 업데이트할까요?", L"현재 V%s입니다. 변경 사항:",
        L"업데이트", L"나중에", L"Custom Start가 최신 상태입니다",
        L"버전 %s · %s 백엔드", L"수정", L"제거", L"닫기",
        L"Custom Start를 제거할까요?", L"기본 시작 메뉴로 돌아갑니다.",
        L"취소", L"Custom Start가 제거되었습니다",
        L"GitHub에 연결할 수 없습니다", L"다시 시도",
        L"업데이트 확인 중…", L"뒤로", L"설치 실패", L"권장",
        L"Custom Start 설치",
        L"성능", L"게임 중에도 Custom Start를 가볍게 유지합니다.", L"하드웨어 가속", L"그래픽 카드로 그려서 Custom Start가 부드럽게 동작하고 게임을 느리게 하지 않습니다.", L"Custom Start가 PC에서 이상하게 보일 때만 끄세요."}},
};
constexpr int kLangCount = (int)(sizeof(kLangs) / sizeof(kLangs[0]));

static int g_lang = 0;
static const wchar_t* T(Str s) { return kLangs[g_lang].s[s]; }
static std::wstring Fmt(Str s, const std::wstring& a, const std::wstring& b = L"") {
    wchar_t buf[512];
    swprintf_s(buf, T(s), a.c_str(), b.c_str());
    return buf;
}

static int LangIndex(const std::wstring& code) {
    for (int i = 0; i < kLangCount; i++)
        if (code.compare(0, 2, kLangs[i].code) == 0) return i;
    return -1;
}

static int SystemLanguage() {
    wchar_t name[LOCALE_NAME_MAX_LENGTH] = {};
    if (LCIDToLocaleName(MAKELCID(GetUserDefaultUILanguage(), SORT_DEFAULT), name, LOCALE_NAME_MAX_LENGTH, 0)) {
        int i = LangIndex(name);
        if (i >= 0) return i;
    }
    return 0;
}

// ---------------------------------------------------------------------------
// Tiny JSON reader (enough for manifest.json)
// ---------------------------------------------------------------------------
struct JVal {
    enum Type { Null, Bool, Num, Str, Arr, Obj } t = Null;
    bool b = false;
    double n = 0;
    std::wstring s;
    std::vector<JVal> a;
    std::vector<std::pair<std::wstring, JVal>> o;
    const JVal* get(const wchar_t* k) const {
        for (auto& kv : o)
            if (kv.first == k) return &kv.second;
        return nullptr;
    }
    std::wstring str(const wchar_t* k) const {
        const JVal* v = get(k);
        return v && v->t == Str ? v->s : L"";
    }
};

struct JParser {
    const std::wstring& src;
    size_t i = 0;
    explicit JParser(const std::wstring& s) : src(s) {}
    void ws() { while (i < src.size() && iswspace(src[i])) i++; }
    bool parse(JVal& v) {
        ws();
        if (i >= src.size()) return false;
        wchar_t c = src[i];
        if (c == L'{') {
            v.t = JVal::Obj;
            i++;
            ws();
            if (i < src.size() && src[i] == L'}') { i++; return true; }
            for (;;) {
                ws();
                JVal key;
                if (!parse(key) || key.t != JVal::Str) return false;
                ws();
                if (i >= src.size() || src[i] != L':') return false;
                i++;
                JVal val;
                if (!parse(val)) return false;
                v.o.push_back({key.s, std::move(val)});
                ws();
                if (i < src.size() && src[i] == L',') { i++; continue; }
                if (i < src.size() && src[i] == L'}') { i++; return true; }
                return false;
            }
        }
        if (c == L'[') {
            v.t = JVal::Arr;
            i++;
            ws();
            if (i < src.size() && src[i] == L']') { i++; return true; }
            for (;;) {
                JVal val;
                if (!parse(val)) return false;
                v.a.push_back(std::move(val));
                ws();
                if (i < src.size() && src[i] == L',') { i++; continue; }
                if (i < src.size() && src[i] == L']') { i++; return true; }
                return false;
            }
        }
        if (c == L'"') {
            v.t = JVal::Str;
            i++;
            while (i < src.size() && src[i] != L'"') {
                wchar_t ch = src[i++];
                if (ch == L'\\' && i < src.size()) {
                    wchar_t e = src[i++];
                    switch (e) {
                    case L'n': v.s += L'\n'; break;
                    case L't': v.s += L'\t'; break;
                    case L'r': break;
                    case L'b': case L'f': break;
                    case L'u':
                        if (i + 4 <= src.size()) { v.s += (wchar_t)wcstol(src.substr(i, 4).c_str(), nullptr, 16); i += 4; }
                        break;
                    default: v.s += e;
                    }
                } else {
                    v.s += ch;
                }
            }
            i++;
            return true;
        }
        if (src.compare(i, 4, L"true") == 0) { v.t = JVal::Bool; v.b = true; i += 4; return true; }
        if (src.compare(i, 5, L"false") == 0) { v.t = JVal::Bool; i += 5; return true; }
        if (src.compare(i, 4, L"null") == 0) { i += 4; return true; }
        wchar_t* end = nullptr;
        v.t = JVal::Num;
        v.n = wcstod(src.c_str() + i, &end);
        size_t used = end - (src.c_str() + i);
        if (!used) return false;
        i += used;
        return true;
    }
};

static std::wstring Utf8ToWide(const std::string& s) {
    int n = MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), nullptr, 0);
    std::wstring w(n, 0);
    MultiByteToWideChar(CP_UTF8, 0, s.data(), (int)s.size(), &w[0], n);
    return w;
}

static std::vector<int> ParseVersion(const std::wstring& v) {
    std::vector<int> r;
    size_t i = 0;
    if (!v.empty() && (v[0] == L'v' || v[0] == L'V')) i = 1;
    while (i < v.size()) {
        r.push_back(_wtoi(v.c_str() + i));
        size_t d = v.find(L'.', i);
        if (d == std::wstring::npos) break;
        i = d + 1;
    }
    while (r.size() < 3) r.push_back(0);
    return r;
}
static int CompareVersions(const std::wstring& a, const std::wstring& b) {
    auto x = ParseVersion(a), y = ParseVersion(b);
    for (size_t i = 0; i < std::max(x.size(), y.size()); i++) {
        int p = i < x.size() ? x[i] : 0, q = i < y.size() ? y[i] : 0;
        if (p != q) return p < q ? -1 : 1;
    }
    return 0;
}

// ---------------------------------------------------------------------------
// HTTP (WinHTTP, follows GitHub's redirects to the release CDN)
// ---------------------------------------------------------------------------
static bool HttpGet(const std::wstring& url, std::string& out, const std::function<void(double)>& progress,
                    std::wstring& err, const wchar_t* headers = nullptr) {
    URL_COMPONENTS uc = {sizeof(uc)};
    wchar_t host[256] = {}, path[4096] = {}, extra[1024] = {};
    uc.lpszHostName = host;
    uc.dwHostNameLength = 256;
    uc.lpszUrlPath = path;
    uc.dwUrlPathLength = 4096;
    uc.lpszExtraInfo = extra;
    uc.dwExtraInfoLength = 1024;
    if (!WinHttpCrackUrl(url.c_str(), 0, 0, &uc)) { err = L"Bad URL"; return false; }
    std::wstring object = std::wstring(path) + extra;

    HINTERNET s = WinHttpOpen(L"CSMInstaller/1.0", WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_NO_PROXY_NAME,
                              WINHTTP_NO_PROXY_BYPASS, 0);
    if (!s) s = WinHttpOpen(L"CSMInstaller/1.0", WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_NO_PROXY_NAME,
                            WINHTTP_NO_PROXY_BYPASS, 0);
    if (!s) { err = L"WinHTTP unavailable"; return false; }
    WinHttpSetTimeouts(s, 10000, 15000, 15000, 30000);
    HINTERNET c = WinHttpConnect(s, host, uc.nPort, 0);
    HINTERNET r = c ? WinHttpOpenRequest(c, L"GET", object.c_str(), nullptr, WINHTTP_NO_REFERER,
                                         WINHTTP_DEFAULT_ACCEPT_TYPES,
                                         uc.nScheme == INTERNET_SCHEME_HTTPS ? WINHTTP_FLAG_SECURE : 0)
                    : nullptr;
    bool ok = false;
    if (r && WinHttpSendRequest(r, headers ? headers : WINHTTP_NO_ADDITIONAL_HEADERS, headers ? (DWORD)-1L : 0,
                                WINHTTP_NO_REQUEST_DATA, 0, 0, 0) &&
        WinHttpReceiveResponse(r, nullptr)) {
        DWORD status = 0, sz = sizeof(status);
        WinHttpQueryHeaders(r, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX,
                            &status, &sz, WINHTTP_NO_HEADER_INDEX);
        DWORD64 total = 0;
        wchar_t lenBuf[64] = {};
        DWORD lenSz = sizeof(lenBuf);
        if (WinHttpQueryHeaders(r, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_HEADER_NAME_BY_INDEX, lenBuf, &lenSz,
                                WINHTTP_NO_HEADER_INDEX))
            total = _wcstoui64(lenBuf, nullptr, 10);
        if (status != 200) {
            err = L"HTTP " + std::to_wstring(status);
        } else {
            out.clear();
            if (total) out.reserve((size_t)total);
            for (;;) {
                DWORD avail = 0;
                if (!WinHttpQueryDataAvailable(r, &avail)) { err = L"Connection lost"; break; }
                if (!avail) { ok = true; break; }
                size_t old = out.size();
                out.resize(old + avail);
                DWORD got = 0;
                if (!WinHttpReadData(r, &out[old], avail, &got)) { err = L"Connection lost"; break; }
                out.resize(old + got);
                if (progress && total) progress(std::min(1.0, double(out.size()) / double(total)));
            }
        }
    } else if (err.empty()) {
        err = L"Network error " + std::to_wstring(GetLastError());
    }
    if (r) WinHttpCloseHandle(r);
    if (c) WinHttpCloseHandle(c);
    WinHttpCloseHandle(s);
    return ok;
}

// ---------------------------------------------------------------------------
// Manifest & install state
// ---------------------------------------------------------------------------
struct Change {
    std::wstring version;
    std::vector<std::wstring> added, removed;
};
struct Manifest {
    std::wstring version;
    std::unordered_map<std::wstring, std::wstring> assets;
    std::vector<Change> releases;
};

static std::mutex g_mx;
static Manifest g_manifest;
static bool g_manifestOk = false;
static std::wstring g_manifestErr;

static std::wstring g_installedVersion, g_installedBackend;
static std::wstring g_backend = L"cpp";
static bool g_hwAccel = true;  // HardwareAcceleration: on unless the user turned it off
static float g_hwAnim = 1.f;   // toggle switch position

static std::wstring RegStr(HKEY root, const wchar_t* key, const wchar_t* name) {
    wchar_t buf[512];
    DWORD sz = sizeof(buf);
    if (RegGetValueW(root, key, name, RRF_RT_REG_SZ, nullptr, buf, &sz) == ERROR_SUCCESS) return buf;
    return L"";
}
static void RegSetStr(HKEY root, const wchar_t* key, const wchar_t* name, const std::wstring& v) {
    HKEY k;
    if (RegCreateKeyExW(root, key, 0, nullptr, 0, KEY_SET_VALUE, nullptr, &k, nullptr) == ERROR_SUCCESS) {
        RegSetValueExW(k, name, 0, REG_SZ, (const BYTE*)v.c_str(), DWORD((v.size() + 1) * sizeof(wchar_t)));
        RegCloseKey(k);
    }
}
static void RegSetDword(HKEY root, const wchar_t* key, const wchar_t* name, DWORD v) {
    HKEY k;
    if (RegCreateKeyExW(root, key, 0, nullptr, 0, KEY_SET_VALUE, nullptr, &k, nullptr) == ERROR_SUCCESS) {
        RegSetValueExW(k, name, 0, REG_DWORD, (const BYTE*)&v, sizeof(v));
        RegCloseKey(k);
    }
}

static bool ParseManifest(const std::string& body, Manifest& m) {
    std::wstring w = Utf8ToWide(body);
    if (!w.empty() && w[0] == 0xFEFF) w.erase(0, 1);
    JVal root;
    JParser p(w);
    if (!p.parse(root) || root.t != JVal::Obj) return false;
    m.version = root.str(L"version");
    if (const JVal* a = root.get(L"assets"))
        for (auto& kv : a->o)
            if (kv.second.t == JVal::Str) m.assets[kv.first] = kv.second.s;
    if (const JVal* rel = root.get(L"releases"))
        for (const JVal& r : rel->a) {
            Change c;
            c.version = r.str(L"version");
            if (const JVal* x = r.get(L"added"))
                for (const JVal& s : x->a) c.added.push_back(s.s);
            if (const JVal* x = r.get(L"removed"))
                for (const JVal& s : x->a) c.removed.push_back(s.s);
            m.releases.push_back(std::move(c));
        }
    return !m.version.empty();
}

static std::wstring KnownFolder(REFKNOWNFOLDERID id) {
    PWSTR p = nullptr;
    std::wstring r;
    if (SUCCEEDED(SHGetKnownFolderPath(id, KF_FLAG_CREATE, nullptr, &p))) r = p;
    CoTaskMemFree(p);
    return r;
}
static std::wstring InstallDir() { return KnownFolder(FOLDERID_UserProgramFiles) + L"\\CustomStartMenu"; }
static std::wstring AppExe() { return InstallDir() + L"\\CustomStartMenu.exe"; }
static std::wstring InstallerExe() { return InstallDir() + L"\\CSMInstaller.exe"; }
static std::wstring SelfPath() {
    wchar_t p[MAX_PATH];
    GetModuleFileNameW(nullptr, p, MAX_PATH);
    return p;
}

// ---------------------------------------------------------------------------
// Graphics
// ---------------------------------------------------------------------------
namespace Ui {
constexpr float W = 840, H = 580, TitleH = 44;
}

static HWND g_hwnd;
static ID2D1Factory* g_d2d;
static IDWriteFactory* g_dw;
static ID2D1HwndRenderTarget* g_rt;
static ID2D1SolidColorBrush* g_solid;
static IDWriteTextFormat *g_fTitle, *g_fSub, *g_fBody, *g_fBtn, *g_fSmall, *g_fGlyph, *g_fBig, *g_fMono, *g_fIcon, *g_fCardTitle;
static float g_dpi = 96;
static bool g_acrylic = false;
static LARGE_INTEGER g_qpf;
static double g_lastFrame = 0;
static POINT g_mousePx = {-1, -1};

template <class T>
static void SafeRelease(T*& p) {
    if (p) { p->Release(); p = nullptr; }
}
static double Now() {
    LARGE_INTEGER t;
    QueryPerformanceCounter(&t);
    return double(t.QuadPart) / double(g_qpf.QuadPart);
}
static float Clamp01(double v) { return (float)std::clamp(v, 0.0, 1.0); }
static double EaseOutCubic(double t) { return 1 - std::pow(1 - t, 3); }
static double EaseInOutSine(double t) { return -(std::cos(3.14159265 * t) - 1) / 2; }
static float Damp(float c, float t, float k, float dt) { return c + (t - c) * (1 - std::exp(-k * dt)); }
static D2D1_COLOR_F Col(UINT32 rgb, float a = 1) { return ColorF(rgb, a); }

// The font chosen in Custom Start ("<install folder>\Fonts") is used for all text, like in the app.
static IDWriteFontCollection* g_fontColl;
static std::wstring g_fontFamily;
static void LoadCustomFont() {
    std::wstring dir = InstallDir() + L"\\Fonts";
    WIN32_FIND_DATAW fd;
    HANDLE h = FindFirstFileW((dir + L"\\*").c_str(), &fd);
    if (h == INVALID_HANDLE_VALUE) return;
    std::vector<std::wstring> files;
    do {
        std::wstring n = fd.cFileName;
        for (auto& c : n) c = (wchar_t)towlower(c);
        if (n.size() > 4 && (n.compare(n.size() - 4, 4, L".otf") == 0 || n.compare(n.size() - 4, 4, L".ttf") == 0))
            files.push_back(dir + L"\\" + fd.cFileName);
    } while (FindNextFileW(h, &fd));
    FindClose(h);
    if (files.empty()) return;
    std::sort(files.begin(), files.end());
    // the font picked with "> font" in Custom Start ("" = the Windows font); before that
    // existed, the first font in the folder
    wchar_t chosen[512] = {};
    DWORD csz = sizeof(chosen);
    if (RegGetValueW(HKEY_CURRENT_USER, kSettingsKey, L"Font", RRF_RT_REG_SZ, nullptr, chosen, &csz) == ERROR_SUCCESS) {
        std::wstring want = dir + L"\\" + chosen;
        if (!chosen[0] || GetFileAttributesW(want.c_str()) == INVALID_FILE_ATTRIBUTES) return;
        files.assign(1, want);
    } else files.resize(1);
    IDWriteFactory5* f5 = nullptr;
    IDWriteFontSetBuilder1* b = nullptr;
    IDWriteFontSet* set = nullptr;
    IDWriteFontCollection1* coll = nullptr;
    if (SUCCEEDED(g_dw->QueryInterface(IID_PPV_ARGS(&f5))) && SUCCEEDED(f5->CreateFontSetBuilder(&b))) {
        for (auto& p : files) {
            IDWriteFontFile* file = nullptr;
            if (SUCCEEDED(f5->CreateFontFileReference(p.c_str(), nullptr, &file))) {
                b->AddFontFile(file);
                file->Release();
            }
        }
        if (SUCCEEDED(b->CreateFontSet(&set)) && SUCCEEDED(f5->CreateFontCollectionFromFontSet(set, &coll)) &&
            coll->GetFontFamilyCount() > 0) {
            IDWriteFontFamily* fam = nullptr;
            IDWriteLocalizedStrings* names = nullptr;
            if (SUCCEEDED(coll->GetFontFamily(0, &fam)) && SUCCEEDED(fam->GetFamilyNames(&names))) {
                UINT32 idx = 0, len = 0;
                BOOL exists = FALSE;
                names->FindLocaleName(L"en-us", &idx, &exists);
                if (!exists) idx = 0;
                names->GetStringLength(idx, &len);
                std::wstring n(len + 1, L'\0');
                names->GetString(idx, &n[0], len + 1);
                n.resize(len);
                if (!n.empty()) {
                    g_fontFamily = n;
                    g_fontColl = coll;
                    coll = nullptr;
                }
            }
            if (names) names->Release();
            if (fam) fam->Release();
        }
    }
    if (coll) coll->Release();
    if (set) set->Release();
    if (b) b->Release();
    if (f5) f5->Release();
}

static IDWriteTextFormat* MakeFormat(float size, DWRITE_FONT_WEIGHT w, const wchar_t* fam = nullptr) {
    static const wchar_t* ui = nullptr;
    if (!fam && g_fontColl) {
        IDWriteTextFormat* f = nullptr;
        g_dw->CreateTextFormat(g_fontFamily.c_str(), g_fontColl, w, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
                               size, L"", &f);
        if (f) {
            f->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            return f;
        }
    }
    if (!ui) {
        IDWriteFontCollection* c = nullptr;
        BOOL ex = FALSE;
        UINT32 idx;
        g_dw->GetSystemFontCollection(&c);
        if (c) { c->FindFamilyName(L"Segoe UI Variable Display", &idx, &ex); c->Release(); }
        ui = ex ? L"Segoe UI Variable Display" : L"Segoe UI";
    }
    IDWriteTextFormat* f = nullptr;
    g_dw->CreateTextFormat(fam ? fam : ui, nullptr, w, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size,
                           L"", &f);
    f->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    return f;
}

static void Text(const std::wstring& s, IDWriteTextFormat* f, D2D1_RECT_F r, D2D1_COLOR_F c,
                 DWRITE_TEXT_ALIGNMENT al = DWRITE_TEXT_ALIGNMENT_CENTER, bool wrap = false) {
    IDWriteTextLayout* l = nullptr;
    if (FAILED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), f, r.right - r.left, r.bottom - r.top, &l))) return;
    l->SetTextAlignment(al);
    l->SetWordWrapping(wrap ? DWRITE_WORD_WRAPPING_WRAP : DWRITE_WORD_WRAPPING_NO_WRAP);
    g_solid->SetColor(c);
    g_rt->DrawTextLayout({r.left, r.top}, l, g_solid);
    l->Release();
}

// ---------------------------------------------------------------------------
// SVG path -> D2D geometry (M/L/H/V/C/S/Q/T/A/Z, absolute + relative)
// ---------------------------------------------------------------------------
static const char* kHelloPath =
    "M-293.58-104.62S-103.61-205.49-60-366.25c9.13-32.45,9-58.31,0-74-10.72-18.82-49.69-33.21-75.55,31.94-27.82,70.11-"
    "52.22,377.24-44.11,322.48s34-176.24,99.89-183.19c37.66-4,49.55,23.58,52.83,47.92a117.06,117.06,0,0,1-3,45.32c-7."
    "17,27.28-20.47,97.67,33.51,96.86,66.93-1,131.91-53.89,159.55-84.49,31.1-36.17,31.1-70.64,19.27-90.25-16.74-29.92-"
    "69.47-33-92.79,16.73C62.78-179.86,98.7-93.8,159-81.63S302.7-99.55,393.3-269.92c29.86-58.16,52.85-114.71,46.14-15"
    "0.08-7.44-39.21-59.74-54.5-92.87-8.7-47,65-61.78,266.62-34.74,308.53S416.62-58,481.52-130.31s133.2-188.56,146.54"
    "-256.23c14-71.15-56.94-94.64-88.4-47.32C500.53-375,467.58-229.49,503.3-127a73.73,73.73,0,0,0,23.43,33.67c25.49,2"
    "0.23,55.1,16,77.46,6.32a111.25,111.25,0,0,0,30.44-19.87c37.73-34.23,29-36.71,64.58-127.53C724-284.3,785-298.63,8"
    "21-259.13a71,71,0,0,1,13.69,22.56c17.68,46,6.81,80-6.81,107.89-12,24.62-34.56,42.72-61.45,47.91-23.06,4.45-48.37"
    "-.35-66.48-24.27a78.88,78.88,0,0,1-12.66-25.8c-14.75-51,4.14-88.76,11-101.41,6.18-11.39,37.26-69.61,103.42-42.24"
    ",55.71,23.05,100.66-23.31,100.66-23.31";

static ID2D1PathGeometry* ParseSvgPath(const char* d) {
    ID2D1PathGeometry* geo = nullptr;
    ID2D1GeometrySink* sink = nullptr;
    if (FAILED(g_d2d->CreatePathGeometry(&geo)) || FAILED(geo->Open(&sink))) { SafeRelease(geo); return nullptr; }
    const char* p = d;
    auto skip = [&] { while (*p == ' ' || *p == ',' || *p == '\n' || *p == '\t' || *p == '\r') p++; };
    auto isNum = [&] { skip(); return (*p >= '0' && *p <= '9') || *p == '-' || *p == '+' || *p == '.'; };
    auto num = [&]() -> float {
        skip();
        const char* s = p;
        if (*p == '-' || *p == '+') p++;
        bool dot = false;
        while ((*p >= '0' && *p <= '9') || (*p == '.' && !dot)) { if (*p == '.') dot = true; p++; }
        if (*p == 'e' || *p == 'E') { p++; if (*p == '-' || *p == '+') p++; while (*p >= '0' && *p <= '9') p++; }
        return (float)atof(std::string(s, p).c_str());
    };
    auto flag = [&]() -> bool { skip(); bool f = *p == '1'; p++; return f; };
    D2D1_POINT_2F cur = {0, 0}, start = {0, 0}, lastCtl = {0, 0};
    bool open = false;
    char cmd = 0, prev = 0;
    while (true) {
        skip();
        if (!*p) break;
        if (isalpha((unsigned char)*p)) cmd = *p++;
        else if (!isNum()) break;
        bool rel = islower((unsigned char)cmd) != 0;
        char C = (char)toupper((unsigned char)cmd);
        auto pt = [&](float x, float y) { return rel ? D2D1::Point2F(cur.x + x, cur.y + y) : D2D1::Point2F(x, y); };
        switch (C) {
        case 'M': {
            float x = num(), y = num();
            if (open) sink->EndFigure(D2D1_FIGURE_END_OPEN);
            cur = start = pt(x, y);
            sink->BeginFigure(cur, D2D1_FIGURE_BEGIN_HOLLOW);
            open = true;
            cmd = rel ? 'l' : 'L';
            break;
        }
        case 'L': { float x = num(), y = num(); D2D1_POINT_2F q = pt(x, y); sink->AddLine(q); cur = q; break; }
        case 'H': { float x = num(); D2D1_POINT_2F q = {rel ? cur.x + x : x, cur.y}; sink->AddLine(q); cur = q; break; }
        case 'V': { float y = num(); D2D1_POINT_2F q = {cur.x, rel ? cur.y + y : y}; sink->AddLine(q); cur = q; break; }
        case 'C': {
            float a = num(), b = num(), c = num(), e = num(), f = num(), g = num();
            D2D1_POINT_2F c1 = pt(a, b), c2 = pt(c, e), q = pt(f, g);
            sink->AddBezier(D2D1::BezierSegment(c1, c2, q));
            lastCtl = c2; cur = q;
            break;
        }
        case 'S': {
            float c = num(), e = num(), f = num(), g = num();
            char pc = (char)toupper((unsigned char)prev);
            D2D1_POINT_2F c1 = (pc == 'C' || pc == 'S') ? D2D1::Point2F(2 * cur.x - lastCtl.x, 2 * cur.y - lastCtl.y) : cur;
            D2D1_POINT_2F c2 = pt(c, e), q = pt(f, g);
            sink->AddBezier(D2D1::BezierSegment(c1, c2, q));
            lastCtl = c2; cur = q;
            break;
        }
        case 'Q': {
            float a = num(), b = num(), f = num(), g = num();
            D2D1_POINT_2F c1 = pt(a, b), q = pt(f, g);
            sink->AddQuadraticBezier(D2D1::QuadraticBezierSegment(c1, q));
            lastCtl = c1; cur = q;
            break;
        }
        case 'T': {
            float f = num(), g = num();
            char pc = (char)toupper((unsigned char)prev);
            D2D1_POINT_2F c1 = (pc == 'Q' || pc == 'T') ? D2D1::Point2F(2 * cur.x - lastCtl.x, 2 * cur.y - lastCtl.y) : cur;
            D2D1_POINT_2F q = pt(f, g);
            sink->AddQuadraticBezier(D2D1::QuadraticBezierSegment(c1, q));
            lastCtl = c1; cur = q;
            break;
        }
        case 'A': {
            float rx = num();
            float ry = num();
            float rot = num();
            bool large = flag();
            bool sweep = flag();
            float x = num(), y = num();
            D2D1_POINT_2F q = pt(x, y);
            sink->AddArc(D2D1::ArcSegment(q, D2D1::SizeF(rx, ry), rot,
                                          sweep ? D2D1_SWEEP_DIRECTION_CLOCKWISE : D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                                          large ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
            cur = q;
            break;
        }
        case 'Z':
            if (open) { sink->EndFigure(D2D1_FIGURE_END_CLOSED); open = false; }
            cur = start;
            break;
        default: p++; break;
        }
        prev = cmd;
    }
    if (open) sink->EndFigure(D2D1_FIGURE_END_OPEN);
    sink->Close();
    sink->Release();
    return geo;
}

// Outline geometry for a greeting in a given set of candidate fonts (first one with all glyphs wins).
static ID2D1PathGeometry* TextGeometry(const std::wstring& text, std::initializer_list<const wchar_t*> fams, float em) {
    IDWriteFontCollection* coll = nullptr;
    g_dw->GetSystemFontCollection(&coll);
    if (!coll) return nullptr;
    std::vector<UINT32> cps;
    for (size_t i = 0; i < text.size(); i++) {
        UINT32 c = text[i];
        if (IS_HIGH_SURROGATE(text[i]) && i + 1 < text.size()) { c = 0x10000 + ((c - 0xD800) << 10) + (text[i + 1] - 0xDC00); i++; }
        cps.push_back(c);
    }
    ID2D1PathGeometry* geo = nullptr;
    for (const wchar_t* fam : fams) {
        UINT32 idx;
        BOOL exists = FALSE;
        coll->FindFamilyName(fam, &idx, &exists);
        if (!exists) continue;
        IDWriteFontFamily* family = nullptr;
        IDWriteFont* font = nullptr;
        IDWriteFontFace* face = nullptr;
        coll->GetFontFamily(idx, &family);
        if (family) family->GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, &font);
        if (font) font->CreateFontFace(&face);
        if (face) {
            std::vector<UINT16> glyphs(cps.size());
            face->GetGlyphIndices(cps.data(), (UINT32)cps.size(), glyphs.data());
            bool complete = std::find(glyphs.begin(), glyphs.end(), (UINT16)0) == glyphs.end();
            if (complete) {
                DWRITE_FONT_METRICS fm;
                face->GetMetrics(&fm);
                std::vector<DWRITE_GLYPH_METRICS> gm(glyphs.size());
                face->GetDesignGlyphMetrics(glyphs.data(), (UINT32)glyphs.size(), gm.data(), FALSE);
                std::vector<FLOAT> adv(glyphs.size());
                for (size_t i = 0; i < glyphs.size(); i++) adv[i] = gm[i].advanceWidth * em / fm.designUnitsPerEm;
                ID2D1GeometrySink* sink = nullptr;
                if (SUCCEEDED(g_d2d->CreatePathGeometry(&geo)) && SUCCEEDED(geo->Open(&sink))) {
                    sink->SetFillMode(D2D1_FILL_MODE_WINDING);
                    face->GetGlyphRunOutline(em, glyphs.data(), adv.data(), nullptr, (UINT32)glyphs.size(), FALSE, FALSE, sink);
                    sink->Close();
                    sink->Release();
                }
            }
        }
        SafeRelease(face);
        SafeRelease(font);
        SafeRelease(family);
        if (geo) break;
    }
    coll->Release();
    return geo;
}

// ---------------------------------------------------------------------------
// Pages & widgets
// ---------------------------------------------------------------------------
enum Page { P_NONE, P_LOADING, P_HELLO, P_LANG, P_BACKEND, P_PERF, P_INSTALL, P_DONE, P_UPDATE, P_UPTODATE,
            P_UNINSTALL, P_UNINSTALLED, P_ERROR };
enum Id { B_NONE, B_CLOSE, B_MIN, B_CONTINUE, B_BACK, B_INSTALL, B_FINISH, B_UPDATE, B_NOTNOW, B_MODIFY,
          B_UNINSTALL, B_CLOSE2, B_UNINSTALL_GO, B_CANCEL, B_RETRY, B_RUST, B_CPP, B_HWACCEL, B_LANG0 = 100 };
enum BtnStyle { Primary, Secondary, Danger };

static Page g_page = P_NONE, g_prevPage = P_NONE;
static double g_pageStart = 0;
static int g_pageDir = 1;
static bool g_updateMode = false;  // the install page is an update
static bool g_installFailed = false, g_uninstalling = false;
static std::wstring g_errorText;

struct Hit { D2D1_RECT_F r; int id; };
static std::vector<Hit> g_hits, g_hitsNext;
static int g_hot = 0, g_press = 0;
static std::unordered_map<int, float> g_hoverA;
static bool g_interactive = true;

// hello animation
static ID2D1PathGeometry* g_helloGeo;
static bool g_helloIsText = false;
static float g_helloLen = 0;
static int g_helloLang = -1;
static double g_helloStart = 0;

// install progress
static std::atomic<float> g_progress{0};
static std::atomic<int> g_status{0};
static float g_progressShown = 0;

#define WM_APP_MANIFEST (WM_APP + 1)
#define WM_APP_INSTALLED (WM_APP + 2)
#define WM_APP_UNINSTALLED (WM_APP + 3)

static void GoTo(Page p, int dir = 1) {
    if (p == g_page) return;
    g_prevPage = g_page;
    g_page = p;
    g_pageStart = Now();
    g_pageDir = dir;
    if (p == P_HELLO) g_helloStart = Now();
}

static float HoverOf(int id) {
    auto it = g_hoverA.find(id);
    return it == g_hoverA.end() ? 0.f : it->second;
}

static void AddHit(D2D1_RECT_F r, int id) {
    if (g_interactive) g_hitsNext.push_back({r, id});
}

static void Button(int id, D2D1_RECT_F r, const std::wstring& label, BtnStyle st, float alpha) {
    AddHit(r, id);
    float h = HoverOf(id);
    bool pressed = g_press == id && g_hot == id;
    D2D1_COLOR_F fill, text = Col(0xFFFFFF, alpha), border = Col(0xFFFFFF, 0);
    switch (st) {
    case Primary: {
        D2D1_COLOR_F a = Col(0x0A7CFF), b = Col(0x3B98FF);
        fill = {a.r + (b.r - a.r) * h, a.g + (b.g - a.g) * h, a.b + (b.b - a.b) * h, alpha};
        if (pressed) fill = Col(0x0868D6, alpha);
        break;
    }
    case Danger:
        fill = Col(pressed ? 0xB3261E : 0xD13438, alpha * (0.85f + 0.15f * h));
        break;
    default:
        fill = Col(0xFFFFFF, alpha * (0.07f + 0.06f * h + (pressed ? 0.03f : 0)));
        border = Col(0xFFFFFF, alpha * 0.10f);
    }
    float sc = pressed ? 0.975f : 1.f;
    D2D1_MATRIX_3X2_F old;
    g_rt->GetTransform(&old);
    D2D1_POINT_2F c = {(r.left + r.right) / 2, (r.top + r.bottom) / 2};
    g_rt->SetTransform(Matrix3x2F::Scale(sc, sc, c) * Matrix3x2F::ReinterpretBaseType(&old)[0]);
    D2D1_ROUNDED_RECT rr = D2D1::RoundedRect(r, 8, 8);
    if (st == Primary && h > 0.01f) {  // soft glow on hover
        g_solid->SetColor(Col(0x3B98FF, 0.18f * h * alpha));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(r.left - 3, r.top - 2, r.right + 3, r.bottom + 4), 11, 11), g_solid);
    }
    g_solid->SetColor(fill);
    g_rt->FillRoundedRectangle(rr, g_solid);
    if (border.a > 0) {
        g_solid->SetColor(border);
        g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(r.left + .5f, r.top + .5f, r.right - .5f, r.bottom - .5f), 7.5f, 7.5f), g_solid, 1);
    }
    Text(label, g_fBtn, r, text);
    g_rt->SetTransform(old);
}

static D2D1_RECT_F CenterRect(float cx, float cy, float w, float h) {
    return D2D1::RectF(cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2);
}

// CSS-style linear-gradient(angle, color, transparent stop%) over a rect.
static void CssGradient(D2D1_RECT_F r, float deg, UINT32 rgb, float stop, float alpha) {
    float a = deg * 3.14159265f / 180.f;
    float w = r.right - r.left, h = r.bottom - r.top;
    D2D1_POINT_2F dir = {std::sin(a), -std::cos(a)};
    float len = std::fabs(w * std::sin(a)) + std::fabs(h * std::cos(a));
    D2D1_POINT_2F c = {r.left + w / 2, r.top + h / 2};
    D2D1_POINT_2F p0 = {c.x - dir.x * len / 2, c.y - dir.y * len / 2}, p1 = {c.x + dir.x * len / 2, c.y + dir.y * len / 2};
    D2D1_GRADIENT_STOP stops[2] = {{0, Col(rgb, alpha)}, {stop, Col(rgb, 0)}};
    ID2D1GradientStopCollection* coll = nullptr;
    ID2D1LinearGradientBrush* br = nullptr;
    if (SUCCEEDED(g_rt->CreateGradientStopCollection(stops, 2, &coll)) &&
        SUCCEEDED(g_rt->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties(p0, p1), coll, &br))) {
        g_rt->FillRectangle(r, br);
    }
    SafeRelease(br);
    SafeRelease(coll);
}

static void EnsureHello() {
    if (g_helloLang == g_lang && g_helloGeo) return;
    SafeRelease(g_helloGeo);
    g_helloLang = g_lang;
    std::wstring code = kLangs[g_lang].code;
    if (code == L"en") {
        g_helloGeo = ParseSvgPath(kHelloPath);
        g_helloIsText = false;
    } else {
        std::wstring word = kLangs[g_lang].s[S_HELLO];
        if (code == L"ja") g_helloGeo = TextGeometry(word, {L"UD Digi Kyokasho NK-R", L"UD Digi Kyokasho N-R", L"Yu Gothic UI", L"Meiryo"}, 96);
        else if (code == L"zh") g_helloGeo = TextGeometry(word, {L"KaiTi", L"STKaiti", L"Microsoft YaHei UI"}, 110);
        else if (code == L"ko") g_helloGeo = TextGeometry(word, {L"Malgun Gothic"}, 92);
        else g_helloGeo = TextGeometry(word, {L"Segoe Script", L"Segoe Print", L"Segoe UI"}, 120);
        g_helloIsText = true;
    }
    g_helloLen = 0;
    if (g_helloGeo) g_helloGeo->ComputeLength(nullptr, &g_helloLen);
}

static void DrawHello(float alpha, float dx, double now) {
    using namespace Ui;
    EnsureHello();
    double t = now - g_helloStart;
    const double delay = 0.55, drawDur = g_helloIsText ? 3.2 : 3.6;
    float prog = (float)EaseInOutSine(Clamp01((t - delay) / drawDur));

    if (g_helloGeo && prog > 0.f) {
        D2D1_RECT_F b;
        if (g_helloIsText) g_helloGeo->GetBounds(nullptr, &b);
        else g_helloGeo->GetWidenedBounds(35.f, nullptr, nullptr, D2D1_DEFAULT_FLATTENING_TOLERANCE, &b);  // hollow path
        float targetW = g_helloIsText ? 460.f : 430.f;
        float k = std::min(targetW / (b.right - b.left), 190.f / (b.bottom - b.top));
        float cx = W / 2 + dx, cy = H * 0.42f;
        Matrix3x2F m = Matrix3x2F::Translation(-(b.left + b.right) / 2, -(b.top + b.bottom) / 2) *
                       Matrix3x2F::Scale(k, k) * Matrix3x2F::Translation(cx, cy);
        D2D1_MATRIX_3X2_F old;
        g_rt->GetTransform(&old);
        g_rt->SetTransform(m * Matrix3x2F::ReinterpretBaseType(&old)[0]);
        float sw = g_helloIsText ? 2.4f / k : 35.f;  // the CodePen stroke is 35 units wide
        g_solid->SetColor(Col(0xFFFFFF, alpha));
        if (prog >= 1.f) {
            ID2D1StrokeStyle* ss = nullptr;
            D2D1_STROKE_STYLE_PROPERTIES sp = D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND,
                                                                         D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND);
            g_d2d->CreateStrokeStyle(sp, nullptr, 0, &ss);
            g_rt->DrawGeometry(g_helloGeo, g_solid, sw, ss);
            SafeRelease(ss);
        } else {
            float dash[2] = {std::max(0.001f, g_helloLen * prog / sw), (g_helloLen + sw * 4) / sw};
            ID2D1StrokeStyle* ss = nullptr;
            D2D1_STROKE_STYLE_PROPERTIES sp = D2D1::StrokeStyleProperties(
                D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND, 10.f,
                D2D1_DASH_STYLE_CUSTOM, 0.f);
            g_d2d->CreateStrokeStyle(sp, dash, 2, &ss);
            g_rt->DrawGeometry(g_helloGeo, g_solid, sw, ss);
            SafeRelease(ss);
        }
        if (g_helloIsText) {  // the letters fill in once traced
            float f = (float)EaseOutCubic(Clamp01((t - delay - drawDur + 0.35) / 0.6));
            if (f > 0) {
                g_solid->SetColor(Col(0xFFFFFF, alpha * f));
                g_rt->FillGeometry(g_helloGeo, g_solid);
            }
        }
        g_rt->SetTransform(old);
    }

    // Continue button rises in when the drawing is done.
    float c = (float)EaseOutCubic(Clamp01((t - delay - drawDur + 0.1) / 0.6));
    if (c > 0.f) {
        float y = H * 0.76f + (1 - c) * 18;
        Button(B_CONTINUE, CenterRect(W / 2 + dx, y, 220, 44), T(S_CONTINUE), Primary, alpha * c);
    }
}

static void Header(const std::wstring& title, const std::wstring& sub, float alpha, float dx, float y = 120) {
    using namespace Ui;
    Text(title, g_fTitle, D2D1::RectF(40 + dx, y - 26, W - 40 + dx, y + 18), Col(0xFFFFFF, alpha));
    if (!sub.empty()) Text(sub, g_fSub, D2D1::RectF(60 + dx, y + 18, W - 60 + dx, y + 48), Col(0xFFFFFF, 0.68f * alpha));
}

static void FooterButtons(float alpha, float dx, int backId, const std::wstring& back, int nextId,
                          const std::wstring& next, BtnStyle nextStyle = Primary) {
    using namespace Ui;
    float y = H - 56;
    if (backId) Button(backId, D2D1::RectF(40 + dx, y - 20, 180 + dx, y + 20), back, Secondary, alpha);
    if (nextId) Button(nextId, D2D1::RectF(W - 220 + dx, y - 20, W - 40 + dx, y + 20), next, nextStyle, alpha);
}

static void DrawLang(float alpha, float dx, float dt) {
    using namespace Ui;
    Header(T(S_LANG_TITLE), T(S_LANG_SUB), alpha, dx);
    const int cols = 3;
    float tw = 220, th = 46, gap = 12;
    float gw = cols * tw + (cols - 1) * gap;
    float x0 = (W - gw) / 2 + dx, y0 = 186;
    for (int i = 0; i < kLangCount; i++) {
        float x = x0 + (i % cols) * (tw + gap), y = y0 + (i / cols) * (th + gap);
        D2D1_RECT_F r = D2D1::RectF(x, y, x + tw, y + th);
        int id = B_LANG0 + i;
        AddHit(r, id);
        float h = HoverOf(id);
        bool sel = i == g_lang;
        g_solid->SetColor(Col(sel ? 0x0A7CFF : 0xFFFFFF, alpha * (sel ? 0.22f : 0.06f + 0.06f * h)));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, 9, 9), g_solid);
        g_solid->SetColor(Col(sel ? 0x4DA3FF : 0xFFFFFF, alpha * (sel ? 0.9f : 0.08f)));
        g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + .5f, y + .5f, x + tw - .5f, y + th - .5f), 8.5f, 8.5f), g_solid, sel ? 1.5f : 1.f);
        Text(kLangs[i].native, g_fBody, D2D1::RectF(x + 16, y, x + tw - 40, y + th), Col(0xFFFFFF, alpha), DWRITE_TEXT_ALIGNMENT_LEADING);
        if (sel) Text(L"", g_fGlyph, D2D1::RectF(x + tw - 36, y, x + tw - 12, y + th), Col(0x8CC4FF, alpha));
    }
    FooterButtons(alpha, dx, B_BACK, T(S_BACK), B_CONTINUE, T(S_CONTINUE));
}

static void BackendCard(int id, D2D1_RECT_F r, const std::wstring& mono, UINT32 monoCol, const std::wstring& name,
                        const std::wstring& desc, bool sel, bool recommended, float alpha) {
    AddHit(r, id);
    float h = HoverOf(id);
    g_solid->SetColor(Col(sel ? 0x0A7CFF : 0xFFFFFF, alpha * (sel ? 0.18f : 0.06f + 0.05f * h)));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, 14, 14), g_solid);
    g_solid->SetColor(Col(sel ? 0x4DA3FF : 0xFFFFFF, alpha * (sel ? 0.95f : 0.09f)));
    g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(r.left + .5f, r.top + .5f, r.right - .5f, r.bottom - .5f), 13.5f, 13.5f), g_solid, sel ? 2.f : 1.f);
    float cx = (r.left + r.right) / 2;
    D2D1_ELLIPSE e = D2D1::Ellipse({cx, r.top + 62}, 32, 32);
    g_solid->SetColor(Col(monoCol, alpha));
    g_rt->FillEllipse(e, g_solid);
    Text(mono, g_fBig, D2D1::RectF(cx - 40, r.top + 30, cx + 40, r.top + 94), Col(0xFFFFFF, alpha));
    Text(name, g_fTitle, D2D1::RectF(r.left, r.top + 104, r.right, r.top + 140), Col(0xFFFFFF, alpha));
    Text(desc, g_fBody, D2D1::RectF(r.left + 20, r.top + 140, r.right - 20, r.top + 172), Col(0xFFFFFF, 0.7f * alpha));
    if (recommended) {
        D2D1_RECT_F b = D2D1::RectF(cx - 56, r.bottom - 38, cx + 56, r.bottom - 14);
        g_solid->SetColor(Col(0x0A7CFF, alpha * 0.25f));
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(b, 12, 12), g_solid);
        Text(T(S_RECOMMENDED), g_fSmall, b, Col(0x9CCBFF, alpha));
    }
}

static void DrawBackend(float alpha, float dx) {
    using namespace Ui;
    Header(T(S_BACKEND_TITLE), T(S_BACKEND_SUB), alpha, dx);
    float cw = 300, ch = 236, gap = 24, y = 190;
    float x0 = (W - 2 * cw - gap) / 2 + dx;
    BackendCard(B_RUST, D2D1::RectF(x0, y, x0 + cw, y + ch), L"Rs", 0xB7410E, L"Rust", T(S_RUST_DESC), g_backend == L"rust", false, alpha);
    BackendCard(B_CPP, D2D1::RectF(x0 + cw + gap, y, x0 + 2 * cw + gap, y + ch), L"C++", 0x3F6FB5, L"C++", T(S_CPP_DESC), g_backend == L"cpp", true, alpha);
    FooterButtons(alpha, dx, B_BACK, T(S_BACK), B_CONTINUE, T(S_CONTINUE));
}

static float TextWidth(const std::wstring& str, IDWriteTextFormat* f) {
    IDWriteTextLayout* l = nullptr;
    DWRITE_TEXT_METRICS m = {};
    if (SUCCEEDED(g_dw->CreateTextLayout(str.c_str(), (UINT32)str.size(), f, 10000.f, 100.f, &l))) {
        l->GetMetrics(&m);
        l->Release();
    }
    return m.widthIncludingTrailingWhitespace;
}

// "Performance": one card with the Hardware acceleration switch (on, and Recommended, by default).
static void DrawPerf(float alpha, float dx, float dt) {
    using namespace Ui;
    Header(T(S_PERF_TITLE), T(S_PERF_SUB), alpha, dx);
    g_hwAnim = Damp(g_hwAnim, g_hwAccel ? 1.f : 0.f, 18.f, dt);
    float k = g_hwAnim;

    float cw = 620, ch = 140, x = (W - cw) / 2 + dx, y = 200;
    D2D1_RECT_F r = D2D1::RectF(x, y, x + cw, y + ch);
    AddHit(r, B_HWACCEL);
    float h = HoverOf(B_HWACCEL);
    g_solid->SetColor(Col(0xFFFFFF, alpha * (0.06f + 0.04f * h)));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(r, 14, 14), g_solid);
    g_solid->SetColor(Col(0xFFFFFF, alpha * 0.09f));
    g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(x + .5f, y + .5f, x + cw - .5f, y + ch - .5f), 13.5f, 13.5f), g_solid, 1.f);

    // icon: a lightning bolt in a tile that lights up with the switch
    D2D1_RECT_F tile = D2D1::RectF(x + 28, y + ch / 2 - 28, x + 84, y + ch / 2 + 28);
    g_solid->SetColor(Col(0x0A7CFF, alpha * (0.18f + 0.62f * k)));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(tile, 14, 14), g_solid);
    Text(L"\uE945", g_fIcon, tile, Col(0xFFFFFF, alpha * (0.7f + 0.3f * k)));

    // name + Recommended badge
    float tx = x + 108, sw = 52, right = x + cw - 28 - sw - 20;
    std::wstring name = T(S_HWACCEL);
    float nw = std::min(TextWidth(name, g_fCardTitle), right - tx - 120);
    Text(name, g_fCardTitle, D2D1::RectF(tx, y + 24, tx + nw + 2, y + 52), Col(0xFFFFFF, alpha), DWRITE_TEXT_ALIGNMENT_LEADING);
    std::wstring rec = T(S_RECOMMENDED);
    float bw = TextWidth(rec, g_fSmall) + 22;
    D2D1_RECT_F b = D2D1::RectF(tx + nw + 12, y + 27, tx + nw + 12 + bw, y + 49);
    g_solid->SetColor(Col(0x0A7CFF, alpha * 0.25f));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(b, 11, 11), g_solid);
    Text(rec, g_fSmall, b, Col(0x9CCBFF, alpha));
    Text(T(S_HWACCEL_DESC), g_fBody, D2D1::RectF(tx, y + 58, right, y + ch - 20), Col(0xFFFFFF, 0.7f * alpha),
         DWRITE_TEXT_ALIGNMENT_LEADING, true);

    // switch
    float sx = x + cw - 28 - sw, sy = y + ch / 2;
    D2D1_RECT_F track = D2D1::RectF(sx, sy - 14, sx + sw, sy + 14);
    D2D1_COLOR_F off = Col(0xFFFFFF, alpha * (0.10f + 0.05f * h)), on = Col(0x0A7CFF, alpha);
    g_solid->SetColor({off.r + (on.r - off.r) * k, off.g + (on.g - off.g) * k, off.b + (on.b - off.b) * k,
                       off.a + (on.a - off.a) * k});
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(track, 14, 14), g_solid);
    if (k < 0.99f) {
        g_solid->SetColor(Col(0xFFFFFF, alpha * 0.45f * (1 - k)));
        g_rt->DrawRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(sx + .5f, sy - 13.5f, sx + sw - .5f, sy + 13.5f), 13.5f, 13.5f), g_solid, 1.f);
    }
    float kr = 9.f + 1.f * k, kx = sx + 14 + (sw - 28) * k;
    g_solid->SetColor(Col(0xFFFFFF, alpha * (0.75f + 0.25f * k)));
    g_rt->FillEllipse(D2D1::Ellipse({kx, sy}, kr, kr), g_solid);

    Text(T(S_HWACCEL_NOTE), g_fSmall, D2D1::RectF(40 + dx, y + ch + 18, W - 40 + dx, y + ch + 40), Col(0xFFFFFF, 0.5f * alpha));
    FooterButtons(alpha, dx, B_BACK, T(S_BACK), B_INSTALL, T(S_INSTALL));
}

static void Spinner(float cx, float cy, float r, float alpha, double now) {
    float a0 = (float)std::fmod(now * 360.0, 360.0);
    float sweep = 100 + 60 * (float)std::sin(now * 3.0);
    ID2D1PathGeometry* g = nullptr;
    ID2D1GeometrySink* s = nullptr;
    if (FAILED(g_d2d->CreatePathGeometry(&g)) || FAILED(g->Open(&s))) { SafeRelease(g); return; }
    auto P = [&](float deg) { float a = deg * 3.14159265f / 180; return D2D1::Point2F(cx + r * std::cos(a), cy + r * std::sin(a)); };
    s->BeginFigure(P(a0), D2D1_FIGURE_BEGIN_HOLLOW);
    s->AddArc(D2D1::ArcSegment(P(a0 + sweep), D2D1::SizeF(r, r), 0, D2D1_SWEEP_DIRECTION_CLOCKWISE,
                               sweep > 180 ? D2D1_ARC_SIZE_LARGE : D2D1_ARC_SIZE_SMALL));
    s->EndFigure(D2D1_FIGURE_END_OPEN);
    s->Close();
    s->Release();
    ID2D1StrokeStyle* ss = nullptr;
    g_d2d->CreateStrokeStyle(D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND), nullptr, 0, &ss);
    g_solid->SetColor(Col(0x4DA3FF, alpha));
    g_rt->DrawGeometry(g, g_solid, 3.5f, ss);
    SafeRelease(ss);
    g->Release();
}

static void DrawInstall(float alpha, float dx, float dt, double now) {
    using namespace Ui;
    Header(T(S_INSTALLING), g_status == 0 ? T(S_DOWNLOADING) : T(S_FINISHING), alpha, dx, 200);
    g_progressShown = Damp(g_progressShown, g_progress.load(), 8.f, dt);
    float bw = 440, bx = (W - bw) / 2 + dx, by = 290;
    g_solid->SetColor(Col(0xFFFFFF, 0.1f * alpha));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(bx, by, bx + bw, by + 6), 3, 3), g_solid);
    float pw = std::max(6.f, bw * g_progressShown);
    D2D1_GRADIENT_STOP st[2] = {{0, Col(0x0A7CFF, alpha)}, {1, Col(0x5CB8FF, alpha)}};
    ID2D1GradientStopCollection* coll = nullptr;
    ID2D1LinearGradientBrush* br = nullptr;
    if (SUCCEEDED(g_rt->CreateGradientStopCollection(st, 2, &coll)) &&
        SUCCEEDED(g_rt->CreateLinearGradientBrush(D2D1::LinearGradientBrushProperties({bx, 0}, {bx + pw, 0}), coll, &br)))
        g_rt->FillRoundedRectangle(D2D1::RoundedRect(D2D1::RectF(bx, by, bx + pw, by + 6), 3, 3), br);
    SafeRelease(br);
    SafeRelease(coll);
    // shimmer
    float sx = bx + std::fmod((float)now * 260.f, pw + 80.f) - 40.f;
    g_rt->PushAxisAlignedClip(D2D1::RectF(bx, by, bx + pw, by + 6), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    g_solid->SetColor(Col(0xFFFFFF, 0.35f * alpha));
    g_rt->FillRectangle(D2D1::RectF(sx, by, sx + 40, by + 6), g_solid);
    g_rt->PopAxisAlignedClip();
    wchar_t pct[16];
    swprintf_s(pct, L"%d%%", (int)std::lround(g_progressShown * 100));
    Text(pct, g_fSmall, D2D1::RectF(bx, by + 14, bx + bw, by + 36), Col(0xFFFFFF, 0.6f * alpha));
}

static void CheckMark(float cx, float cy, float r, float alpha, double t) {
    float ring = (float)EaseOutCubic(Clamp01(t / 0.5));
    float tick = (float)EaseOutCubic(Clamp01((t - 0.3) / 0.45));
    g_solid->SetColor(Col(0x0A7CFF, alpha * ring));
    float rr = r * (0.6f + 0.4f * ring);
    g_rt->FillEllipse(D2D1::Ellipse({cx, cy}, rr, rr), g_solid);
    if (tick > 0) {
        ID2D1PathGeometry* g = nullptr;
        ID2D1GeometrySink* s = nullptr;
        if (SUCCEEDED(g_d2d->CreatePathGeometry(&g)) && SUCCEEDED(g->Open(&s))) {
            s->BeginFigure({cx - r * 0.38f, cy + r * 0.02f}, D2D1_FIGURE_BEGIN_HOLLOW);
            s->AddLine({cx - r * 0.1f, cy + r * 0.3f});
            s->AddLine({cx + r * 0.42f, cy - r * 0.28f});
            s->EndFigure(D2D1_FIGURE_END_OPEN);
            s->Close();
            s->Release();
            float len = 0;
            g->ComputeLength(nullptr, &len);
            float sw = r * 0.14f;
            float dash[2] = {std::max(0.001f, len * tick / sw), len * 2 / sw};
            ID2D1StrokeStyle* ss = nullptr;
            g_d2d->CreateStrokeStyle(D2D1::StrokeStyleProperties(D2D1_CAP_STYLE_ROUND, D2D1_CAP_STYLE_ROUND,
                                                                 D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND, 10.f,
                                                                 D2D1_DASH_STYLE_CUSTOM, 0.f),
                                     dash, 2, &ss);
            g_solid->SetColor(Col(0xFFFFFF, alpha));
            g_rt->DrawGeometry(g, g_solid, sw, ss);
            SafeRelease(ss);
        }
        SafeRelease(g);
    }
}

static std::wstring BackendName(const std::wstring& b) { return b == L"rust" ? L"Rust" : L"C++"; }

static void DrawDone(float alpha, float dx, double now) {
    using namespace Ui;
    CheckMark(W / 2 + dx, 190, 42, alpha, now - g_pageStart);
    Header(T(S_DONE_TITLE), T(S_DONE_SUB), alpha, dx, 290);
    Button(B_FINISH, CenterRect(W / 2 + dx, H - 120, 220, 44), T(S_FINISH), Primary, alpha);
}

static float g_updateScroll = 0, g_updateContentH = 0;
static D2D1_RECT_F g_updateBox = {};

static void DrawUpdate(float alpha, float dx) {
    using namespace Ui;
    Manifest m;
    { std::lock_guard<std::mutex> lk(g_mx); m = g_manifest; }
    Text(L"Custom Start", g_fSmall, D2D1::RectF(40 + dx, 62, W - 40 + dx, 82), Col(0x9CCBFF, alpha));
    Header(Fmt(S_UPDATE_TITLE, m.version), Fmt(S_UPDATE_SUB, g_installedVersion), alpha, dx, 112);

    D2D1_RECT_F box = D2D1::RectF(90 + dx, 178, W - 90 + dx, H - 96);
    g_solid->SetColor(Col(0x000000, 0.18f * alpha));
    g_rt->FillRoundedRectangle(D2D1::RoundedRect(box, 12, 12), g_solid);
    g_solid->SetColor(Col(0xFFFFFF, 0.08f * alpha));
    g_rt->DrawRoundedRectangle(D2D1::RoundedRect(box, 12, 12), g_solid, 1);
    g_rt->PushAxisAlignedClip(D2D1::RectF(box.left, box.top + 4, box.right, box.bottom - 4), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    g_updateBox = box;
    float y = box.top + 14 - g_updateScroll;
    float textW = (box.right - 20) - (box.left + 52);
    for (const Change& c : m.releases) {
        if (CompareVersions(c.version, g_installedVersion) <= 0) continue;
        Text(L"V" + c.version, g_fBtn, D2D1::RectF(box.left + 20, y, box.right - 20, y + 26), Col(0xFFFFFF, alpha), DWRITE_TEXT_ALIGNMENT_LEADING);
        y += 30;
        auto line = [&](const std::wstring& s, bool add) {
            UINT32 col = add ? 0x6CCB5F : 0xFF6B6B;
            D2D1_RECT_F pill = D2D1::RectF(box.left + 20, y + 3, box.left + 40, y + 21);
            g_solid->SetColor(Col(col, 0.18f * alpha));
            g_rt->FillRoundedRectangle(D2D1::RoundedRect(pill, 5, 5), g_solid);
            Text(add ? L"+" : L"−", g_fBtn, pill, Col(col, alpha));
            // long lines wrap instead of running off the edge
            float h = 24;
            IDWriteTextLayout* l = nullptr;
            if (SUCCEEDED(g_dw->CreateTextLayout(s.c_str(), (UINT32)s.size(), g_fBody, textW, 1000, &l))) {
                l->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                l->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
                l->SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
                DWRITE_TEXT_METRICS tm{};
                l->GetMetrics(&tm);
                float th = tm.height;
                h = std::max(24.f, th + 4);
                g_solid->SetColor(Col(col, alpha));
                g_rt->DrawTextLayout({box.left + 52, y + 2}, l, g_solid);
                l->Release();
            }
            y += h + 4;
        };
        for (auto& s : c.added) line(s, true);
        for (auto& s : c.removed) line(s, false);
        y += 10;
    }
    g_updateContentH = y + g_updateScroll - box.top;
    g_rt->PopAxisAlignedClip();
    FooterButtons(alpha, dx, B_NOTNOW, T(S_NOT_NOW), B_UPDATE, T(S_UPDATE));
}

static void DrawUpToDate(float alpha, float dx, double now) {
    using namespace Ui;
    CheckMark(W / 2 + dx, 170, 36, alpha, now - g_pageStart);
    Header(T(S_UPTODATE), Fmt(S_VERSION_LINE, g_installedVersion, BackendName(g_installedBackend)), alpha, dx, 262);
    float y = H - 130;
    Button(B_MODIFY, CenterRect(W / 2 - 170 + dx, y, 150, 42), T(S_MODIFY), Secondary, alpha);
    Button(B_UNINSTALL, CenterRect(W / 2 + dx, y, 150, 42), T(S_UNINSTALL), Secondary, alpha);
    Button(B_CLOSE2, CenterRect(W / 2 + 170 + dx, y, 150, 42), T(S_CLOSE), Primary, alpha);
}

static void DrawUninstall(float alpha, float dx) {
    using namespace Ui;
    Text(L"", g_fMono, CenterRect(W / 2 + dx, 170, 80, 80), Col(0xFF6B6B, alpha));
    Header(T(S_UNINSTALL_TITLE), T(S_UNINSTALL_SUB), alpha, dx, 262);
    FooterButtons(alpha, dx, B_CANCEL, T(S_CANCEL), B_UNINSTALL_GO, T(S_UNINSTALL), Danger);
}

static void DrawUninstalled(float alpha, float dx, double now) {
    using namespace Ui;
    CheckMark(W / 2 + dx, 190, 42, alpha, now - g_pageStart);
    Header(T(S_UNINSTALLED), L"", alpha, dx, 290);
    Button(B_CLOSE2, CenterRect(W / 2 + dx, H - 120, 220, 44), T(S_CLOSE), Primary, alpha);
}

static void DrawError(float alpha, float dx) {
    using namespace Ui;
    Text(L"", g_fMono, CenterRect(W / 2 + dx, 170, 80, 80), Col(0xFFB454, alpha));
    Header(g_installFailed ? T(S_INSTALL_FAILED) : T(S_OFFLINE_TITLE), g_errorText, alpha, dx, 262);
    FooterButtons(alpha, dx, B_CLOSE2, T(S_CLOSE), B_RETRY, T(S_RETRY));
}

static void DrawLoading(float alpha, float dx, double now) {
    using namespace Ui;
    Spinner(W / 2 + dx, H / 2 - 20, 18, alpha, now);
    Text(T(S_CHECKING), g_fSub, D2D1::RectF(dx, H / 2 + 10, W + dx, H / 2 + 40), Col(0xFFFFFF, 0.7f * alpha));
}

static void DrawPage(Page p, float alpha, float dx, float dt, double now) {
    switch (p) {
    case P_LOADING: DrawLoading(alpha, dx, now); break;
    case P_HELLO: DrawHello(alpha, dx, now); break;
    case P_LANG: DrawLang(alpha, dx, dt); break;
    case P_BACKEND: DrawBackend(alpha, dx); break;
    case P_PERF: DrawPerf(alpha, dx, dt); break;
    case P_INSTALL: DrawInstall(alpha, dx, dt, now); break;
    case P_DONE: DrawDone(alpha, dx, now); break;
    case P_UPDATE: DrawUpdate(alpha, dx); break;
    case P_UPTODATE: DrawUpToDate(alpha, dx, now); break;
    case P_UNINSTALL: DrawUninstall(alpha, dx); break;
    case P_UNINSTALLED: DrawUninstalled(alpha, dx, now); break;
    case P_ERROR: DrawError(alpha, dx); break;
    default: break;
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------
static float g_helloBgA = 0;

static bool CreateDevice() {
    if (g_rt) return true;
    RECT rc;
    GetClientRect(g_hwnd, &rc);
    D2D1_RENDER_TARGET_PROPERTIES props = D2D1::RenderTargetProperties(
        D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED));
    D2D1_HWND_RENDER_TARGET_PROPERTIES hp =
        D2D1::HwndRenderTargetProperties(g_hwnd, D2D1::SizeU(rc.right, rc.bottom), D2D1_PRESENT_OPTIONS_NONE);
    if (FAILED(g_d2d->CreateHwndRenderTarget(props, hp, &g_rt))) return false;
    g_rt->SetDpi(g_dpi, g_dpi);
    g_rt->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
    g_rt->CreateSolidColorBrush(ColorF(0xFFFFFF), &g_solid);
    return true;
}

static void Render() {
    if (!CreateDevice()) return;
    using namespace Ui;
    double now = Now();
    float dt = (float)std::clamp(now - g_lastFrame, 0.0, 0.05);
    g_lastFrame = now;

    // hover animation
    for (auto& kv : g_hoverA) kv.second = Damp(kv.second, kv.first == g_hot ? 1.f : 0.f, 16.f, dt);
    if (g_hot && !g_hoverA.count(g_hot)) g_hoverA[g_hot] = 0;

    g_rt->BeginDraw();
    g_rt->SetTransform(Matrix3x2F::Identity());
    g_rt->Clear(ColorF(0, 0));

    // Translucent theme tint over the live acrylic backdrop.
    D2D1_RECT_F all = D2D1::RectF(0, 0, W, H);
    if (!g_acrylic) {
        g_solid->SetColor(Col(0x1B1B1D));
        g_rt->FillRectangle(all, g_solid);
    }
    g_solid->SetColor(Col(0x1A1A1C, g_acrylic ? 0.42f : 1.f));
    g_rt->FillRectangle(all, g_solid);

    // "hello" pastel backdrop (the CodePen's three layered gradients), only on the hello page.
    g_helloBgA = Damp(g_helloBgA, g_page == P_HELLO ? 1.f : 0.f, 4.f, dt);
    if (g_helloBgA > 0.005f) {
        float a = 0.62f * g_helloBgA;
        CssGradient(all, 217, 0xFF9FED, 0.7071f, a);
        CssGradient(all, 127, 0x76A4FF, 0.65f, a);
        CssGradient(all, 336, 0xFFBF8A, 0.7071f, a);
    }

    // page transition: outgoing slides/fades away, incoming slides in
    double tt = Clamp01((now - g_pageStart) / 0.5);
    float e = (float)EaseOutCubic(tt);
    g_hitsNext.clear();
    if (g_prevPage != P_NONE && tt < 1) {
        g_interactive = false;
        DrawPage(g_prevPage, 1 - e, -60.f * e * g_pageDir, dt, now);
    }
    g_interactive = true;
    DrawPage(g_page, e, 60.f * (1 - e) * g_pageDir, dt, now);

    // title bar
    g_solid->SetColor(Col(0x4CC2FF));
    for (int i = 0; i < 4; i++) {
        float x = 18 + (i % 2) * 7.f, y = 15 + (i / 2) * 7.f;
        g_rt->FillRectangle(D2D1::RectF(x, y, x + 6, y + 6), g_solid);
    }
    Text(T(S_SETUP), g_fSmall, D2D1::RectF(40, 0, 400, TitleH - 4), Col(0xFFFFFF, 0.75f), DWRITE_TEXT_ALIGNMENT_LEADING);
    struct Cap { int id; const wchar_t* glyph; float x; } caps[] = {{B_MIN, L"", W - 92}, {B_CLOSE, L"", W - 46}};
    for (auto& c : caps) {
        D2D1_RECT_F r = D2D1::RectF(c.x, 0, c.x + 46, 34);
        g_hitsNext.push_back({r, c.id});
        float h = HoverOf(c.id);
        if (h > 0.01f) {
            g_solid->SetColor(c.id == B_CLOSE ? Col(0xC42B1C, h) : Col(0xFFFFFF, 0.08f * h));
            g_rt->FillRectangle(r, g_solid);
        }
        Text(c.glyph, g_fGlyph, r, Col(0xFFFFFF, 0.85f));
    }

    HRESULT hr = g_rt->EndDraw();
    g_hits.swap(g_hitsNext);
    if (hr == D2DERR_RECREATE_TARGET) { SafeRelease(g_solid); SafeRelease(g_rt); }
}

static int HitAt(POINT px) {
    float s = 96.f / g_dpi;
    float x = px.x * s, y = px.y * s;
    for (auto it = g_hits.rbegin(); it != g_hits.rend(); ++it)
        if (x >= it->r.left && x <= it->r.right && y >= it->r.top && y <= it->r.bottom) return it->id;
    return 0;
}

// ---------------------------------------------------------------------------
// Install / uninstall work
// ---------------------------------------------------------------------------
static void StopApp() {
    for (int tries = 0; tries < 2; tries++) {
        HWND w = FindWindowW(kAppClass, nullptr);
        if (!w) return;
        DWORD pid = 0;
        GetWindowThreadProcessId(w, &pid);
        HANDLE proc = OpenProcess(SYNCHRONIZE | PROCESS_TERMINATE, FALSE, pid);
        PostMessageW(w, WM_CSM_QUIT, 0, 0);
        if (proc) {
            if (WaitForSingleObject(proc, 3000) == WAIT_TIMEOUT) TerminateProcess(proc, 0), WaitForSingleObject(proc, 2000);
            CloseHandle(proc);
        } else {
            Sleep(800);
        }
    }
}

static bool WriteFileAtomic(const std::wstring& path, const std::string& data) {
    std::wstring tmp = path + L".download";
    HANDLE f = CreateFileW(tmp.c_str(), GENERIC_WRITE, 0, nullptr, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (f == INVALID_HANDLE_VALUE) return false;
    DWORD w = 0;
    bool ok = WriteFile(f, data.data(), (DWORD)data.size(), &w, nullptr) && w == data.size();
    CloseHandle(f);
    if (!ok) { DeleteFileW(tmp.c_str()); return false; }
    for (int i = 0; i < 20; i++) {
        if (MoveFileExW(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING)) return true;
        Sleep(150);
    }
    DeleteFileW(tmp.c_str());
    return false;
}

static void MakeShortcut(const std::wstring& lnk, const std::wstring& target, const std::wstring& args,
                         const std::wstring& desc) {
    IShellLinkW* sl = nullptr;
    if (FAILED(CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&sl)))) return;
    sl->SetPath(target.c_str());
    sl->SetArguments(args.c_str());
    sl->SetDescription(desc.c_str());
    sl->SetIconLocation(target.c_str(), 0);
    IPersistFile* pf = nullptr;
    if (SUCCEEDED(sl->QueryInterface(IID_PPV_ARGS(&pf)))) {
        pf->Save(lnk.c_str(), TRUE);
        pf->Release();
    }
    sl->Release();
}

static bool FetchManifest(std::wstring& err) {
    // Ask both sources and keep whichever knows the newer version: either one can lag behind.
    Manifest m;
    bool have = false;
    {
        std::string body;
        std::wstring e;
        Manifest a;
        if (HttpGet(kManifestApiUrl, body, nullptr, e,
                    L"Accept: application/vnd.github.raw+json\r\nX-GitHub-Api-Version: 2022-11-28\r\nCache-Control: no-cache\r\n") &&
            ParseManifest(body, a)) {
            m = a;
            have = true;
        }
    }
    {
        std::string body;
        std::wstring e;
        Manifest b;
        std::wstring url = std::wstring(kManifestUrl) + L"?t=" + std::to_wstring(GetTickCount64());
        if (HttpGet(url, body, nullptr, e, L"Cache-Control: no-cache\r\nPragma: no-cache\r\n")) {
            if (ParseManifest(body, b)) {
                if (!have || CompareVersions(b.version, m.version) > 0) m = b;
                have = true;
            } else if (!have) {
                e = L"manifest.json could not be read";
            }
        }
        if (!have) { err = e; return false; }
    }
    std::lock_guard<std::mutex> lk(g_mx);
    g_manifest = m;
    g_manifestOk = true;
    return true;
}

static void InstallWorker(std::wstring backend, std::wstring langCode, bool hwAccel) {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    std::wstring err;
    bool ok = false;
    do {
        g_status = 0;
        g_progress = 0.02f;
        bool have;
        { std::lock_guard<std::mutex> lk(g_mx); have = g_manifestOk; }
        if (!have && !FetchManifest(err)) break;
        Manifest m;
        { std::lock_guard<std::mutex> lk(g_mx); m = g_manifest; }
        std::wstring asset = m.assets.count(backend) ? m.assets[backend] : (backend == L"rust" ? L"CSM-rust.exe" : L"CSM-cpp.exe");
        std::wstring base = kReleaseBase + m.version + L"/";

        std::string app, inst;
        if (!HttpGet(base + asset, app, [](double p) { g_progress = 0.05f + 0.75f * (float)p; }, err)) break;
        std::wstring instAsset = m.assets.count(L"installer") ? m.assets[L"installer"] : L"CSMInstaller.exe";
        std::wstring ignored;
        HttpGet(base + instAsset, inst, [](double p) { g_progress = 0.80f + 0.12f * (float)p; }, ignored);

        g_status = 1;
        g_progress = 0.93f;
        std::wstring dir = InstallDir();
        SHCreateDirectoryExW(nullptr, dir.c_str(), nullptr);
        StopApp();
        if (!WriteFileAtomic(AppExe(), app)) { err = L"Could not write " + AppExe(); break; }

        // Keep a copy of Setup next to the app (it checks for updates and uninstalls).
        std::wstring self = SelfPath(), dest = InstallerExe();
        if (!inst.empty() || _wcsicmp(self.c_str(), dest.c_str()) != 0) {
            if (_wcsicmp(self.c_str(), dest.c_str()) == 0) MoveFileExW(dest.c_str(), (dest + L".old").c_str(), MOVEFILE_REPLACE_EXISTING);
            if (!inst.empty()) WriteFileAtomic(dest, inst);
            else CopyFileW(self.c_str(), dest.c_str(), FALSE);
        }

        RegSetStr(HKEY_CURRENT_USER, kSettingsKey, L"InstalledVersion", m.version);
        RegSetStr(HKEY_CURRENT_USER, kSettingsKey, L"Backend", backend);
        RegSetStr(HKEY_CURRENT_USER, kSettingsKey, L"Language", langCode);
        RegSetDword(HKEY_CURRENT_USER, kSettingsKey, L"HardwareAcceleration", hwAccel ? 1 : 0);
        RegSetStr(HKEY_CURRENT_USER, kRunKey, L"CustomStartMenu", L"\"" + AppExe() + L"\"");

        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"DisplayName", L"Custom Start (CSM)");
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"DisplayVersion", m.version);
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"Publisher", L"CustomStartMenu");
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"DisplayIcon", AppExe());
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"InstallLocation", dir);
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"UninstallString", L"\"" + dest + L"\" --uninstall");
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"ModifyPath", L"\"" + dest + L"\"");
        RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"URLInfoAbout", L"https://github.com/mysisnotreal/CustomStartMenu");
        RegSetDword(HKEY_CURRENT_USER, kUninstallKey, L"NoRepair", 1);
        RegSetDword(HKEY_CURRENT_USER, kUninstallKey, L"EstimatedSize", (DWORD)((app.size() + inst.size()) / 1024));

        std::wstring programs = KnownFolder(FOLDERID_Programs);
        MakeShortcut(programs + L"\\Custom Start.lnk", AppExe(), L"", L"Custom Start command bar");
        MakeShortcut(programs + L"\\Custom Start Setup.lnk", dest, L"", L"Install, update or remove Custom Start");

        ShellExecuteW(nullptr, L"open", AppExe().c_str(), nullptr, dir.c_str(), SW_SHOWNORMAL);
        g_installedVersion = m.version;
        g_installedBackend = backend;
        g_progress = 1.f;
        ok = true;
    } while (false);
    if (!ok) g_errorText = err;
    CoUninitialize();
    PostMessageW(g_hwnd, WM_APP_INSTALLED, ok, 0);
}

static void UninstallWorker() {
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    StopApp();
    std::wstring dir = InstallDir();
    DeleteFileW(AppExe().c_str());
    DeleteFileW((InstallerExe() + L".old").c_str());
    {  // fonts added for Custom Start
        std::wstring fonts = dir + L"\\Fonts";
        WIN32_FIND_DATAW fd;
        HANDLE fh = FindFirstFileW((fonts + L"\\*").c_str(), &fd);
        if (fh != INVALID_HANDLE_VALUE) {
            do {
                if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) DeleteFileW((fonts + L"\\" + fd.cFileName).c_str());
            } while (FindNextFileW(fh, &fd));
            FindClose(fh);
        }
        RemoveDirectoryW(fonts.c_str());
        // the "> font" card pictures
        std::wstring previews = dir + L"\\Cache\\FontPreviews";
        fh = FindFirstFileW((previews + L"\\*").c_str(), &fd);
        if (fh != INVALID_HANDLE_VALUE) {
            do {
                if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) DeleteFileW((previews + L"\\" + fd.cFileName).c_str());
            } while (FindNextFileW(fh, &fd));
            FindClose(fh);
        }
        RemoveDirectoryW(previews.c_str());
        RemoveDirectoryW((dir + L"\\Cache").c_str());
    }
    std::wstring programs = KnownFolder(FOLDERID_Programs);
    DeleteFileW((programs + L"\\Custom Start.lnk").c_str());
    DeleteFileW((programs + L"\\Custom Start Setup.lnk").c_str());
    HKEY k;
    if (RegOpenKeyExW(HKEY_CURRENT_USER, kRunKey, 0, KEY_SET_VALUE, &k) == ERROR_SUCCESS) {
        RegDeleteValueW(k, L"CustomStartMenu");
        RegCloseKey(k);
    }
    RegDeleteTreeW(HKEY_CURRENT_USER, kSettingsKey);
    RegDeleteTreeW(HKEY_CURRENT_USER, kUninstallKey);
    CoUninitialize();
    PostMessageW(g_hwnd, WM_APP_UNINSTALLED, 0, 0);
}

static void ScheduleSelfDelete() {
    std::wstring self = SelfPath(), dest = InstallerExe();
    if (_wcsicmp(self.c_str(), dest.c_str()) != 0) {
        DeleteFileW(dest.c_str());
        RemoveDirectoryW(InstallDir().c_str());
        return;
    }
    std::wstring cmd = L"cmd.exe /c ping 127.0.0.1 -n 3 > nul & del /f /q \"" + dest + L"\" & rmdir \"" + InstallDir() + L"\"";
    STARTUPINFOW si = {sizeof(si)};
    PROCESS_INFORMATION pi = {};
    if (CreateProcessW(nullptr, &cmd[0], nullptr, nullptr, FALSE, CREATE_NO_WINDOW, nullptr, nullptr, &si, &pi)) {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
}

static bool g_checkOnly = false;

// The version of the installed app, read from the exe itself ("" if it can't be read).
static std::wstring ExeVersion(const std::wstring& path) {
    DWORD dummy = 0, n = GetFileVersionInfoSizeW(path.c_str(), &dummy);
    if (!n) return L"";
    std::vector<BYTE> buf(n);
    if (!GetFileVersionInfoW(path.c_str(), 0, n, buf.data())) return L"";
    VS_FIXEDFILEINFO* fi = nullptr;
    UINT len = 0;
    if (!VerQueryValueW(buf.data(), L"\\", (void**)&fi, &len) || !fi || !len) return L"";
    return std::to_wstring(HIWORD(fi->dwProductVersionMS)) + L"." + std::to_wstring(LOWORD(fi->dwProductVersionMS)) + L"." +
           std::to_wstring(HIWORD(fi->dwProductVersionLS));
}

static void StartManifestFetch() {
    std::thread([] {
        std::wstring err;
        bool ok = FetchManifest(err);
        if (!ok) { std::lock_guard<std::mutex> lk(g_mx); g_manifestErr = err; }
        PostMessageW(g_hwnd, WM_APP_MANIFEST, ok, 0);
    }).detach();
}

static void BeginInstall(bool update) {
    g_updateMode = update;
    g_progress = 0;
    g_progressShown = 0;
    GoTo(P_INSTALL);
    std::thread(InstallWorker, g_backend, std::wstring(kLangs[g_lang].code), g_hwAccel).detach();
}

static void OnClick(int id) {
    switch (id) {
    case B_CLOSE:
    case B_CLOSE2:
        if (g_page != P_INSTALL) DestroyWindow(g_hwnd);
        break;
    case B_MIN: ShowWindow(g_hwnd, SW_MINIMIZE); break;
    case B_CONTINUE:
        if (g_page == P_HELLO) GoTo(P_LANG);
        else if (g_page == P_LANG) GoTo(P_BACKEND);
        else if (g_page == P_BACKEND) GoTo(P_PERF);
        break;
    case B_BACK:
        if (g_page == P_LANG) GoTo(g_installedVersion.empty() ? P_HELLO : P_UPTODATE, -1);
        else if (g_page == P_BACKEND) GoTo(P_LANG, -1);
        else if (g_page == P_PERF) GoTo(P_BACKEND, -1);
        break;
    case B_RUST: g_backend = L"rust"; break;
    case B_CPP: g_backend = L"cpp"; break;
    case B_HWACCEL: g_hwAccel = !g_hwAccel; break;
    case B_INSTALL: BeginInstall(false); break;
    case B_UPDATE:
        if (!g_installedBackend.empty()) g_backend = g_installedBackend;
        BeginInstall(true);
        break;
    case B_NOTNOW: DestroyWindow(g_hwnd); break;
    case B_FINISH: DestroyWindow(g_hwnd); break;
    case B_MODIFY: GoTo(P_LANG); break;
    case B_UNINSTALL: GoTo(P_UNINSTALL); break;
    case B_CANCEL: GoTo(g_installedVersion.empty() ? P_HELLO : P_UPTODATE, -1); break;
    case B_UNINSTALL_GO:
        g_uninstalling = true;
        GoTo(P_LOADING);
        std::thread(UninstallWorker).detach();
        break;
    case B_RETRY:
        g_installFailed = false;
        if (g_installedVersion.empty()) BeginInstall(false);
        else { GoTo(P_LOADING); StartManifestFetch(); }
        break;
    default:
        if (id >= B_LANG0 && id < B_LANG0 + kLangCount) g_lang = id - B_LANG0;
    }
}

static void OnEnter() {
    switch (g_page) {
    case P_HELLO: if (Now() - g_helloStart > 3.5) OnClick(B_CONTINUE); break;
    case P_LANG: OnClick(B_CONTINUE); break;
    case P_BACKEND: OnClick(B_CONTINUE); break;
    case P_PERF: OnClick(B_INSTALL); break;
    case P_DONE: OnClick(B_FINISH); break;
    case P_UPDATE: OnClick(B_UPDATE); break;
    case P_UPTODATE: case P_UNINSTALLED: OnClick(B_CLOSE2); break;
    case P_UNINSTALL: OnClick(B_UNINSTALL_GO); break;
    case P_ERROR: OnClick(B_RETRY); break;
    default: break;
    }
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------
// A game or other fullscreen app is in front: an update prompt waits until it's gone.
static bool SomethingFullscreen() {
    QUERY_USER_NOTIFICATION_STATE qs;
    if (SUCCEEDED(SHQueryUserNotificationState(&qs)) &&
        (qs == QUNS_RUNNING_D3D_FULL_SCREEN || qs == QUNS_BUSY || qs == QUNS_PRESENTATION_MODE))
        return true;
    HWND fg = GetForegroundWindow();
    if (!fg || fg == g_hwnd || fg == GetShellWindow() || fg == GetDesktopWindow()) return false;
    wchar_t cls[64] = {};
    GetClassNameW(fg, cls, 64);
    if (!wcscmp(cls, L"Progman") || !wcscmp(cls, L"WorkerW") || !wcscmp(cls, L"Shell_TrayWnd")) return false;
    if ((GetWindowLongW(fg, GWL_STYLE) & WS_CAPTION) == WS_CAPTION) return false;
    RECT r;
    MONITORINFO mi = {sizeof(mi)};
    if (!GetWindowRect(fg, &r) || !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mi)) return false;
    return r.left <= mi.rcMonitor.left && r.top <= mi.rcMonitor.top && r.right >= mi.rcMonitor.right &&
           r.bottom >= mi.rcMonitor.bottom;
}
#define TIMER_SHOW_UPDATE 7

static void ApplyBackdrop(HWND h) {
    BOOL dark = TRUE;
    DwmSetWindowAttribute(h, 20 /*DWMWA_USE_IMMERSIVE_DARK_MODE*/, &dark, sizeof(dark));
    int corner = 2;  // DWMWCP_ROUND
    DwmSetWindowAttribute(h, 33 /*DWMWA_WINDOW_CORNER_PREFERENCE*/, &corner, sizeof(corner));
    int backdrop = 3;  // DWMSBT_TRANSIENTWINDOW (acrylic)
    g_acrylic = SUCCEEDED(DwmSetWindowAttribute(h, 38 /*DWMWA_SYSTEMBACKDROP_TYPE*/, &backdrop, sizeof(backdrop)));
    MARGINS m = {-1, -1, -1, -1};
    DwmExtendFrameIntoClientArea(h, &m);
}

static LRESULT CALLBACK WndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp) {
    switch (msg) {
    case WM_NCCALCSIZE:
        if (wp) return 0;  // client area covers the whole window (custom title bar)
        break;
    case WM_NCHITTEST: {
        LRESULT r = DefWindowProcW(h, msg, wp, lp);
        if (r != HTCLIENT) return r;
        POINT p = {GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
        ScreenToClient(h, &p);
        if (p.y * 96.f / g_dpi < Ui::TitleH && HitAt(p) == 0) return HTCAPTION;
        return HTCLIENT;
    }
    case WM_MOUSEMOVE: {
        g_mousePx = {GET_X_LPARAM(lp), GET_Y_LPARAM(lp)};
        g_hot = HitAt(g_mousePx);
        TRACKMOUSEEVENT tme = {sizeof(tme), TME_LEAVE, h, 0};
        TrackMouseEvent(&tme);
        return 0;
    }
    case WM_MOUSELEAVE: g_hot = 0; g_press = 0; return 0;
    case WM_MOUSEWHEEL:
        if (g_page == P_UPDATE) {  // scroll a long changelog
            float view = g_updateBox.bottom - g_updateBox.top - 8;
            float maxScroll = std::max(0.f, g_updateContentH - view);
            g_updateScroll = std::clamp(g_updateScroll - GET_WHEEL_DELTA_WPARAM(wp) / 120.f * 40.f, 0.f, maxScroll);
            InvalidateRect(h, nullptr, FALSE);
        }
        return 0;
    case WM_LBUTTONDOWN:
        g_press = HitAt({GET_X_LPARAM(lp), GET_Y_LPARAM(lp)});
        SetCapture(h);
        return 0;
    case WM_LBUTTONUP: {
        ReleaseCapture();
        int id = HitAt({GET_X_LPARAM(lp), GET_Y_LPARAM(lp)});
        int pressed = g_press;
        g_press = 0;
        if (id && id == pressed) OnClick(id);
        return 0;
    }
    case WM_SETCURSOR:
        if (LOWORD(lp) == HTCLIENT) { SetCursor(LoadCursorW(nullptr, g_hot ? IDC_HAND : IDC_ARROW)); return TRUE; }
        break;
    case WM_KEYDOWN:
        if (wp == VK_RETURN) OnEnter();
        else if (wp == VK_SPACE && g_page == P_PERF) OnClick(B_HWACCEL);
        else if (wp == VK_ESCAPE && g_page == P_UPDATE) OnClick(B_NOTNOW);
        return 0;
    case WM_SIZE:
        if (g_rt) g_rt->Resize(D2D1::SizeU(LOWORD(lp), HIWORD(lp)));
        return 0;
    case WM_DPICHANGED: {
        g_dpi = (float)HIWORD(wp);
        if (g_rt) g_rt->SetDpi(g_dpi, g_dpi);
        RECT* r = (RECT*)lp;
        SetWindowPos(h, nullptr, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
        return 0;
    }
    case WM_APP_MANIFEST: {
        if (g_uninstalling || g_page == P_UNINSTALL || g_page == P_INSTALL || g_page == P_HELLO || g_page == P_LANG || g_page == P_BACKEND ||
            g_page == P_PERF)
            return 0;  // first-run flow: the manifest is just cached for the install step
        bool ok = wp != 0;
        std::wstring latest;
        { std::lock_guard<std::mutex> lk(g_mx); latest = g_manifest.version; }
        if (!ok) {
            if (g_checkOnly) { DestroyWindow(h); return 0; }
            { std::lock_guard<std::mutex> lk(g_mx); g_errorText = g_manifestErr; }
            GoTo(P_ERROR);
        } else if (CompareVersions(latest, g_installedVersion) > 0) {
            GoTo(P_UPDATE);
            if (g_checkOnly) {
                if (SomethingFullscreen()) SetTimer(h, TIMER_SHOW_UPDATE, 30 * 1000, nullptr);
                else { ShowWindow(h, SW_SHOW); SetForegroundWindow(h); }
            }
        } else {
            if (g_checkOnly) { DestroyWindow(h); return 0; }
            GoTo(P_UPTODATE);
        }
        return 0;
    }
    case WM_TIMER:
        if (wp == TIMER_SHOW_UPDATE && !SomethingFullscreen()) {
            KillTimer(h, TIMER_SHOW_UPDATE);
            ShowWindow(h, SW_SHOW);
            SetForegroundWindow(h);
        }
        return 0;
    case WM_APP_INSTALLED:
        g_installFailed = wp == 0;
        if (wp) GoTo(P_DONE);
        else GoTo(P_ERROR);
        return 0;
    case WM_APP_UNINSTALLED:
        g_installedVersion.clear();
        ScheduleSelfDelete();
        GoTo(P_UNINSTALLED);
        return 0;
    case WM_CLOSE:
        if (g_page == P_INSTALL) return 0;
        DestroyWindow(h);
        return 0;
    case WM_DESTROY: PostQuitMessage(0); return 0;
    }
    return DefWindowProcW(h, msg, wp, lp);
}

int WINAPI wWinMain(HINSTANCE inst, HINSTANCE, PWSTR cmdLine, int) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    QueryPerformanceFrequency(&g_qpf);

    std::wstring args = cmdLine ? cmdLine : L"";
    g_checkOnly = args.find(L"--check-update") != std::wstring::npos;
    bool uninstall = args.find(L"--uninstall") != std::wstring::npos;

    HANDLE mutex = CreateMutexW(nullptr, TRUE, L"Local\\CSMInstaller.Singleton");
    if (GetLastError() == ERROR_ALREADY_EXISTS) return 0;

    DeleteFileW((InstallerExe() + L".old").c_str());
    g_installedVersion = RegStr(HKEY_CURRENT_USER, kSettingsKey, L"InstalledVersion");
    g_installedBackend = RegStr(HKEY_CURRENT_USER, kSettingsKey, L"Backend");
    if (GetFileAttributesW(AppExe().c_str()) == INVALID_FILE_ATTRIBUTES) g_installedVersion.clear();
    else if (!g_installedVersion.empty()) {
        // The exe is the truth: the registry can fall behind if the app was replaced some other way.
        std::wstring real = ExeVersion(AppExe());
        if (!real.empty() && real != g_installedVersion) {
            g_installedVersion = real;
            RegSetStr(HKEY_CURRENT_USER, kSettingsKey, L"InstalledVersion", real);
            RegSetStr(HKEY_CURRENT_USER, kUninstallKey, L"DisplayVersion", real);
        }
    }
    if (!g_installedBackend.empty()) g_backend = g_installedBackend;
    {
        DWORD v = 1, sz = sizeof(v);
        if (RegGetValueW(HKEY_CURRENT_USER, kSettingsKey, L"HardwareAcceleration", RRF_RT_REG_DWORD, nullptr, &v, &sz) == ERROR_SUCCESS)
            g_hwAccel = v != 0;
        g_hwAnim = g_hwAccel ? 1.f : 0.f;
    }
    std::wstring savedLang = RegStr(HKEY_CURRENT_USER, kSettingsKey, L"Language");
    g_lang = savedLang.empty() || LangIndex(savedLang) < 0 ? SystemLanguage() : LangIndex(savedLang);
    if (g_checkOnly && g_installedVersion.empty()) return 0;

    D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, &g_d2d);
    DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory), reinterpret_cast<IUnknown**>(&g_dw));
    if (!g_d2d || !g_dw) return 1;
    LoadCustomFont();
    g_fTitle = MakeFormat(28, DWRITE_FONT_WEIGHT_SEMI_BOLD);
    g_fSub = MakeFormat(15, DWRITE_FONT_WEIGHT_NORMAL);
    g_fBody = MakeFormat(14, DWRITE_FONT_WEIGHT_NORMAL);
    g_fBtn = MakeFormat(14, DWRITE_FONT_WEIGHT_SEMI_BOLD);
    g_fSmall = MakeFormat(12, DWRITE_FONT_WEIGHT_NORMAL);
    g_fBig = MakeFormat(24, DWRITE_FONT_WEIGHT_BOLD);
    {
        IDWriteFontCollection* c = nullptr;
        BOOL ex = FALSE;
        UINT32 idx;
        g_dw->GetSystemFontCollection(&c);
        if (c) { c->FindFamilyName(L"Segoe Fluent Icons", &idx, &ex); c->Release(); }
        g_fGlyph = MakeFormat(10, DWRITE_FONT_WEIGHT_NORMAL, ex ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets");
        IDWriteTextFormat* bigGlyph = MakeFormat(44, DWRITE_FONT_WEIGHT_NORMAL, ex ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets");
        g_fMono = bigGlyph;
        g_fIcon = MakeFormat(24, DWRITE_FONT_WEIGHT_NORMAL, ex ? L"Segoe Fluent Icons" : L"Segoe MDL2 Assets");
    }
    {
        g_fCardTitle = MakeFormat(18, DWRITE_FONT_WEIGHT_SEMI_BOLD);
    }

    WNDCLASSEXW wc = {sizeof(wc)};
    wc.lpfnWndProc = WndProc;
    wc.hInstance = inst;
    wc.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    wc.hIcon = LoadIconW(inst, MAKEINTRESOURCEW(1));
    wc.hIconSm = wc.hIcon;
    wc.lpszClassName = L"CSMInstaller.Window";
    RegisterClassExW(&wc);

    POINT pt;
    GetCursorPos(&pt);
    HMONITOR mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
    MONITORINFO mi = {sizeof(mi)};
    GetMonitorInfoW(mon, &mi);
    UINT dpi = GetDpiForSystem();
    {
        UINT dx = 96, dy = 96;
        typedef HRESULT(WINAPI * GetDpiForMonitorFn)(HMONITOR, int, UINT*, UINT*);
        if (HMODULE sh = LoadLibraryW(L"shcore.dll"))
            if (auto fn = (GetDpiForMonitorFn)GetProcAddress(sh, "GetDpiForMonitor"))
                if (SUCCEEDED(fn(mon, 0, &dx, &dy))) dpi = dx;
    }
    g_dpi = (float)dpi;
    int w = (int)(Ui::W * g_dpi / 96), hgt = (int)(Ui::H * g_dpi / 96);
    int x = (mi.rcWork.left + mi.rcWork.right - w) / 2, y = (mi.rcWork.top + mi.rcWork.bottom - hgt) / 2;
    g_hwnd = CreateWindowExW(WS_EX_APPWINDOW, wc.lpszClassName, L"Custom Start Setup",
                             WS_OVERLAPPED | WS_CAPTION | WS_MINIMIZEBOX, x, y, w, hgt, nullptr, nullptr, inst,
                             nullptr);
    if (!g_hwnd) return 1;
    ApplyBackdrop(g_hwnd);
    SetWindowPos(g_hwnd, nullptr, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER);

    if (uninstall) {
        GoTo(P_UNINSTALL);
    } else if (g_installedVersion.empty()) {
        GoTo(P_HELLO);
        StartManifestFetch();
    } else {
        GoTo(P_LOADING);
        StartManifestFetch();
    }
    g_prevPage = P_NONE;
    if (!g_checkOnly) {
        ShowWindow(g_hwnd, SW_SHOW);
        SetForegroundWindow(g_hwnd);
    }

    MSG msg;
    bool running = true;
    while (running) {
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) {
            if (msg.message == WM_QUIT) { running = false; break; }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if (!running) break;
        if (IsWindowVisible(g_hwnd) && !IsIconic(g_hwnd)) {
            Render();
            if (FAILED(DwmFlush())) Sleep(10);
        } else {
            WaitMessage();
        }
    }

    SafeRelease(g_helloGeo);
    SafeRelease(g_solid);
    SafeRelease(g_rt);
    for (auto* f : {g_fTitle, g_fSub, g_fBody, g_fBtn, g_fSmall, g_fGlyph, g_fBig, g_fMono, g_fIcon, g_fCardTitle})
        if (f) f->Release();
    SafeRelease(g_fontColl);
    SafeRelease(g_dw);
    SafeRelease(g_d2d);
    CoUninitialize();
    ReleaseMutex(mutex);
    CloseHandle(mutex);
    return 0;
}
