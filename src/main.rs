use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
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
const HOME_URL: &str = "zenith://assets/home";
const SETTINGS_URL: &str = "zenith://assets/settings";

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

fn fallback_title_for_url(raw_url: &str) -> String {
    if raw_url.starts_with(SETTINGS_URL) {
        return "Settings".to_string();
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
    format!("https://www.google.com/search?q={q}")
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
        "open_auth" => {
            if let Some(url) = message.url {
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
    let protocol_html = ui_html;
    let init_script = tab_initialization_script(tab_id);

    let webview = WebViewBuilder::new_with_web_context(web_context)
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
            }
            wry::NewWindowResponse::Deny
        })
        .with_document_title_changed_handler(move |title| {
            let _ = title_proxy.send_event(UserEvent::TabTitleChanged { tab_id, title });
        })
        .with_on_page_load_handler(move |_event: PageLoadEvent, url| {
            let _ = load_proxy.send_event(UserEvent::TabUrlChanged { tab_id, url });
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

fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let profile_dir = profile_directory();
    let mut web_context = WebContext::new(Some(profile_dir.join("webview")));

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
            Event::UserEvent(UserEvent::OpenAuthWindow(url)) => {
                if is_http_like_url(&url) {
                    let popup_proxy = proxy.clone();
                    if let Ok(auth_window) = WindowBuilder::new()
                        .with_title("Zenith Sign In")
                        .with_inner_size(LogicalSize::new(980.0, 760.0))
                        .build(event_loop_target)
                        && let Ok(auth_webview) =
                            WebViewBuilder::new_with_web_context(&mut web_context)
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
                if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
                    let old_fallback = fallback_title_for_url(&tab.url);
                    tab.url = url;
                    if tab.title.trim().is_empty() || tab.title == "Zenith" || tab.title == old_fallback
                    {
                        tab.title = fallback_title_for_url(&tab.url);
                    }
                    if should_warmup_youtube_account_sync(&tab.url) {
                        let _ = proxy.send_event(UserEvent::OpenBackgroundAuthSync(
                            "https://accounts.google.com/RotateCookiesPage".to_string(),
                        ));
                    }
                    apply_browser_theme_to_tab(tab, &current_theme);
                    if chrome_ready {
                        sync_chrome_state(&chrome_webview, &tabs, active_tab_id);
                    }
                }
            }
            Event::UserEvent(UserEvent::TabTitleChanged { tab_id, title }) => {
                if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
                    let trimmed = title.trim();
                    tab.title = if trimmed.is_empty() {
                        fallback_title_for_url(&tab.url)
                    } else {
                        trimmed.to_string()
                    };
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
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        fallback_title_for_url, is_background_google_account_sync_url, normalize_user_input_url,
        should_open_auth_window, should_warmup_youtube_account_sync,
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
        assert!(out.starts_with("https://www.google.com/search?q="));
    }

    #[test]
    fn fallback_title_uses_hostname() {
        assert_eq!(
            fallback_title_for_url("https://www.youtube.com/watch?v=1"),
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
}
