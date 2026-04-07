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
#[cfg(target_os = "macos")]
use tao::platform::macos::WindowExtMacOS;

const CHROME_HEIGHT: u32 = 82;
const CUSTOM_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
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
    ShowThreeDotsMenu { x: f64, y: f64 },
    FindInPage {
        query: String,
        forward: bool,
    },
    MenuAction(muda::MenuId),
    OpenFindBar,
    SaveImage {
        url: String,
        filename: String,
    },
    OpenImageInTab(String),
    ShowToast {
        message: String,
        toast_type: String,
    },
    ImageContextMenu {
        url: String,
        filename: String,
        x: f64,
        y: f64,
    },
    DownloadHistoryUpdate,
    TabPermissionChanged {
        tab_id: u32,
        permission: String,
        granted: bool,
    },
    PermissionRequest {
        tab_id: u32,
        url: String,
        permission: String,
        request_id: String,
    },
    PermissionDecision {
        tab_id: u32,
        permission: String,
        decision: String,
        request_id: String,
    },
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
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    forward: Option<bool>,
    #[serde(default)]
    filename: Option<String>,
    // Permission related
    #[serde(default)]
    permission: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    granted: Option<bool>,
    #[serde(default)]
    toast_type: Option<String>,
    // Bridge for mixed naming conventions
}

