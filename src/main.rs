use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Window, WindowBuilder},
};
use url::Url;
use wry::{
    PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize as WryLogicalSize},
    http::{Request, Response, header},
};

const CHROME_HEIGHT: u32 = 82;
const CUSTOM_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const HOME_URL: &str = "zenith://assets/home";
const SETTINGS_URL: &str = "zenith://assets/settings";
const HISTORY_URL: &str = "zenith://assets/history";
const DOWNLOADS_URL: &str = "zenith://assets/downloads";

enum UserEvent {
    ChromeReady,
    NewTab {
        url: Option<String>,
        activate: bool,
    },
    SwitchTab(u32),
    CloseTab(Option<u32>),
    NavigateTab {
        tab_id: Option<u32>,
        url: String,
    },
    TabAction {
        tab_id: Option<u32>,
        action: BrowserAction,
    },
    OpenSettingsTab,
    OpenHistoryTab,
    OpenDownloadsTab,
    BookmarkActiveTab(Option<u32>),
    OpenAuthWindow(String),
    OpenBackgroundAuthSync(String),
    TabUrlChanged {
        tab_id: u32,
        url: String,
    },
    TabTitleChanged {
        tab_id: u32,
        title: String,
    },
    SettingsChanged {
        key: String,
        value: String,
    },
    ClearHistory,
    ClearDownloads,
    DownloadStarted {
        url: String,
        path: String,
    },
    DownloadCompleted {
        url: String,
        path: Option<String>,
        success: bool,
    },
    ToggleMenu,
}

#[derive(Clone, Copy)]
enum BrowserAction {
    Back,
    Forward,
    Reload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IpcMessage {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default)]
    tab_id: Option<u32>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    open: Option<bool>,
}

struct BrowserTab {
    id: u32,
    url: String,
    title: String,
    webview: WebView,
}

struct AuthWindow {
    window: Window,
    _webview: WebView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeTabState {
    id: u32,
    title: String,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeState {
    tabs: Vec<ChromeTabState>,
    active_id: Option<u32>,
}

#[derive(Clone, Serialize, Deserialize)]
struct RecentSite {
    url: String,
    title: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct BookmarkSite {
    url: String,
    title: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct DownloadEntry {
    url: String,
    path: String,
    status: String,
}

fn is_http_like_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn is_assets_url(url: &str) -> bool {
    url.starts_with("zenith://assets/") || url == "zenith://assets"
}

fn profile_directory() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".zenith").join("profile")
    } else {
        std::env::temp_dir().join("zenith-profile")
    }
}

fn recent_sites_path() -> PathBuf {
    profile_directory().join("recent-sites.json")
}

fn bookmarks_path() -> PathBuf {
    profile_directory().join("bookmarks.json")
}

fn downloads_path() -> PathBuf {
    profile_directory().join("downloads.json")
}

fn fallback_title_for_url(raw_url: &str) -> String {
    if raw_url.starts_with(SETTINGS_URL) {
        return "Settings".to_string();
    }
    if raw_url.starts_with(HISTORY_URL) {
        return "History".to_string();
    }
    if raw_url.starts_with(DOWNLOADS_URL) {
        return "Downloads".to_string();
    }
    if raw_url.starts_with(HOME_URL) {
        return "New Tab".to_string();
    }

    if let Ok(url) = Url::parse(raw_url)
        && let Some(host) = url.host_str()
    {
        let host = host.strip_prefix("www.").unwrap_or(host);
        if !host.is_empty() {
            return host.to_string();
        }
    }

    "Zenith".to_string()
}

fn resolved_tab_title(raw_title: &str, current_url: &str) -> String {
    let trimmed = raw_title.trim();
    if trimmed.is_empty() {
        return fallback_title_for_url(current_url);
    }

    let lower = trimmed.to_ascii_lowercase();
    let generic = matches!(
        lower.as_str(),
        "zenith" | "zenith browser" | "about:blank" | "new tab"
    );
    if generic && is_http_like_url(current_url) {
        return fallback_title_for_url(current_url);
    }

    trimmed.to_string()
}

fn normalize_user_input_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return HOME_URL.to_string();
    }

    if trimmed.starts_with("zenith://") || is_http_like_url(trimmed) {
        return trimmed.to_string();
    }

    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{trimmed}");
    }

    let q = utf8_percent_encode(trimmed, NON_ALPHANUMERIC).to_string();
    format!("https://www.google.com/search?igu=1&q={q}")
}

fn load_recent_sites(path: &PathBuf) -> Vec<RecentSite> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<RecentSite>>(&raw).unwrap_or_default()
}

fn load_bookmarks(path: &PathBuf) -> Vec<BookmarkSite> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<BookmarkSite>>(&raw).unwrap_or_default()
}

fn load_downloads(path: &PathBuf) -> Vec<DownloadEntry> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<DownloadEntry>>(&raw).unwrap_or_default()
}

fn save_recent_sites(path: &PathBuf, recent_sites: &[RecentSite]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(recent_sites) {
        let _ = fs::write(path, serialized);
    }
}

fn save_bookmarks(path: &PathBuf, bookmarks: &[BookmarkSite]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(bookmarks) {
        let _ = fs::write(path, serialized);
    }
}

fn save_downloads(path: &PathBuf, downloads: &[DownloadEntry]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(downloads) {
        let _ = fs::write(path, serialized);
    }
}

fn should_track_recent_site(raw_url: &str) -> bool {
    if !is_http_like_url(raw_url) {
        return false;
    }

    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }

    if host == "accounts.google.com" {
        return false;
    }

    true
}

fn upsert_recent_site(recent_sites: &mut Vec<RecentSite>, raw_url: &str, raw_title: &str) -> bool {
    if !should_track_recent_site(raw_url) {
        return false;
    }

    let title = resolved_tab_title(raw_title, raw_url);
    let mut changed = false;

    if let Some(existing) = recent_sites.iter().position(|s| s.url == raw_url) {
        let item = recent_sites.remove(existing);
        if existing != 0 || item.title != title {
            changed = true;
        }
    } else {
        changed = true;
    }

    recent_sites.insert(
        0,
        RecentSite {
            url: raw_url.to_string(),
            title,
        },
    );

    const MAX_RECENT_SITES: usize = 20;
    if recent_sites.len() > MAX_RECENT_SITES {
        recent_sites.truncate(MAX_RECENT_SITES);
        changed = true;
    }

    changed
}

fn upsert_bookmark(bookmarks: &mut Vec<BookmarkSite>, raw_url: &str, raw_title: &str) -> bool {
    if !should_track_recent_site(raw_url) {
        return false;
    }

    let title = resolved_tab_title(raw_title, raw_url);
    let mut changed = false;

    if let Some(existing) = bookmarks.iter().position(|s| s.url == raw_url) {
        let item = bookmarks.remove(existing);
        if existing != 0 || item.title != title {
            changed = true;
        }
    } else {
        changed = true;
    }

    bookmarks.insert(
        0,
        BookmarkSite {
            url: raw_url.to_string(),
            title,
        },
    );

    const MAX_BOOKMARKS: usize = 50;
    if bookmarks.len() > MAX_BOOKMARKS {
        bookmarks.truncate(MAX_BOOKMARKS);
        changed = true;
    }

    changed
}

fn record_download_started(downloads: &mut Vec<DownloadEntry>, url: &str, path: &str) {
    if url.trim().is_empty() {
        return;
    }

    if let Some(existing) = downloads.iter().position(|d| d.url == url) {
        downloads.remove(existing);
    }

    downloads.insert(
        0,
        DownloadEntry {
            url: url.to_string(),
            path: path.to_string(),
            status: "in_progress".to_string(),
        },
    );

    const MAX_DOWNLOADS: usize = 80;
    if downloads.len() > MAX_DOWNLOADS {
        downloads.truncate(MAX_DOWNLOADS);
    }
}

fn record_download_completed(
    downloads: &mut Vec<DownloadEntry>,
    url: &str,
    path: Option<String>,
    success: bool,
) {
    let status = if success { "completed" } else { "failed" }.to_string();
    let final_path = path.unwrap_or_default();

    if let Some(existing) = downloads.iter_mut().find(|d| d.url == url) {
        existing.status = status;
        if !final_path.is_empty() {
            existing.path = final_path;
        }
        return;
    }

    downloads.insert(
        0,
        DownloadEntry {
            url: url.to_string(),
            path: final_path,
            status,
        },
    );

    const MAX_DOWNLOADS: usize = 80;
    if downloads.len() > MAX_DOWNLOADS {
        downloads.truncate(MAX_DOWNLOADS);
    }
}

fn is_auth_host(host: &str) -> bool {
    matches!(
        host,
        "accounts.google.com"
            | "oauth2.googleapis.com"
            | "github.com"
            | "gitlab.com"
            | "bitbucket.org"
            | "login.live.com"
            | "account.microsoft.com"
            | "login.microsoftonline.com"
            | "appleid.apple.com"
            | "id.twitch.tv"
            | "discord.com"
            | "slack.com"
            | "auth.openai.com"
            | "www.facebook.com"
            | "m.facebook.com"
            | "www.instagram.com"
            | "x.com"
            | "twitter.com"
            | "www.linkedin.com"
    )
}

fn has_auth_markers(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or_default().to_ascii_lowercase();
    let combined = format!("{path}?{query}");

    [
        "oauth",
        "authorize",
        "signin",
        "login",
        "consent",
        "accountchooser",
        "servicelogin",
        "sso",
        "2fa",
        "mfa",
        "challenge",
        "checkpoint",
    ]
    .iter()
    .any(|m| combined.contains(m))
}

fn looks_like_oauth_exchange(url: &Url) -> bool {
    let query = url.query().unwrap_or_default().to_ascii_lowercase();
    (query.contains("client_id=") || query.contains("appid=") || query.contains("scope="))
        && (query.contains("redirect_uri=")
            || query.contains("response_type=")
            || query.contains("code_challenge="))
}

fn should_open_auth_window(raw_url: &str) -> bool {
    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };

    let scheme = url.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return false;
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }

    if host == "accounts.google.com" {
        let path = url.path().to_ascii_lowercase();
        if path.contains("rotatecookiespage")
            || path.contains("checkcookie")
            || path.contains("listaccounts")
        {
            return false;
        }
    }

    let auth_like_host = is_auth_host(&host)
        || host.starts_with("accounts.")
        || host.starts_with("auth.")
        || host.starts_with("login.")
        || host.contains("oauth");
    let oauth_exchange = looks_like_oauth_exchange(&url);
    if oauth_exchange {
        return true;
    }

    if !auth_like_host {
        return false;
    }

    let auth_markers = has_auth_markers(&url);
    if auth_markers {
        return true;
    }

    let path = url.path().to_ascii_lowercase();
    matches!(
        path.as_str(),
        "/signin" | "/login" | "/oauth" | "/authorize"
    )
}

fn is_background_google_account_sync_url(raw_url: &str) -> bool {
    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "accounts.google.com" {
        return false;
    }

    let path = url.path().to_ascii_lowercase();
    path.contains("rotatecookiespage")
        || path.contains("checkcookie")
        || path.contains("listaccounts")
}

fn should_warmup_youtube_account_sync(raw_url: &str) -> bool {
    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "youtube.com" && host != "www.youtube.com" && host != "m.youtube.com" {
        return false;
    }

    let path = url.path().trim_end_matches('/').to_ascii_lowercase();
    path.is_empty()
}

fn chrome_bounds_for_window(window: &Window) -> Rect {
    let size = window.inner_size().to_logical::<u32>(window.scale_factor());
    let height = CHROME_HEIGHT.min(size.height.max(1));
    Rect {
        position: LogicalPosition::new(0, 0).into(),
        size: WryLogicalSize::new(size.width.max(1), height).into(),
    }
}

fn content_bounds_for_window(window: &Window) -> Rect {
    let size = window.inner_size().to_logical::<u32>(window.scale_factor());
    let y = CHROME_HEIGHT.min(size.height);
    let height = size.height.saturating_sub(y).max(1);
    Rect {
        position: LogicalPosition::new(0, y).into(),
        size: WryLogicalSize::new(size.width.max(1), height).into(),
    }
}