struct BrowserTab {
    id: u32,
    url: String,
    title: String,
    webview: WebView,
    active_permissions: Vec<String>,
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
    is_bookmarked: bool,
    active_permissions: Vec<String>,
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

fn toggle_bookmark(bookmarks: &mut Vec<BookmarkSite>, raw_url: &str, raw_title: &str) -> (bool, bool) {
    if !should_track_recent_site(raw_url) {
        return (false, false);
    }

    let title = resolved_tab_title(raw_title, raw_url);
    
    if let Some(existing) = bookmarks.iter().position(|s| s.url == raw_url) {
        bookmarks.remove(existing);
        (true, false) // Changed, was_removed
    } else {
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
        }
        (true, true) // Changed, was_added
    }
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

fn get_user_agent_data_js() -> String {
    r#"
    try {
        Object.defineProperty(navigator, 'vendor', { get: function() { return 'Google Inc.'; } });
        Object.defineProperty(navigator, 'userAgentData', {
            get: () => ({
                brands: [
                    { brand: 'Chromium', version: '124' },
                    { brand: 'Google Chrome', version: '124' },
                    { brand: 'Not-A.Brand', version: '99' }
                ],
                mobile: false,
                platform: 'macOS'
            })
        });
    } catch (_) {}
    "#.to_string()
}

fn tab_initialization_script(tab_id: u32) -> String {
    let ua_js = get_user_agent_data_js();
    format!(
        r#"
        (function() {{
            {ua_js}
            
            window.__ZENITH_TAB_ID = {tab_id};
            var send = function(payload) {{
                try {{ window.ipc.postMessage(JSON.stringify(payload)); }} catch (_) {{}}
            }};

            // Permission Tracking & Custom Prompts
            window._permRequests = {{}};
            var notifyPermission = function(name, granted) {{
                send({{ type: 'tab_permission_update', tabId: {tab_id}, permission: name, granted: granted }});
            }};

            var requestPermission = function(name) {{
                return new Promise(function(resolve) {{
                    var requestId = Math.random().toString(36).substring(7);
                    window._permRequests[requestId] = resolve;
                    send({{ type: 'permission_request', tabId: {tab_id}, permission: name, requestId: requestId, url: window.location.href }});
                }});
            }};

            window._zenith_grant_permission = function(requestId, result) {{
                if (window._permRequests[requestId]) {{
                    window._permRequests[requestId](result);
                    delete window._permRequests[requestId];
                }}
            }};

            // Camera & Microphone
            if (navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia === 'function') {{
                var originalGUM = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
                navigator.mediaDevices.getUserMedia = async function() {{
                    console.log("[Zenith] Intercepted REAL getUserMedia");
                    try {{
                        var constraints = arguments[0];
                        var type = (constraints && constraints.video) ? 'camera' : 'microphone';
                        
                        var result = await requestPermission(type);
                        console.log("[Zenith] Permission result:", result);
                        if (result === 'granted') {{
                            if (constraints.video) notifyPermission('camera', true);
                            if (constraints.audio) notifyPermission('microphone', true);
                            return originalGUM.apply(navigator.mediaDevices, arguments);
                        }} else {{
                            throw new DOMException("Permission denied by user", "NotAllowedError");
                        }}
                    }} catch (e) {{
                        console.error("[Zenith] getUserMedia error:", e);
                        throw e;
                    }}
                }};
            }}

            // Geolocation
            if (navigator.geolocation && typeof navigator.geolocation.getCurrentPosition === 'function') {{
                var originalGCP = navigator.geolocation.getCurrentPosition.bind(navigator.geolocation);
                navigator.geolocation.getCurrentPosition = async function() {{
                    var success = arguments[0];
                    var error = arguments[1];
                    var options = arguments[2];

                    var result = await requestPermission('geolocation');
                    if (result === 'granted') {{
                        notifyPermission('geolocation', true);
                        return originalGCP.apply(navigator.geolocation, arguments);
                    }} else if (error) {{
                        error({{ code: 1, message: "User denied Geolocation" }});
                    }}
                }};
                
                var originalWP = navigator.geolocation.watchPosition.bind(navigator.geolocation);
                navigator.geolocation.watchPosition = async function() {{
                    var result = await requestPermission('geolocation');
                    if (result === 'granted') {{
                        notifyPermission('geolocation', true);
                        return originalWP.apply(navigator.geolocation, arguments);
                    }}
                    return -1;
                }};
            }}

            // Notifications
            if (window.Notification && window.Notification.requestPermission) {{
                var originalRequestPermission = window.Notification.requestPermission.bind(window.Notification);
                window.Notification.requestPermission = function() {{
                    return originalRequestPermission.apply(this, arguments).then(function(result) {{
                        if (result === 'granted') notifyPermission('notifications', true);
                        return result;
                    }});
                }};
            }}

            // Shim mediaDevices if missing (common in insecure contexts or unbundled apps)
            if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {{
                var mockMediaDevices = {{
                    getUserMedia: async function() {{ 
                        console.log("[Zenith] Intercepted MOCK getUserMedia");
                        var constraints = arguments[0];
                        var type = (constraints && constraints.video) ? 'camera' : 'microphone';
                        var result = await requestPermission(type);
                        console.log("[Zenith] Mock permission result:", result);
                        if (result === 'granted') {{
                             return Promise.reject(new DOMException("Zenith internal grant successful, but real hardware is blocked by OS/context. Use a secure HTTPS connection for real video.", "NotReadableError")); 
                        }}
                        return Promise.reject(new DOMException("Permission denied by user", "NotAllowedError")); 
                    }},
                    enumerateDevices: function() {{ 
                        return Promise.resolve([
                            {{ deviceId: 'zenith-vcam', kind: 'videoinput', label: 'Zenith Camera (Permission Required)', groupId: 'zenith-media' }},
                            {{ deviceId: 'zenith-vmic', kind: 'audioinput', label: 'Zenith Microphone (Permission Required)', groupId: 'zenith-media' }}
                        ]); 
                    }},
                    addEventListener: function() {{}},
                    removeEventListener: function() {{}},
                    dispatchEvent: function() {{ return false; }},
                    ondevicechange: null
                }};
                try {{
                    if (!navigator.mediaDevices) {{
                        Object.defineProperty(navigator, 'mediaDevices', {{
                            get: function() {{ return mockMediaDevices; }},
                            configurable: true
                        }});
                    }} else if (!navigator.mediaDevices.getUserMedia) {{
                        navigator.mediaDevices.getUserMedia = mockMediaDevices.getUserMedia;
                    }}
                }} catch (e) {{}}
            }}
            // Spoof Secure Context to enable features on zenith:// schemes
            if (window.isSecureContext === false) {{
                try {{
                    Object.defineProperty(window, 'isSecureContext', {{ get: function() {{ return true; }} }});
                }} catch (e) {{}}
            }}

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

            // Right-click context menu for images
            document.addEventListener('contextmenu', function(e) {{
                var el = e.target ? e.target.closest('img') : null;
                if (!el) return;
                e.preventDefault();
                var src = el.src || el.currentSrc || el.getAttribute('src') || '';
                if (!src) return;
                var filename = src.split('/').pop().split('?')[0] || 'image.jpg';
                if (!filename.includes('.')) filename += '.jpg';
                send({{ type: 'image_context_menu', url: src, filename: filename, x: e.screenX, y: e.screenY }});
            }});
        }})();
        "#,
        ua_js = ua_js,
        tab_id = tab_id,
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

fn sync_chrome_state(chrome: &WebView, tabs: &[BrowserTab], active_tab_id: Option<u32>, bookmarks: &[BookmarkSite]) {
    let state = ChromeState {
        tabs: tabs
            .iter()
            .map(|t| {
                let is_bookmarked = bookmarks.iter().any(|b| b.url == t.url);
                ChromeTabState {
                    id: t.id,
                    title: t.title.clone(),
                    url: t.url.clone(),
                    is_bookmarked,
                    active_permissions: t.active_permissions.clone(),
                }
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
        "tab_permission_update" => {
            if let (Some(id), Some(perm)) = (tab_id, message.permission) {
                let granted = message.granted.unwrap_or(true);
                let _ = proxy.send_event(UserEvent::TabPermissionChanged {
                    tab_id: id,
                    permission: perm,
                    granted,
                });
            }
        }
        "permission_request" => {
            if let (Some(id), Some(url), Some(permission), Some(request_id)) = (tab_id, message.url, message.permission, message.request_id) {
                println!("Permission request: {} for {} on tab {}", permission, url, id);
                let _ = proxy.send_event(UserEvent::PermissionRequest {
                    tab_id: id,
                    url,
                    permission: permission,
                    request_id,
                });
            }
        }
        "permission_decision" => {
            if let (Some(id), Some(permission), Some(decision), Some(request_id)) = (message.tab_id, message.permission, message.decision, message.request_id) {
                let _ = proxy.send_event(UserEvent::PermissionDecision {
                    tab_id: id,
                    permission: permission,
                    decision,
                    request_id,
                });
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
        "show_context_menu" => {
            if let (Some(x), Some(y)) = (message.x, message.y) {
                let _ = proxy.send_event(UserEvent::ShowThreeDotsMenu { x, y });
            }
        }
        "find_in_page" => {
            if let Some(query) = message.query {
                let forward = message.forward.unwrap_or(true);
                let _ = proxy.send_event(UserEvent::FindInPage { query, forward });
            }
        }
        "save_image" => {
            if let (Some(url), Some(filename)) = (message.url, message.filename) {
                let _ = proxy.send_event(UserEvent::SaveImage { url, filename });
            }
        }
        "image_context_menu" => {
            if let (Some(url), Some(filename)) = (message.url.clone(), message.filename.clone()) {
                let x = message.x.unwrap_or(0.0);
                let y = message.y.unwrap_or(0.0);
                let _ = proxy.send_event(UserEvent::ImageContextMenu { url, filename, x, y });
            }
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
    let _perm_proxy = proxy.clone();
    let protocol_html = ui_html;
    let init_script = tab_initialization_script(tab_id);

    let webview_builder = WebViewBuilder::new_with_web_context(web_context)
        .with_user_agent(CUSTOM_USER_AGENT)
        .with_bounds(bounds)
        .with_url(url)
        .with_back_forward_navigation_gestures(true)
        .with_initialization_script(&init_script)
        .with_navigation_handler(move |next: String| {
            if next.starts_with("zenith://") {
                return is_assets_url(&next);
            }

            is_http_like_url(&next)
        })
        .with_new_window_req_handler(move |next: String, _features: wry::NewWindowFeatures| {
            let next_url = next.clone();
            if is_background_google_account_sync_url(&next_url) {
                let _ = popup_proxy.send_event(UserEvent::OpenBackgroundAuthSync(next_url));
                wry::NewWindowResponse::Deny
            } else if should_open_auth_window(&next_url) {
                // Determine if this is a background-like redirect that should just navigate current tab
                let Ok(parsed) = Url::parse(&next_url) else { return wry::NewWindowResponse::Deny };
                let host = parsed.host_str().unwrap_or_default();
                if host == "accounts.google.com" && (parsed.path().contains("checkcookie") || parsed.path().contains("rotatecookiespage")) {
                    let _ = popup_proxy.send_event(UserEvent::NavigateTab { tab_id: Some(tab_id), url: next_url });
                } else {
                    let _ = popup_proxy.send_event(UserEvent::OpenAuthWindow(next_url));
                }
                wry::NewWindowResponse::Deny
            } else if is_http_like_url(&next_url) {
                let _ = popup_proxy.send_event(UserEvent::NewTab {
                    url: Some(next_url),
                    activate: true,
                });
                wry::NewWindowResponse::Deny
            } else {
                wry::NewWindowResponse::Deny
            }
        })
        .with_document_title_changed_handler(move |title: String| {
            let _ = title_proxy.send_event(UserEvent::TabTitleChanged { tab_id, title });
        })
        .with_on_page_load_handler(move |_event: PageLoadEvent, url: String| {
            let _ = load_proxy.send_event(UserEvent::TabUrlChanged { tab_id, url });
        })
        .with_download_started_handler(move |url: String, path: &mut std::path::PathBuf| {
            // If wry doesn't set a path, force it to ~/Downloads/<filename>
            if path.as_os_str().is_empty() {
                if let Some(filename) = url.split('/').last().and_then(|f: &str| if f.is_empty() { None } else { Some(f) }) {
                    let dl_dir = dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
                    *path = dl_dir.join(filename.split('?').next().unwrap_or(filename));
                }
            }
            let path_str = path.to_string_lossy().to_string();
            let _ = download_start_proxy.send_event(UserEvent::DownloadStarted {
                url,
                path: path_str,
            });
            true
        })
        .with_download_completed_handler(move |url: String, path: Option<std::path::PathBuf>, success: bool| {
            let _ = download_complete_proxy.send_event(UserEvent::DownloadCompleted {
                url,
                path: path.map(|p: std::path::PathBuf| p.to_string_lossy().to_string()),
                success,
            });
        })
        .with_ipc_handler(move |request: Request<String>| {
            dispatch_ipc_message(request.body(), &ipc_proxy, Some(tab_id));
        })
        .with_custom_protocol("zenith".into(), move |_id, request: Request<Vec<u8>>| {
            handle_zenith_request(protocol_html.as_str(), request)
        });

    let webview = webview_builder
        .build_as_child(window)
        .ok()?;

    Some(BrowserTab {
        id: tab_id,
        url: url.to_string(),
        title: "Zenith".to_string(),
        webview,
        active_permissions: Vec::new(),
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
    
    use muda::{Menu, MenuItem, CheckMenuItem, Submenu, PredefinedMenuItem, ContextMenu};
    use muda::accelerator::{Accelerator, Code, Modifiers};
    
    let menu_bar = Menu::new();
    
    // Application Menu
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::new("Zenith", true);
        app_menu.append_items(&[
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
        menu_bar.append_items(&[&app_menu]).unwrap();
    }

    // File/Tab Menu
    let tab_menu = Submenu::new("Tabs", true);
    let m_new_tab = MenuItem::new("New Tab", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyT)));
    let m_close_tab = MenuItem::new("Close Tab", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyW)));
    let m_bookmark = MenuItem::new("Bookmark Page", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyD)));
    let m_settings = MenuItem::new("Settings", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)));
    tab_menu.append_items(&[
        &m_new_tab,
        &m_close_tab,
        &PredefinedMenuItem::separator(),
        &m_bookmark,
        &m_settings,
    ]).unwrap();

    // Edit/Find Menu
    let edit_menu = Submenu::new("Edit", true);
    let m_find = MenuItem::new("Find in Page...", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyF)));
    edit_menu.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
        &PredefinedMenuItem::separator(),
        &m_find,
    ]).unwrap();