fn handle_zenith_request(ui_html: &str, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = request.uri();
    let host = uri.host().unwrap_or_default();
    let path = uri.path();

    if host == "assets" && (path == "/ui" || path == "/ui/") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(ui_html.as_bytes().to_vec()))
            .unwrap();
    }

    if host == "assets" && (path == "/home" || path == "/home/") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(include_bytes!("ui/home.html").to_vec()))
            .unwrap();
    }

    if host == "assets" && (path == "/settings" || path == "/settings/") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(include_bytes!("ui/settings.html").to_vec()))
            .unwrap();
    }

    if host == "assets" && (path == "/history" || path == "/history/") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(include_bytes!("ui/history.html").to_vec()))
            .unwrap();
    }

    if host == "assets" && (path == "/downloads" || path == "/downloads/") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Owned(include_bytes!("ui/downloads.html").to_vec()))
            .unwrap();
    }

    Response::builder()
        .status(404)
        .body(Cow::Borrowed(&[][..]))
        .unwrap()
}

fn tab_initialization_script(tab_id: u32) -> String {
    format!(
        r#"
        (function() {{
            window.__ZENITH_TAB_ID = {tab_id};
            var send = function(payload) {{
                try {{ window.ipc.postMessage(JSON.stringify(payload)); }} catch (_) {{}}
            }};
            var notifyUrl = function() {{
                send({{ type: 'tab_url_update', tabId: {tab_id}, url: window.location.href }});
            }};

            var wrapHistoryMethod = function(name) {{
                try {{
                    var original = history[name];
                    if (!original) return;
                    history[name] = function() {{
                        var result = original.apply(history, arguments);
                        notifyUrl();
                        return result;
                    }};
                }} catch (_) {{}}
            }};

            wrapHistoryMethod('pushState');
            wrapHistoryMethod('replaceState');
            window.addEventListener('popstate', notifyUrl);
            window.addEventListener('hashchange', notifyUrl);

            var applyYoutubeTheme = function(theme) {{
                try {{
                    var host = (window.location.hostname || '').toLowerCase();
                    if (!(host === 'youtube.com' || host === 'www.youtube.com' || host === 'm.youtube.com')) return;
                    var dark = theme === 'dark';
                    var apply = function() {{
                        var html = document.documentElement;
                        if (html) {{
                            if (dark) html.setAttribute('dark', '');
                            else html.removeAttribute('dark');
                            html.style.colorScheme = dark ? 'dark' : 'light';
                        }}
                        if (document.body) {{
                            document.body.classList.toggle('dark-theme', dark);
                            document.body.classList.toggle('light-theme', !dark);
                        }}
                        var app = document.querySelector('ytd-app');
                        if (app) {{
                            if (dark) {{
                                app.setAttribute('dark', '');
                                app.setAttribute('dark-theme', '');
                            }} else {{
                                app.removeAttribute('dark');
                                app.removeAttribute('dark-theme');
                            }}
                        }}
                    }};
                    apply();
                    setTimeout(apply, 120);
                    setTimeout(apply, 450);
                }} catch (_) {{}}
            }};

            window.__zenithApplyBrowserTheme = function(theme) {{
                try {{
                    window.__ZENITH_THEME = theme === 'light' ? 'light' : 'dark';
                    applyYoutubeTheme(window.__ZENITH_THEME);
                }} catch (_) {{}}
            }};

            window.addEventListener('yt-navigate-finish', function() {{
                try {{
                    if (window.__ZENITH_THEME) applyYoutubeTheme(window.__ZENITH_THEME);
                }} catch (_) {{}}
            }});

            var ensureYoutubeAccountSync = function() {{
                try {{
                    var host = (window.location.hostname || '').toLowerCase();
                    if (!(host === 'youtube.com' || host === 'www.youtube.com' || host === 'm.youtube.com')) return;
                    var path = (window.location.pathname || '').replace(/\/+$/, '');
                    if (path !== '' && path !== '/') return;
                    var storageKey = '__zenith_yt_home_sync_reload_done';
                    if (sessionStorage.getItem(storageKey) === '1') return;

                    setTimeout(function() {{
                        try {{
                            var hasSignIn = !!document.querySelector("a[href*='ServiceLogin']");
                            var hasAvatarButton = !!document.querySelector(
                                "ytd-topbar-menu-button-renderer #avatar-btn, button#avatar-btn"
                            );
                            if (hasSignIn && !hasAvatarButton) {{
                                sessionStorage.setItem(storageKey, '1');
                                window.location.reload();
                            }}
                        }} catch (_) {{}}
                    }}, 1400);
                }} catch (_) {{}}
            }};

            window.addEventListener('yt-navigate-finish', ensureYoutubeAccountSync);

            if (document.readyState === 'loading') {{
                document.addEventListener('DOMContentLoaded', function() {{
                    notifyUrl();
                    ensureYoutubeAccountSync();
                }}, {{ once: true }});
            }} else {{
                notifyUrl();
                ensureYoutubeAccountSync();
            }}

            window.addEventListener('keydown', function(e) {{
                var isPrimary = e.metaKey || e.ctrlKey;
                if (!isPrimary) return;

                var key = (e.key || '').toLowerCase();
                if (key === 't') {{
                    e.preventDefault();
                    send({{ type: 'new_tab' }});
                }} else if (key === 'w') {{
                    e.preventDefault();
                    send({{ type: 'close_tab', tabId: {tab_id} }});
                }} else if (key === 'r') {{
                    e.preventDefault();
                    send({{ type: 'tab_action', tabId: {tab_id}, action: 'reload' }});
                }} else if (key === 'd') {{
                    e.preventDefault();
                    send({{ type: 'bookmark_active_tab', tabId: {tab_id} }});
                }} else if (key === 'y') {{
                    e.preventDefault();
                    send({{ type: 'open_history_tab' }});
                }} else if (key === 'j') {{
                    e.preventDefault();
                    send({{ type: 'open_downloads_tab' }});
                }} else if (key === ',') {{
                    e.preventDefault();
                    send({{ type: 'open_settings_tab' }});
                }}
            }});
        }})();
        "#
    )
}

fn apply_browser_theme_to_tab(tab: &BrowserTab, theme: &str) {
    let normalized = if theme.eq_ignore_ascii_case("light") {
        "light"
    } else {
        "dark"
    };
    if let Ok(theme_json) = serde_json::to_string(normalized) {
        let js = format!(
            "if(window.__zenithApplyBrowserTheme) window.__zenithApplyBrowserTheme({theme_json});"
        );
        let _ = tab.webview.evaluate_script(&js);
    }
}

fn sync_recent_sites_to_tab(tab: &BrowserTab, recent_sites: &[RecentSite]) {
    if !tab.url.starts_with(HOME_URL) {
        return;
    }

    if let Ok(sites_json) = serde_json::to_string(recent_sites) {
        let js =
            format!("window.postMessage({{ type: 'recent-sites', sites: {sites_json} }}, '*');");
        let _ = tab.webview.evaluate_script(&js);
    }
}

fn sync_bookmarks_to_tab(tab: &BrowserTab, bookmarks: &[BookmarkSite]) {
    if !tab.url.starts_with(HOME_URL) {
        return;
    }

    if let Ok(bookmarks_json) = serde_json::to_string(bookmarks) {
        let js = format!(
            "window.postMessage({{ type: 'bookmarks-data', bookmarks: {bookmarks_json} }}, '*');"
        );
        let _ = tab.webview.evaluate_script(&js);
    }
}

fn sync_history_to_tab(tab: &BrowserTab, recent_sites: &[RecentSite]) {
    if !tab.url.starts_with(HISTORY_URL) {
        return;
    }

    if let Ok(history_json) = serde_json::to_string(recent_sites) {
        let js = format!(
            "window.postMessage({{ type: 'history-data', entries: {history_json} }}, '*');"
        );
        let _ = tab.webview.evaluate_script(&js);
    }
}

fn sync_downloads_to_tab(tab: &BrowserTab, downloads: &[DownloadEntry]) {
    if !tab.url.starts_with(DOWNLOADS_URL) {
        return;
    }

    if let Ok(downloads_json) = serde_json::to_string(downloads) {
        let js = format!(
            "window.postMessage({{ type: 'downloads-data', entries: {downloads_json} }}, '*');"
        );
        let _ = tab.webview.evaluate_script(&js);
    }
}

fn apply_tab_visibility(tabs: &[BrowserTab], active_tab_id: Option<u32>) {
    for tab in tabs {
        let _ = tab.webview.set_visible(Some(tab.id) == active_tab_id);
    }
}

fn apply_tab_bounds(tabs: &[BrowserTab], bounds: Rect) {
    for tab in tabs {
        let _ = tab.webview.set_bounds(bounds);
    }
}

fn sync_chrome_state(chrome: &WebView, tabs: &[BrowserTab], active_tab_id: Option<u32>) {
    let state = ChromeState {
        tabs: tabs
            .iter()
            .map(|t| ChromeTabState {
                id: t.id,
                title: t.title.clone(),
                url: t.url.clone(),
            })
            .collect(),
        active_id: active_tab_id,
    };

    if let Ok(json) = serde_json::to_string(&state) {
        let js = format!("if(window.zenithSetState) window.zenithSetState({json});");
        let _ = chrome.evaluate_script(&js);
    }
}

fn dispatch_ipc_message(
    raw: &str,
    proxy: &EventLoopProxy<UserEvent>,
    fallback_tab_id: Option<u32>,
) {
    let Ok(message) = serde_json::from_str::<IpcMessage>(raw) else {
        return;
    };

    let tab_id = message.tab_id.or(fallback_tab_id);

    match message.message_type.as_str() {
        "chrome_ready" => {
            let _ = proxy.send_event(UserEvent::ChromeReady);
        }
        "new_tab" => {
            let _ = proxy.send_event(UserEvent::NewTab {
                url: message.url,
                activate: true,
            });
        }
        "switch_tab" => {
            if let Some(id) = tab_id {
                let _ = proxy.send_event(UserEvent::SwitchTab(id));
            }
        }
        "close_tab" => {
            let _ = proxy.send_event(UserEvent::CloseTab(tab_id));
        }
        "navigate" => {
            if let Some(url) = message.url {
                let _ = proxy.send_event(UserEvent::NavigateTab { tab_id, url });
            }
        }
        "tab_action" => {
            let action = match message.action.as_deref() {
                Some("back") => Some(BrowserAction::Back),
                Some("forward") => Some(BrowserAction::Forward),
                Some("reload") => Some(BrowserAction::Reload),
                _ => None,
            };
            if let Some(action) = action {
                let _ = proxy.send_event(UserEvent::TabAction { tab_id, action });
            }
        }
        "open_settings_tab" => {
            let _ = proxy.send_event(UserEvent::OpenSettingsTab);
        }
        "open_history_tab" => {
            let _ = proxy.send_event(UserEvent::OpenHistoryTab);
        }
        "open_downloads_tab" => {
            let _ = proxy.send_event(UserEvent::OpenDownloadsTab);
        }
        "bookmark_active_tab" => {
            let _ = proxy.send_event(UserEvent::BookmarkActiveTab(tab_id));
        }
        "open_auth" => {
            if fallback_tab_id.is_none()
                && let Some(url) = message.url
            {
                let _ = proxy.send_event(UserEvent::OpenAuthWindow(url));
            }
        }
        "tab_url_update" => {
            if let (Some(id), Some(url)) = (tab_id, message.url) {
                let _ = proxy.send_event(UserEvent::TabUrlChanged { tab_id: id, url });
            }
        }
        "settings-change" | "settings_change" => {
            if let (Some(key), Some(value)) = (message.key, message.value) {
                let _ = proxy.send_event(UserEvent::SettingsChanged { key, value });
            }
        }
        "settings-action" => {
            if message.action.as_deref() == Some("reset") {
                let _ = proxy.send_event(UserEvent::SettingsChanged {
                    key: "theme".to_string(),
                    value: "dark".to_string(),
                });
                let _ = proxy.send_event(UserEvent::SettingsChanged {
                    key: "searchEngine".to_string(),
                    value: "google".to_string(),
                });
            }
        }
        "clear_history" => {
            let _ = proxy.send_event(UserEvent::ClearHistory);
        }
        "clear_downloads" => {
            let _ = proxy.send_event(UserEvent::ClearDownloads);
        }
        "toggle_menu" => {
            let _ = proxy.send_event(UserEvent::ToggleMenu);
        }
        _ => {}
    }
}