    // View Menu
    let view_menu = Submenu::new("View", true);
    let m_history = MenuItem::new("History", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyY)));
    let m_downloads = MenuItem::new("Downloads", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyJ)));
    let m_theme = CheckMenuItem::new("Light Mode", true, current_theme == "light", None);
    let m_reload = MenuItem::new("Reload Page", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyR)));
    view_menu.append_items(&[
        &m_reload,
        &PredefinedMenuItem::separator(),
        &m_history,
        &m_downloads,
        &PredefinedMenuItem::separator(),
        &m_theme,
    ]).unwrap();

    menu_bar.append_items(&[&tab_menu, &edit_menu, &view_menu]).unwrap();

    #[cfg(target_os = "macos")]
    menu_bar.init_for_nsapp();

    // Route ALL menu events through EventLoopProxy so they wake the event loop
    // regardless of which window/webview has focus.
    let menu_proxy = proxy.clone();
    muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::MenuAction(e.id));
    }));

    // Context menu (for dots button - shared items)
    let dots_menu = Menu::new();
    dots_menu.append_items(&[
        &m_new_tab, &m_bookmark, &m_history, &m_downloads,
        &muda::PredefinedMenuItem::separator(),
        &m_find, &m_theme,
        &muda::PredefinedMenuItem::separator(),
        &m_close_tab,
    ]).unwrap();

    // Image right-click context menu
    let img_menu = Menu::new();
    let img_save = MenuItem::new("Save Image to Downloads", true, None);
    let img_open = MenuItem::new("Open Image in New Tab", true, None);
    img_menu.append_items(&[&img_save, &img_open]).unwrap();

    // Shared mutable state for the active image context
    use std::sync::{Arc as SArc, Mutex};
    let img_ctx: SArc<Mutex<Option<(String, String)>>> = SArc::new(Mutex::new(None));

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
        tabs.push(BrowserTab {
            id: initial_tab.id,
            url: initial_tab.url,
            title: initial_tab.title,
            webview: initial_tab.webview,
            active_permissions: Vec::new(),
        });
        apply_tab_visibility(&tabs, active_tab_id);
    }

    event_loop.run(move |event, event_loop_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if let Event::UserEvent(UserEvent::MenuAction(ref menu_id)) = event {
            if *menu_id == m_new_tab.id() {
                let _ = proxy.send_event(UserEvent::NewTab { url: None, activate: true });
            } else if *menu_id == m_close_tab.id() {
                let _ = proxy.send_event(UserEvent::CloseTab(active_tab_id));
            } else if *menu_id == m_bookmark.id() {
                let _ = proxy.send_event(UserEvent::BookmarkActiveTab(active_tab_id));
            } else if *menu_id == m_history.id() {
                let _ = proxy.send_event(UserEvent::OpenHistoryTab);
            } else if *menu_id == m_downloads.id() {
                let _ = proxy.send_event(UserEvent::OpenDownloadsTab);
            } else if *menu_id == m_find.id() {
                let _ = proxy.send_event(UserEvent::OpenFindBar);
            } else if *menu_id == m_reload.id() {
                let _ = proxy.send_event(UserEvent::TabAction { tab_id: active_tab_id, action: BrowserAction::Reload });
            } else if *menu_id == m_theme.id() {
                let next = if current_theme == "light" { "dark" } else { "light" };
                let _ = proxy.send_event(UserEvent::SettingsChanged { key: "theme".to_string(), value: next.to_string() });
            } else if *menu_id == m_settings.id() {
                let _ = proxy.send_event(UserEvent::OpenSettingsTab);
            } else if *menu_id == img_save.id() {
                if let Ok(guard) = img_ctx.lock() {
                    if let Some((url, filename)) = guard.clone() {
                        let _ = proxy.send_event(UserEvent::SaveImage { url, filename });
                    }
                }
            } else if *menu_id == img_open.id() {
                if let Ok(guard) = img_ctx.lock() {
                    if let Some((url, _)) = guard.clone() {
                        let _ = proxy.send_event(UserEvent::OpenImageInTab(url));
                    }
                }
            }
        }

        let _keep_context_alive = &web_context;

        match event {
            Event::UserEvent(UserEvent::ChromeReady) => {
                chrome_ready = true;
                sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
                    }
                }
            }
            Event::UserEvent(UserEvent::SwitchTab(tab_id)) => {
                if tabs.iter().any(|t| t.id == tab_id) {
                    active_tab_id = Some(tab_id);
                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
                    }
                }
            }
            Event::UserEvent(UserEvent::CloseTab(tab_id)) => {
                if let Some(close_id) = tab_id.or(active_tab_id) {
                    if let Some(idx) = tabs.iter().position(|t| t.id == close_id) {
                        if tabs.len() <= 1 {
                            // User requested that the application closes when the last tab is closed
                            *control_flow = ControlFlow::Exit;
                            return;
                        }

                        tabs.remove(idx);
                        if active_tab_id == Some(close_id) {
                            let next_idx = idx.saturating_sub(1);
                            active_tab_id = tabs.get(next_idx).map(|t| t.id);
                        }
                    }

                    apply_tab_visibility(&tabs, active_tab_id);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                                sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                    let (changed, was_added) = toggle_bookmark(&mut bookmarks, &bookmark_url, &bookmark_title);
                    if changed {
                        save_bookmarks(&bookmarks_path, &bookmarks);
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
                        for t in &tabs {
                            sync_bookmarks_to_tab(t, &bookmarks);
                        }
                        
                        let msg = if was_added { "Added Bookmark" } else { "Bookmark Removed" };
                        let toast_type = if was_added { "success" } else { "info" };
                        let _ = chrome_webview.evaluate_script(&format!("if (window.showToast) window.showToast({}, '{}');", serde_json::to_string(&msg).unwrap(), toast_type));
                    }
                }
            }
            Event::UserEvent(UserEvent::DownloadStarted { url, path }) => {
                record_download_started(&mut downloads, &url, &path);
                save_downloads(&downloads_path, &downloads);
                for tab in &tabs {
                    sync_downloads_to_tab(tab, &downloads);
                }
                let filename = std::path::Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("file");
                let msg = format!("Downloading {}", filename);
                let _ = chrome_webview.evaluate_script(&format!("if (window.showToast) window.showToast({}, 'info');", serde_json::to_string(&msg).unwrap()));
            }
            Event::UserEvent(UserEvent::DownloadCompleted { url, path, success }) => {
                record_download_completed(&mut downloads, &url, path.clone(), success);
                save_downloads(&downloads_path, &downloads);
                for tab in &tabs {
                    sync_downloads_to_tab(tab, &downloads);
                }
                let filename = path.as_ref().and_then(|p| std::path::Path::new(p).file_name()).and_then(|s| s.to_str()).unwrap_or("file");
                let (msg, toast_type) = if success {
                    (format!("Finished downloading {}", filename), "success")
                } else {
                    (format!("Failed to download {}", filename), "error")
                };
                let _ = chrome_webview.evaluate_script(&format!("if (window.showToast) window.showToast({}, '{}');", serde_json::to_string(&msg).unwrap(), toast_type));
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
                                .with_initialization_script(&get_user_agent_data_js())
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
                            .with_initialization_script(&get_user_agent_data_js())
                            .build_as_child(&window)
                        {
                            let _ = bg_webview.set_visible(false);
                            background_sync_webview = Some(bg_webview);
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::TabPermissionChanged { tab_id, permission, granted }) => {
                if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
                    let p = permission.to_lowercase();
                    if granted {
                        if !tab.active_permissions.contains(&p) {
                            tab.active_permissions.push(p);
                        }
                    } else {
                        tab.active_permissions.retain(|perm| perm != &p);
                    }
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        tab.active_permissions.clear();
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id, &bookmarks);
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
            Event::UserEvent(UserEvent::ShowThreeDotsMenu { x, y }) => {
                #[cfg(target_os = "macos")]
                unsafe {
                    dots_menu.show_context_menu_for_nsview(window.ns_view() as _, Some(tao::dpi::Position::Logical(tao::dpi::LogicalPosition::new(x, y))));
                }
            }
            Event::UserEvent(UserEvent::FindInPage { query, forward }) => {
                if let Some(tab_id) = active_tab_id {
                    if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
                        // window.find(aString, aCaseSensitive, aBackwards, aWrapAround, aWholeWord, aSearchInFrames, aShowDialog)
                        let backwards = if forward { "false" } else { "true" };
                        let escaped = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_string());
                        let js = format!("window.find({}, false, {}, true, false, false, false);", escaped, backwards);
                        let _ = tab.webview.evaluate_script(&js);
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenFindBar) => {
                // Inject a self-contained find bar into the active tab's webview.
                // This is reliable because window.find() runs in the same context as the page.
                if let Some(tab_id) = active_tab_id {
                    if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
                        let find_js = r#"
(function() {
    const existingBar = document.getElementById('__zenith_find__');
    if (existingBar) {
        const inp = existingBar.querySelector('input');
        if (inp) { inp.focus(); inp.select(); }
        return;
    }
    
    // Create the host element with a shadow DOM to isolate from page CSS
    const host = document.createElement('div');
    host.id = '__zenith_find__';
    host.style.cssText = 'all:initial;position:fixed;top:16px;right:16px;z-index:2147483647;';
    document.documentElement.appendChild(host);
    
    const shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML = `
        <style>
            :host { all: initial; }
            .bar {
                display: flex; align-items: center; gap: 6px;
                background: rgba(25,25,35,0.97);
                border: 1px solid rgba(100,120,255,0.4);
                border-radius: 12px; padding: 7px 10px;
                box-shadow: 0 12px 40px rgba(0,0,0,0.6);
                backdrop-filter: blur(24px);
                font-family: -apple-system, BlinkMacSystemFont, sans-serif;
            }
            input {
                background: rgba(255,255,255,0.12);
                border: 1px solid rgba(255,255,255,0.2);
                border-radius: 7px; padding: 6px 11px;
                color: #fff; font-size: 13px; width: 190px;
                outline: none; font-family: inherit;
            }
            input:focus { border-color: rgba(100,140,255,0.7); }
            input::placeholder { color: rgba(255,255,255,0.4); }
            button {
                background: rgba(255,255,255,0.08);
                border: 1px solid rgba(255,255,255,0.14);
                border-radius: 7px; color: #ccc;
                padding: 5px 9px; cursor: pointer;
                font-size: 13px; font-family: inherit;
                transition: background 0.1s;
            }
            button:hover { background: rgba(255,255,255,0.18); color: #fff; }
            .count { color: rgba(255,255,255,0.5); font-size: 11px; min-width: 44px; text-align: center; }
            .close { background: transparent; border: none; font-size: 17px; color: rgba(255,255,255,0.5); padding: 2px 6px; }
            .close:hover { color: #f87171; background: rgba(248,113,113,0.15); }
        </style>
        <div class="bar">
            <input id="findinput" placeholder="Find in page..." autocomplete="off" spellcheck="false" />
            <button id="prev">↑</button>
            <button id="next">↓</button>
            <span class="count" id="count"></span>
            <button class="close" id="close">✕</button>
        </div>
    `;
    
    const inp = shadow.getElementById('findinput');
    const countEl = shadow.getElementById('count');
    let lastQuery = '';
    
    function doFind(q, forward) {
        if (!q) { window.getSelection() && window.getSelection().removeAllRanges(); countEl.textContent = ''; return; }
        if (q !== lastQuery) {
            window.getSelection() && window.getSelection().removeAllRanges();
            lastQuery = q;
        }
        const found = window.find(q, false, !forward, true, false, false, false);
        countEl.textContent = found ? '✓ Found' : '✗ Not found';
        // Reclaim focus after window.find() moves it to the highlighted text
        setTimeout(function() { inp.focus(); }, 0);
    }
    
    // Do NOT search on input - only on Enter / button click
    // (live search causes focus-reset loop after every character)
    inp.addEventListener('keydown', function(e) {
        e.stopPropagation();
        e.stopImmediatePropagation();
        if (e.key === 'Enter') { doFind(inp.value, !e.shiftKey); e.preventDefault(); }
        if (e.key === 'Escape') { host.remove(); e.preventDefault(); }
    }, true);
    
    shadow.getElementById('next').addEventListener('click', function() { doFind(inp.value, true); });
    shadow.getElementById('prev').addEventListener('click', function() { doFind(inp.value, false); });
    shadow.getElementById('close').addEventListener('click', function() { host.remove(); });
    
    setTimeout(function() { inp.focus(); }, 30);
})();
"#;
                        let _ = tab.webview.evaluate_script(find_js);
                    }
                }
            }
            Event::UserEvent(UserEvent::MenuAction(_)) => {
                // Already handled above before the match
            }
            Event::UserEvent(UserEvent::ImageContextMenu { url, filename, x, y }) => {
                // Store current image context so menu items can reference it
                if let Ok(mut guard) = img_ctx.lock() {
                    *guard = Some((url, filename));
                }
                // Show the image context menu at the click position
                #[cfg(target_os = "macos")]
                unsafe {
                    img_menu.show_context_menu_for_nsview(
                        window.ns_view() as _,
                        Some(tao::dpi::Position::Logical(tao::dpi::LogicalPosition::new(x, y))),
                    );
                }
            }
            Event::UserEvent(UserEvent::OpenImageInTab(url)) => {
                let _ = proxy.send_event(UserEvent::NewTab { url: Some(url), activate: true });
            }
            Event::UserEvent(UserEvent::DownloadHistoryUpdate) => {
                for tab in &tabs {
                    sync_downloads_to_tab(tab, &downloads);
                }
            }
            Event::UserEvent(UserEvent::SaveImage { url, filename }) => {
                let dl_dir = dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                let save_path = dl_dir.join(&filename);
                let path_str = save_path.display().to_string();
                // Record in download history immediately
                record_download_started(&mut downloads, &url, &path_str);
                save_downloads(&downloads_path, &downloads);
                for tab in &tabs { sync_downloads_to_tab(tab, &downloads); }
                let msg_start = format!("Downloading {}", filename);
                let _ = chrome_webview.evaluate_script(&format!(
                    "if (window.showToast) window.showToast({}, 'info');",
                    serde_json::to_string(&msg_start).unwrap()
                ));
                let toast_proxy = proxy.clone();
                let filename_clone = filename.clone();
                let url_clone = url.clone();
                let path_str_clone = path_str.clone();
                std::thread::spawn(move || {
                    match reqwest::blocking::get(&url_clone) {
                        Ok(resp) if resp.status().is_success() => {
                            match resp.bytes() {
                                Ok(bytes) => {
                                    if let Err(e) = std::fs::write(&save_path, &bytes) {
                                        let _ = toast_proxy.send_event(UserEvent::DownloadCompleted {
                                            url: url_clone,
                                            path: Some(path_str_clone),
                                            success: false,
                                        });
                                        let _ = toast_proxy.send_event(UserEvent::ShowToast {
                                            message: format!("Failed to save {}: {}", filename_clone, e),
                                            toast_type: "error".to_string(),
                                        });
                                    } else {
                                        let _ = toast_proxy.send_event(UserEvent::DownloadCompleted {
                                            url: url_clone,
                                            path: Some(path_str_clone),
                                            success: true,
                                        });
                                        let _ = toast_proxy.send_event(UserEvent::ShowToast {
                                            message: format!("Saved {} to Downloads", filename_clone),
                                            toast_type: "success".to_string(),
                                        });
                                    }
                                }
                                Err(e) => {
                                    let _ = toast_proxy.send_event(UserEvent::DownloadCompleted {
                                        url: url_clone,
                                        path: None,
                                        success: false,
                                    });
                                    let _ = toast_proxy.send_event(UserEvent::ShowToast {
                                        message: format!("Download failed: {}", e),
                                        toast_type: "error".to_string(),
                                    });
                                }
                            }
                        }
                        _ => {
                            let _ = toast_proxy.send_event(UserEvent::ShowToast {
                                message: format!("Could not download {}", filename_clone),
                                toast_type: "error".to_string(),
                            });
                        }
                    }
                });
            }
            Event::UserEvent(UserEvent::ShowToast { message, toast_type }) => {
                let _ = chrome_webview.evaluate_script(&format!(
                    "if (window.showToast) window.showToast({}, {});",
                    serde_json::to_string(&message).unwrap(),
                    serde_json::to_string(&toast_type).unwrap()
                ));
            }
            Event::UserEvent(UserEvent::PermissionRequest { tab_id, url, permission, request_id }) => {
                println!("[Zenith] Permission Request: tab {}, url {}, perm {}", tab_id, url, permission);
                let js = format!("if (window.showPermissionPrompt) window.showPermissionPrompt({}, {}, '{}', '{}');", 
                    tab_id, 
                    serde_json::to_string(&url).unwrap(),
                    permission,
                    request_id);
                let _ = chrome_webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::PermissionDecision { tab_id, permission: _, decision, request_id }) => {
                if let Some(tab) = tabs.iter().find(|t| t.id == tab_id) {
                    let js = format!("if (window._zenith_grant_permission) window._zenith_grant_permission('{}', '{}');", 
                        request_id, decision);
                    let _ = tab.webview.evaluate_script(&js);
                }
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