fn build_browser_tab(
    window: &Window,
    web_context: &mut WebContext,
    tab_id: u32,
    url: &str,
    bounds: Rect,
    proxy: &EventLoopProxy<UserEvent>,
    ui_html: Arc<String>,
) -> Option<BrowserTab> {
    let popup_proxy = proxy.clone();
    let title_proxy = proxy.clone();
    let load_proxy = proxy.clone();
    let ipc_proxy = proxy.clone();
    let download_start_proxy = proxy.clone();
    let download_complete_proxy = proxy.clone();
    let protocol_html = ui_html;
    let init_script = tab_initialization_script(tab_id);

    let webview = WebViewBuilder::new_with_web_context(web_context)
        .with_user_agent(CUSTOM_USER_AGENT)
        .with_bounds(bounds)
        .with_url(url)
        .with_initialization_script(&init_script)
        .with_navigation_handler(move |next| {
            if next.starts_with("zenith://") {
                return is_assets_url(&next);
            }

            is_http_like_url(&next)
        })
        .with_new_window_req_handler(move |next, _features| {
            if is_background_google_account_sync_url(&next) {
                let _ = popup_proxy.send_event(UserEvent::OpenBackgroundAuthSync(next));
            } else if should_open_auth_window(&next) {
                let _ = popup_proxy.send_event(UserEvent::NavigateTab {
                    tab_id: Some(tab_id),
                    url: next,
                });
            } else if is_http_like_url(&next) {
                let _ = popup_proxy.send_event(UserEvent::NewTab {
                    url: Some(next),
                    activate: true,
                });
            }
            wry::NewWindowResponse::Deny
        })
        .with_document_title_changed_handler(move |title| {
            let _ = title_proxy.send_event(UserEvent::TabTitleChanged { tab_id, title });
        })
        .with_on_page_load_handler(move |_event: PageLoadEvent, url| {
            let _ = load_proxy.send_event(UserEvent::TabUrlChanged { tab_id, url });
        })
        .with_download_started_handler(move |url, path| {
            let _ = download_start_proxy.send_event(UserEvent::DownloadStarted {
                url,
                path: path.to_string_lossy().to_string(),
            });
            true
        })
        .with_download_completed_handler(move |url, path, success| {
            let _ = download_complete_proxy.send_event(UserEvent::DownloadCompleted {
                url,
                path: path.map(|p| p.to_string_lossy().to_string()),
                success,
            });
        })
        .with_ipc_handler(move |request: Request<String>| {
            dispatch_ipc_message(request.body(), &ipc_proxy, Some(tab_id));
        })
        .with_custom_protocol("zenith".into(), move |_id, request: Request<Vec<u8>>| {
            handle_zenith_request(protocol_html.as_str(), request)
        })
        .build_as_child(window)
        .ok()?;

    Some(BrowserTab {
        id: tab_id,
        url: url.to_string(),
        title: fallback_title_for_url(url),
        webview,
    })
}

#[cfg(target_os = "macos")]
fn setup_macos_menu() {
    use muda::{Menu, Submenu, PredefinedMenuItem};
    let menu = Menu::new();

    let app_m = Submenu::new("Zenith", true);
    menu.append(&app_m).unwrap();
    app_m.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]).unwrap();

    let edit_m = Submenu::new("Edit", true);
    menu.append(&edit_m).unwrap();
    edit_m.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]).unwrap();

    let _ = menu.init_for_nsapp();
}

fn main() {
    #[cfg(target_os = "macos")]
    setup_macos_menu();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let profile_dir = profile_directory();
    let mut web_context = WebContext::new(Some(profile_dir.join("webview")));
    let recent_sites_path = recent_sites_path();
    let bookmarks_path = bookmarks_path();
    let downloads_path = downloads_path();

    let window = WindowBuilder::new()
        .with_title("Zenith")
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .build(&event_loop)
        .unwrap();

    let ui_html = include_str!("ui/ui.html");
    let ui_css = include_str!("ui/ui.css");
    let final_ui_html = Arc::new(ui_html.replace(
        "<link rel=\"stylesheet\" href=\"ui.css\">",
        &format!("<style>{}</style>", ui_css),
    ));

    let chrome_proxy = proxy.clone();
    let chrome_protocol_html = final_ui_html.clone();
    let chrome_webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_bounds(chrome_bounds_for_window(&window))
        .with_url("zenith://assets/ui")
        .with_navigation_handler(|url| is_assets_url(&url))
        .with_custom_protocol("zenith".into(), move |_id, request: Request<Vec<u8>>| {
            handle_zenith_request(chrome_protocol_html.as_str(), request)
        })
        .with_ipc_handler(move |request: Request<String>| {
            dispatch_ipc_message(request.body(), &chrome_proxy, None);
        })
        .build_as_child(&window)
        .unwrap();

    let mut tabs: Vec<BrowserTab> = Vec::new();
    let mut next_tab_id: u32 = 1;
    let mut active_tab_id: Option<u32> = None;
    let mut chrome_ready = false;
    let mut current_theme = "dark".to_string();
    let mut auth_windows: Vec<AuthWindow> = Vec::new();
    let mut background_sync_webview: Option<WebView> = None;
    let mut recent_sites = load_recent_sites(&recent_sites_path);
    let mut bookmarks = load_bookmarks(&bookmarks_path);
    let mut downloads = load_downloads(&downloads_path);

    if let Some(initial_tab) = build_browser_tab(
        &window,
        &mut web_context,
        next_tab_id,
        HOME_URL,
        content_bounds_for_window(&window),
        &proxy,
        final_ui_html.clone(),
    ) {
        apply_browser_theme_to_tab(&initial_tab, &current_theme);
        active_tab_id = Some(next_tab_id);
        next_tab_id += 1;
        tabs.push(initial_tab);
        apply_tab_visibility(&tabs, active_tab_id);
    }

    event_loop.run(move |event, event_loop_target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep_context_alive = &web_context;

        match event {
            Event::UserEvent(UserEvent::ChromeReady) => {
                chrome_ready = true;
                sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                for tab in &tabs {
                    sync_recent_sites_to_tab(tab, &recent_sites);
                    sync_bookmarks_to_tab(tab, &bookmarks);
                    sync_history_to_tab(tab, &recent_sites);
                    sync_downloads_to_tab(tab, &downloads);
                }
            }
            Event::UserEvent(UserEvent::NewTab { url, activate }) => {
                let start_url = normalize_user_input_url(url.as_deref().unwrap_or(HOME_URL));

                if let Some(tab) = build_browser_tab(
                    &window,
                    &mut web_context,
                    next_tab_id,
                    &start_url,
                    content_bounds_for_window(&window),
                    &proxy,
                    final_ui_html.clone(),
                ) {
                    apply_browser_theme_to_tab(&tab, &current_theme);
                    tabs.push(tab);
                    if activate || active_tab_id.is_none() {
                        active_tab_id = Some(next_tab_id);
                    }
                    if let Some(new_tab) = tabs.last() {
                        sync_recent_sites_to_tab(new_tab, &recent_sites);
                        sync_bookmarks_to_tab(new_tab, &bookmarks);
                        sync_history_to_tab(new_tab, &recent_sites);
                        sync_downloads_to_tab(new_tab, &downloads);
                    }
                    next_tab_id += 1;
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::SwitchTab(tab_id)) => {
                if tabs.iter().any(|t| t.id == tab_id) {
                    active_tab_id = Some(tab_id);
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::CloseTab(tab_id)) => {
                if let Some(close_id) = tab_id.or(active_tab_id) {
                    if tabs.len() <= 1 {
                        if let Some(tab) = tabs.first_mut() {
                            tab.url = HOME_URL.to_string();
                            tab.title = fallback_title_for_url(HOME_URL);
                            let _ = tab.webview.load_url(HOME_URL);
                            active_tab_id = Some(tab.id);
                        }
                    } else if let Some(idx) = tabs.iter().position(|t| t.id == close_id) {
                        tabs.remove(idx);
                        if active_tab_id == Some(close_id) {
                            let next_idx = idx.saturating_sub(1);
                            active_tab_id = tabs.get(next_idx).map(|t| t.id);
                        }
                    }

                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::NavigateTab { tab_id, url }) => {
                if let Some(target_id) = tab_id.or(active_tab_id) {
                    let next_url = normalize_user_input_url(&url);
                    if !(next_url.starts_with("zenith://") && !is_assets_url(&next_url)) {
                        if should_warmup_youtube_account_sync(&next_url) {
                            let _ = proxy.send_event(UserEvent::OpenBackgroundAuthSync(
                                "https://accounts.google.com/RotateCookiesPage".to_string(),
                            ));
                        }
                        if let Some(tab) = tabs.iter_mut().find(|t| t.id == target_id) {
                            tab.url = next_url.clone();
                            tab.title = fallback_title_for_url(&next_url);
                            let _ = tab.webview.load_url(&next_url);
                            if chrome_ready {
                                sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                            }
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::TabAction { tab_id, action }) => {
                if let Some(target_id) = tab_id.or(active_tab_id) {
                    if let Some(tab) = tabs.iter().find(|t| t.id == target_id) {
                        match action {
                            BrowserAction::Back => {
                                let _ = tab.webview.evaluate_script("history.back();");
                            }
                            BrowserAction::Forward => {
                                let _ = tab.webview.evaluate_script("history.forward();");
                            }
                            BrowserAction::Reload => {
                                let _ = tab.webview.reload();
                            }
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenSettingsTab) => {
                if let Some(existing_id) = tabs
                    .iter()
                    .find(|t| t.url.starts_with(SETTINGS_URL))
                    .map(|t| t.id)
                {
                    active_tab_id = Some(existing_id);
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                } else {
                    let _ = proxy.send_event(UserEvent::NewTab {
                        url: Some(SETTINGS_URL.to_string()),
                        activate: true,
                    });
                }
            }
            Event::UserEvent(UserEvent::OpenHistoryTab) => {
                if let Some(existing_id) = tabs
                    .iter()
                    .find(|t| t.url.starts_with(HISTORY_URL))
                    .map(|t| t.id)
                {
                    active_tab_id = Some(existing_id);
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                } else {
                    let _ = proxy.send_event(UserEvent::NewTab {
                        url: Some(HISTORY_URL.to_string()),
                        activate: true,
                    });
                }
            }
            Event::UserEvent(UserEvent::OpenDownloadsTab) => {
                if let Some(existing_id) = tabs
                    .iter()
                    .find(|t| t.url.starts_with(DOWNLOADS_URL))
                    .map(|t| t.id)
                {
                    active_tab_id = Some(existing_id);
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                } else {
                    let _ = proxy.send_event(UserEvent::NewTab {
                        url: Some(DOWNLOADS_URL.to_string()),
                        activate: true,
                    });
                }
            }
            Event::UserEvent(UserEvent::BookmarkActiveTab(tab_id)) => {
                if let Some(target_id) = tab_id.or(active_tab_id)
                    && let Some(tab) = tabs.iter().find(|t| t.id == target_id)
                {
                    let bookmark_url = tab.url.clone();
                    let bookmark_title = tab.title.clone();
                    if upsert_bookmark(&mut bookmarks, &bookmark_url, &bookmark_title) {
                        save_bookmarks(&bookmarks_path, &bookmarks);
                        for t in &tabs {
                            sync_bookmarks_to_tab(t, &bookmarks);
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::DownloadStarted { url, path }) => {
                record_download_started(&mut downloads, &url, &path);
                save_downloads(&downloads_path, &downloads);
                for tab in &tabs {
                    sync_downloads_to_tab(tab, &downloads);
                }
            }
            Event::UserEvent(UserEvent::DownloadCompleted { url, path, success }) => {
                record_download_completed(&mut downloads, &url, path, success);
                save_downloads(&downloads_path, &downloads);
                for tab in &tabs {
                    sync_downloads_to_tab(tab, &downloads);
                }
            }
            Event::UserEvent(UserEvent::ClearHistory) => {
                if !recent_sites.is_empty() {
                    recent_sites.clear();
                    save_recent_sites(&recent_sites_path, &recent_sites);
                    for tab in &tabs {
                        sync_recent_sites_to_tab(tab, &recent_sites);
                        sync_history_to_tab(tab, &recent_sites);
                    }
                }
            }
            Event::UserEvent(UserEvent::ClearDownloads) => {
                if !downloads.is_empty() {
                    downloads.clear();
                    save_downloads(&downloads_path, &downloads);
                    for tab in &tabs {
                        sync_downloads_to_tab(tab, &downloads);
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenAuthWindow(url)) => {
                if is_http_like_url(&url) {
                    let popup_proxy = proxy.clone();
                    if let Ok(auth_window) = WindowBuilder::new()
                        .with_title("Zenith Sign In")
                        .with_inner_size(LogicalSize::new(980.0, 760.0))
                        .build(event_loop_target)
                        && let Ok(auth_webview) =
                            WebViewBuilder::new_with_web_context(&mut web_context)
                                .with_user_agent(CUSTOM_USER_AGENT)
                                .with_url(&url)
                                .with_navigation_handler(|next| {
                                    is_http_like_url(&next) || is_assets_url(&next)
                                })
                                .with_new_window_req_handler(move |next, _| {
                                    if should_open_auth_window(&next) {
                                        let _ =
                                            popup_proxy.send_event(UserEvent::OpenAuthWindow(next));
                                    }
                                    wry::NewWindowResponse::Deny
                                })
                                .build(&auth_window)
                    {
                        auth_windows.push(AuthWindow {
                            window: auth_window,
                            _webview: auth_webview,
                        });
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenBackgroundAuthSync(url)) => {
                if is_http_like_url(&url) {
                    if let Some(bg) = background_sync_webview.as_ref() {
                        let _ = bg.load_url(&url);
                    } else {
                        let sync_proxy = proxy.clone();
                        let hidden_bounds = Rect {
                            position: LogicalPosition::new(0, 0).into(),
                            size: WryLogicalSize::new(1, 1).into(),
                        };
                        if let Ok(bg_webview) = WebViewBuilder::new_with_web_context(&mut web_context)
                            .with_user_agent(CUSTOM_USER_AGENT)
                            .with_bounds(hidden_bounds)
                            .with_url(&url)
                            .with_navigation_handler(|next| {
                                is_http_like_url(&next) || is_assets_url(&next)
                            })
                            .with_new_window_req_handler(move |next, _| {
                                if is_background_google_account_sync_url(&next) {
                                    let _ = sync_proxy
                                        .send_event(UserEvent::OpenBackgroundAuthSync(next));
                                }
                                wry::NewWindowResponse::Deny
                            })
                            .build_as_child(&window)
                        {
                            let _ = bg_webview.set_visible(false);
                            background_sync_webview = Some(bg_webview);
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::TabUrlChanged { tab_id, url }) => {
                if let Some(index) = tabs.iter().position(|t| t.id == tab_id) {
                    let mut recent_candidate: Option<(String, String)> = None;
                    {
                        let tab = &mut tabs[index];
                        let old_fallback = fallback_title_for_url(&tab.url);
                        tab.url = url;
                        if tab.title.trim().is_empty()
                            || tab.title == "Zenith"
                            || tab.title == old_fallback
                        {
                            tab.title = fallback_title_for_url(&tab.url);
                        }
                        if should_warmup_youtube_account_sync(&tab.url) {
                            let _ = proxy.send_event(UserEvent::OpenBackgroundAuthSync(
                                "https://accounts.google.com/RotateCookiesPage".to_string(),
                            ));
                        }
                        apply_browser_theme_to_tab(tab, &current_theme);
                        if should_track_recent_site(&tab.url) {
                            recent_candidate = Some((tab.url.clone(), tab.title.clone()));
                        }
                    }

                    if let Some((recent_url, recent_title)) = recent_candidate
                        && upsert_recent_site(&mut recent_sites, &recent_url, &recent_title)
                    {
                        save_recent_sites(&recent_sites_path, &recent_sites);
                        for tab in &tabs {
                            sync_recent_sites_to_tab(tab, &recent_sites);
                            sync_bookmarks_to_tab(tab, &bookmarks);
                            sync_history_to_tab(tab, &recent_sites);
                            sync_downloads_to_tab(tab, &downloads);
                        }
                    } else if let Some(home_tab) = tabs.get(index) {
                        sync_recent_sites_to_tab(home_tab, &recent_sites);
                        sync_bookmarks_to_tab(home_tab, &bookmarks);
                        sync_history_to_tab(home_tab, &recent_sites);
                        sync_downloads_to_tab(home_tab, &downloads);
                    }

                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::TabTitleChanged { tab_id, title }) => {
                if let Some(index) = tabs.iter().position(|t| t.id == tab_id) {
                    let mut recent_candidate: Option<(String, String)> = None;
                    {
                        let tab = &mut tabs[index];
                        tab.title = resolved_tab_title(&title, &tab.url);
                        if should_track_recent_site(&tab.url) {
                            recent_candidate = Some((tab.url.clone(), tab.title.clone()));
                        }
                    }

                    if let Some((recent_url, recent_title)) = recent_candidate
                        && upsert_recent_site(&mut recent_sites, &recent_url, &recent_title)
                    {
                        save_recent_sites(&recent_sites_path, &recent_sites);
                        for tab in &tabs {
                            sync_recent_sites_to_tab(tab, &recent_sites);
                            sync_bookmarks_to_tab(tab, &bookmarks);
                            sync_history_to_tab(tab, &recent_sites);
                            sync_downloads_to_tab(tab, &downloads);
                        }
                    }

                    if let Some(tab) = tabs.get(index) {
                        sync_bookmarks_to_tab(tab, &bookmarks);
                        sync_history_to_tab(tab, &recent_sites);
                        sync_downloads_to_tab(tab, &downloads);
                    }

                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::SettingsChanged { key, value }) => {
                let mut next_theme: Option<&str> = None;
                let normalized = match key.as_str() {
                    "theme" => {
                        let v = if value.eq_ignore_ascii_case("light") {
                            "light"
                        } else {
                            "dark"
                        };
                        next_theme = Some(v);
                        Some(("theme", v))
                    }
                    "searchEngine" | "search-engine" => Some((
                        "searchEngine",
                        match value.as_str() {
                            "duckduckgo" => "duckduckgo",
                            "bing" => "bing",
                            _ => "google",
                        },
                    )),
                    _ => None,
                };

                if let Some((k, v)) = normalized
                    && let (Ok(k_json), Ok(v_json)) =
                        (serde_json::to_string(k), serde_json::to_string(v))
                {
                    let js = format!(
                        "if(window.zenithApplySetting) window.zenithApplySetting({k_json}, {v_json});"
                    );
                    let _ = chrome_webview.evaluate_script(&js);
                }

                if let Some(theme) = next_theme {
                    current_theme = theme.to_string();
                    for tab in &tabs {
                        apply_browser_theme_to_tab(tab, &current_theme);
                    }
                }
            }
            Event::WindowEvent {
                window_id, event, ..
            } => match event {
                WindowEvent::CloseRequested if window_id == window.id() => {
                    *control_flow = ControlFlow::Exit;
                }
                WindowEvent::CloseRequested => {
                    auth_windows.retain(|w| w.window.id() != window_id);
                }
                WindowEvent::Resized(_size) if window_id == window.id() => {
                    let _ = chrome_webview.set_bounds(chrome_bounds_for_window(&window));
                    apply_tab_bounds(&tabs, content_bounds_for_window(&window));
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::ToggleMenu) => {
                // Menu logic is now fully handled in ui.html horizontally
            }
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::BookmarkSite;
    use super::DownloadEntry;
    use super::RecentSite;
    use super::{
        fallback_title_for_url, is_background_google_account_sync_url, normalize_user_input_url,
        record_download_completed, record_download_started, resolved_tab_title,
        should_open_auth_window, should_track_recent_site, should_warmup_youtube_account_sync,
        upsert_bookmark, upsert_recent_site,
    };

    #[test]
    fn normalize_user_input_uses_https_for_domains() {
        assert_eq!(
            normalize_user_input_url("example.com"),
            "https://example.com".to_string()
        );
    }

    #[test]
    fn normalize_user_input_uses_google_for_queries() {
        let out = normalize_user_input_url("rust browser project");
        assert!(out.starts_with("https://www.google.com/search?"));
        assert!(out.contains("q=rust%20browser%20project"));
    }

    #[test]
    fn fallback_title_uses_hostname() {
        assert_eq!(
            fallback_title_for_url("https://www.youtube.com/watch?v=1"),
            "youtube.com"
        );
        assert_eq!(fallback_title_for_url("zenith://assets/history"), "History");
    }

    #[test]
    fn resolved_title_falls_back_for_generic_browser_title() {
        assert_eq!(
            resolved_tab_title("Zenith", "https://www.youtube.com/"),
            "youtube.com"
        );
    }

    #[test]
    fn auth_window_detects_known_auth_host() {
        assert!(should_open_auth_window("https://github.com/login"));
    }

    #[test]
    fn auth_window_detects_oauth_parameters() {
        assert!(should_open_auth_window(
            "https://example.com/authorize?client_id=a&redirect_uri=b&response_type=code"
        ));
    }

    #[test]
    fn auth_window_ignores_background_accounts_popup_urls() {
        assert!(!should_open_auth_window(
            "https://accounts.google.com/RotateCookiesPage"
        ));
    }

    #[test]
    fn auth_window_ignores_accounts_root() {
        assert!(!should_open_auth_window("https://accounts.google.com/"));
    }

    #[test]
    fn auth_window_accepts_google_service_login() {
        assert!(should_open_auth_window(
            "https://accounts.google.com/ServiceLogin?hl=en"
        ));
    }

    #[test]
    fn detects_background_google_account_sync_url() {
        assert!(is_background_google_account_sync_url(
            "https://accounts.google.com/RotateCookiesPage?origin=https://www.youtube.com"
        ));
        assert!(!is_background_google_account_sync_url(
            "https://accounts.google.com/ServiceLogin?hl=en"
        ));
    }

    #[test]
    fn detects_youtube_home_for_sync_warmup() {
        assert!(should_warmup_youtube_account_sync(
            "https://www.youtube.com/"
        ));
        assert!(should_warmup_youtube_account_sync("https://youtube.com"));
        assert!(!should_warmup_youtube_account_sync(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
    }

    #[test]
    fn tracks_recent_sites_only_for_regular_http_pages() {
        assert!(should_track_recent_site("https://example.com/a"));
        assert!(!should_track_recent_site("zenith://assets/home"));
        assert!(!should_track_recent_site(
            "https://accounts.google.com/ServiceLogin"
        ));
    }

    #[test]
    fn upsert_recent_site_moves_duplicate_to_front() {
        let mut sites = vec![
            RecentSite {
                url: "https://a.com".to_string(),
                title: "A".to_string(),
            },
            RecentSite {
                url: "https://b.com".to_string(),
                title: "B".to_string(),
            },
        ];
        assert!(upsert_recent_site(&mut sites, "https://a.com", "Site A"));
        assert_eq!(sites[0].url, "https://a.com");
        assert_eq!(sites[0].title, "Site A");
        assert_eq!(sites.len(), 2);
    }

    #[test]
    fn upsert_bookmark_adds_and_prioritizes_latest() {
        let mut bookmarks = vec![BookmarkSite {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
        }];
        assert!(upsert_bookmark(
            &mut bookmarks,
            "https://github.com",
            "GitHub - Build software better"
        ));
        assert_eq!(bookmarks[0].url, "https://github.com");
    }

    #[test]
    fn records_download_start_and_completion_status() {
        let mut downloads: Vec<DownloadEntry> = Vec::new();
        record_download_started(
            &mut downloads,
            "https://example.com/file.zip",
            "/tmp/file.zip",
        );
        assert_eq!(downloads[0].status, "in_progress");

        record_download_completed(
            &mut downloads,
            "https://example.com/file.zip",
            Some("/tmp/file.zip".to_string()),
            true,
        );
        assert_eq!(downloads[0].status, "completed");
    }
}
