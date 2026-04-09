use std::sync::{Arc, Mutex};
use tao::window::Window;
use tao::event_loop::{EventLoopProxy, ControlFlow, EventLoopWindowTarget};
use tao::dpi::LogicalSize;
use wry::{WebView, WebViewBuilder, WebContext, Rect, dpi::{LogicalPosition, LogicalSize as WryLogicalSize}};

use crate::ipc::{UserEvent, BrowserAction, Suggestion, ChromeState, ChromeTabState};
use crate::config::{RecentSite, BookmarkSite, DownloadEntry, load_recent_sites, load_bookmarks, load_downloads, save_bookmarks, toggle_bookmark};
use crate::tab::{BrowserTab, build_browser_tab};
use crate::utils::{is_assets_url, fallback_title_for_url, normalize_user_input_url, should_warmup_youtube_account_sync, HOME_URL, HISTORY_URL, DOWNLOADS_URL, NON_ALPHANUMERIC};
use percent_encoding::utf8_percent_encode;
use crate::ui_handler::handle_zenith_request;
use crate::menu::AppMenu;

pub const CHROME_HEIGHT: u32 = 82;

pub struct AuthWindow {
    pub window: Window,
    pub _webview: WebView,
}

pub struct BrowserApp {
    pub window: Window,
    pub web_context: WebContext,
    pub tabs: Vec<BrowserTab>,
    pub active_tab_id: Option<u32>,
    pub next_tab_id: u32,
    pub chrome_webview: WebView,
    pub palette_webview: WebView,
    pub chrome_ready: bool,
    pub current_theme: String,
    pub auth_windows: Vec<AuthWindow>,
    pub background_sync_webview: Option<WebView>,
    pub recent_sites: Vec<RecentSite>,
    pub bookmarks: Vec<BookmarkSite>,
    pub downloads: Vec<DownloadEntry>,
    pub menu: AppMenu,
    pub img_ctx: Arc<Mutex<Option<(String, String)>>>,
    pub final_ui_html: Arc<String>,
}

impl BrowserApp {
    pub fn new(event_loop: &EventLoopWindowTarget<UserEvent>, proxy: &EventLoopProxy<UserEvent>) -> Self {
        let profile_dir = crate::config::profile_directory();
        let mut web_context = WebContext::new(Some(profile_dir.join("webview")));
        
        let recent_sites = load_recent_sites();
        let bookmarks = load_bookmarks();
        let downloads = load_downloads();
        let current_theme = "dark".to_string();

        let window = tao::window::WindowBuilder::new()
            .with_title("Zenith")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .build(event_loop)
            .unwrap();

        let ui_html = include_str!("ui/ui.html");
        let ui_css = include_str!("ui/ui.css");
        let final_ui_html = Arc::new(ui_html.replace(
            "<head>",
            &format!("<head><style>{}</style>", ui_css),
        ));

        let menu = AppMenu::new(&current_theme);
        menu.init();

        let chrome_proxy = proxy.clone();
        let chrome_protocol_html = final_ui_html.clone();
        let chrome_webview = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_bounds(Self::chrome_bounds(&window))
            .with_url("zenith://assets/ui")
            .with_navigation_handler(|url| is_assets_url(&url))
            .with_custom_protocol("zenith".into(), move |_id, request| {
                handle_zenith_request(chrome_protocol_html.as_str(), request)
            })
            .with_ipc_handler(move |request| {
                crate::ipc::dispatch_ipc_message(request.body(), &chrome_proxy, None);
            })
            .build_as_child(&window)
            .unwrap();

        let palette_proxy = proxy.clone();
        let palette_protocol_html = final_ui_html.clone();
        let palette_webview = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_transparent(true)
            .with_background_color((28, 29, 34, 255))
            .with_visible(false)
            .with_bounds(Self::palette_bounds(&window))
            .with_url("zenith://assets/ui?mode=palette")
            .with_custom_protocol("zenith".into(), move |_id, request| {
                handle_zenith_request(palette_protocol_html.as_str(), request)
            })
            .with_ipc_handler(move |request| {
                crate::ipc::dispatch_ipc_message(request.body(), &palette_proxy, None);
            })
            .build_as_child(&window)
            .unwrap();

        let mut app = Self {
            window,
            web_context,
            tabs: Vec::new(),
            active_tab_id: None,
            next_tab_id: 1,
            chrome_webview,
            palette_webview,
            chrome_ready: false,
            current_theme,
            auth_windows: Vec::new(),
            background_sync_webview: None,
            recent_sites,
            bookmarks,
            downloads,
            menu,
            img_ctx: Arc::new(Mutex::new(None)),
            final_ui_html,
        };

        app.new_tab(None, true, proxy);
        app
    }

    pub fn chrome_bounds(window: &Window) -> Rect {
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        Rect {
            position: LogicalPosition::new(0, 0).into(),
            size: WryLogicalSize::new(size.width.max(1), CHROME_HEIGHT).into(),
        }
    }

    pub fn palette_bounds(window: &Window) -> Rect {
        let size = window.inner_size().to_logical::<f64>(window.scale_factor());
        let width = 640.0;
        let height = 500.0;
        let x = (size.width - width) / 2.0;
        let y = 96.0;
        Rect {
            position: LogicalPosition::new(x.max(0.0) as i32, y as i32).into(),
            size: WryLogicalSize::new(width as u32, height as u32).into(),
        }
    }

    pub fn content_bounds(window: &Window) -> Rect {
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let y = CHROME_HEIGHT.min(size.height);
        let height = size.height.saturating_sub(y).max(1);
        Rect {
            position: LogicalPosition::new(0, y).into(),
            size: WryLogicalSize::new(size.width.max(1), height).into(),
        }
    }

    pub fn update_bounds(&self) {
        let _ = self.chrome_webview.set_bounds(Self::chrome_bounds(&self.window));
        let _ = self.palette_webview.set_bounds(Self::palette_bounds(&self.window));
        let bounds = Self::content_bounds(&self.window);
        for tab in &self.tabs {
            let _ = tab.webview.set_bounds(bounds);
        }
    }

    pub fn new_tab(&mut self, url: Option<String>, activate: bool, proxy: &EventLoopProxy<UserEvent>) {
        let start_url = normalize_user_input_url(url.as_deref().unwrap_or(HOME_URL), "https://www.google.com/search?q={}");

        if let Some(tab) = build_browser_tab(
            &self.window,
            &mut self.web_context,
            self.next_tab_id,
            &start_url,
            Self::content_bounds(&self.window),
            proxy,
            self.final_ui_html.clone(),
        ) {
            Self::apply_theme_to_webview(&tab.webview, &self.current_theme);
            self.tabs.push(tab);
            if activate || self.active_tab_id.is_none() {
                self.active_tab_id = Some(self.next_tab_id);
            }
            
            let new_tab_index = self.tabs.len() - 1;
            self.sync_tab_data(new_tab_index);
            
            self.next_tab_id += 1;
            self.apply_tab_visibility();
            
            // Force the UI layer back to the top of the stack (macOS specific layering)
            #[cfg(target_os = "macos")]
            {
                let _ = self.chrome_webview.set_visible(false);
                let _ = self.chrome_webview.set_visible(true);
            }
            
            if self.chrome_ready {
                self.sync_chrome_state();
            }
        }
    }

    pub fn switch_tab(&mut self, tab_id: u32) {
        if self.tabs.iter().any(|t| t.id == tab_id) {
            self.active_tab_id = Some(tab_id);
            self.apply_tab_visibility();
            #[cfg(target_os = "macos")]
            {
                let _ = self.chrome_webview.set_visible(false);
                let _ = self.chrome_webview.set_visible(true);
            }
            if self.chrome_ready {
                self.sync_chrome_state();
            }
        }
    }

    pub fn close_tab(&mut self, tab_id: Option<u32>, control_flow: &mut ControlFlow) {
        if let Some(close_id) = tab_id.or(self.active_tab_id) {
            if let Some(idx) = self.tabs.iter().position(|t| t.id == close_id) {
                if self.tabs.len() <= 1 {
                    *control_flow = ControlFlow::Exit;
                    return;
                }

                self.tabs.remove(idx);
                if self.active_tab_id == Some(close_id) {
                    let next_idx = idx.saturating_sub(1);
                    self.active_tab_id = self.tabs.get(next_idx).map(|t| t.id);
                }
            }

            self.apply_tab_visibility();
            if self.chrome_ready {
                self.sync_chrome_state();
            }
        }
    }

    pub fn navigate_tab(&mut self, tab_id: Option<u32>, url: String, proxy: &EventLoopProxy<UserEvent>) {
        if let Some(target_id) = tab_id.or(self.active_tab_id) {
            let next_url = normalize_user_input_url(&url, "https://www.google.com/search?q={}");
            if !(next_url.starts_with("zenith://") && !is_assets_url(&next_url)) {
                if should_warmup_youtube_account_sync(&next_url) {
                    let _ = proxy.send_event(UserEvent::OpenBackgroundAuthSync(
                        "https://accounts.google.com/RotateCookiesPage".to_string(),
                    ));
                }
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == target_id) {
                    tab.url = next_url.clone();
                    tab.title = fallback_title_for_url(&next_url);
                    let _ = tab.webview.load_url(&next_url);
                    if self.chrome_ready {
                        self.sync_chrome_state();
                    }
                }
            }
        }
    }

    pub fn tab_action(&self, tab_id: Option<u32>, action: BrowserAction) {
        if let Some(target_id) = tab_id.or(self.active_tab_id) {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == target_id) {
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

    pub fn sync_chrome_state(&self) {
        let state = ChromeState {
            tabs: self.tabs
                .iter()
                .map(|t| {
                    let is_bookmarked = self.bookmarks.iter().any(|b| b.url == t.url);
                    ChromeTabState {
                        id: t.id,
                        title: t.title.clone(),
                        url: t.url.clone(),
                        is_bookmarked,
                        active_permissions: t.active_permissions.clone(),
                    }
                })
                .collect(),
            active_id: self.active_tab_id,
        };

        if let Ok(json) = serde_json::to_string(&state) {
            let js = format!("if(window.zenithSetState) window.zenithSetState({json});");
            let _ = self.chrome_webview.evaluate_script(&js);
            let _ = self.palette_webview.evaluate_script(&js);
        }
    }

    pub fn apply_tab_visibility(&self) {
        for tab in &self.tabs {
            let _ = tab.webview.set_visible(Some(tab.id) == self.active_tab_id);
        }
    }

    pub fn apply_theme_to_webview(webview: &WebView, theme: &str) {
        let theme_json = serde_json::to_string(theme).unwrap_or_else(|_| "\"dark\"".into());
        let js = format!("if(window.__zenithApplyBrowserTheme) window.__zenithApplyBrowserTheme({theme_json});");
        let _ = webview.evaluate_script(&js);
    }

    pub fn sync_tab_data(&self, index: usize) {
        if let Some(tab) = self.tabs.get(index) {
            if tab.url.starts_with(HOME_URL) {
                if let Ok(sites_json) = serde_json::to_string(&self.recent_sites) {
                    let _ = tab.webview.evaluate_script(&format!("window.postMessage({{ type: 'recent-sites', sites: {sites_json} }}, '*');"));
                }
                if let Ok(bookmarks_json) = serde_json::to_string(&self.bookmarks) {
                    let _ = tab.webview.evaluate_script(&format!("window.postMessage({{ type: 'bookmarks-data', bookmarks: {bookmarks_json} }}, '*');"));
                }
            }
            if tab.url.starts_with(HISTORY_URL) {
                if let Ok(history_json) = serde_json::to_string(&self.recent_sites) {
                    let _ = tab.webview.evaluate_script(&format!("window.postMessage({{ type: 'history-data', entries: {history_json} }}, '*');"));
                }
            }
            if tab.url.starts_with(DOWNLOADS_URL) {
                if let Ok(downloads_json) = serde_json::to_string(&self.downloads) {
                    let _ = tab.webview.evaluate_script(&format!("window.postMessage({{ type: 'downloads-data', entries: {downloads_json} }}, '*');"));
                }
            }
        }
    }

    pub fn sync_all_tabs_data(&self) {
        for i in 0..self.tabs.len() {
            self.sync_tab_data(i);
        }
    }

    pub fn toggle_bookmark(&mut self, tab_id: Option<u32>) {
        if let Some(target_id) = tab_id.or(self.active_tab_id)
            && let Some(tab) = self.tabs.iter().find(|t| t.id == target_id)
        {
            let bookmark_url = tab.url.clone();
            let bookmark_title = tab.title.clone();
            let (changed, was_added) = toggle_bookmark(&mut self.bookmarks, &bookmark_url, &bookmark_title);
            if changed {
                save_bookmarks(&self.bookmarks);
                self.sync_chrome_state();
                self.sync_all_tabs_data();
                
                let msg = if was_added { "Added Bookmark" } else { "Bookmark Removed" };
                let toast_type = if was_added { "success" } else { "info" };
                self.show_toast(msg, toast_type);
            }
        }
    }

    pub fn show_toast(&self, message: &str, toast_type: &str) {
        let msg_json = serde_json::to_string(message).unwrap_or_else(|_| "\"\"".into());
        let type_json = serde_json::to_string(toast_type).unwrap_or_else(|_| "\"info\"".into());
        let js = format!("if (window.showToast) window.showToast({msg_json}, {type_json});");
        let _ = self.chrome_webview.evaluate_script(&js);
    }

    pub fn fetch_suggestions(&self, query: String, proxy: EventLoopProxy<UserEvent>) {
        let recent_sites = self.recent_sites.clone();
        let bookmarks = self.bookmarks.clone();
        
        let mut tabs_snapshot = Vec::new();
        for t in &self.tabs {
            tabs_snapshot.push(Suggestion {
                title: t.title.clone(),
                url: Some(t.url.clone()),
                suggestion_type: "tab".to_string(),
                tab_id: Some(t.id),
            });
        }

        std::thread::spawn(move || {
            let query_lc = query.to_lowercase();
            let mut results = Vec::new();

            // 0. Tabs
            for t in tabs_snapshot {
                if t.title.to_lowercase().contains(&query_lc) || t.url.as_ref().map(|u| u.to_lowercase()).unwrap_or_default().contains(&query_lc) {
                    results.push(t);
                }
                if results.len() >= 3 { break; }
            }

            // 1. Bookmarks
            for b in &bookmarks {
                if b.url.to_lowercase().contains(&query_lc) || b.title.to_lowercase().contains(&query_lc) {
                    if !results.iter().any(|r| r.url.as_ref() == Some(&b.url)) {
                        results.push(Suggestion {
                            title: b.title.clone(),
                            url: Some(b.url.clone()),
                            suggestion_type: "bookmark".to_string(),
                            tab_id: None,
                        });
                    }
                }
                if results.len() >= 5 { break; }
            }

            // 2. History
            for s in &recent_sites {
                if results.len() >= 7 { break; }
                if s.url.to_lowercase().contains(&query_lc) || s.title.to_lowercase().contains(&query_lc) {
                    if !results.iter().any(|r| r.url.as_ref() == Some(&s.url)) {
                        results.push(Suggestion {
                            title: s.title.clone(),
                            url: Some(s.url.clone()),
                            suggestion_type: "history".to_string(),
                            tab_id: None,
                        });
                    }
                }
            }

            // 3. Search suggestions
            let client = reqwest::blocking::Client::new();
            let api_url = format!("https://suggestqueries.google.com/complete/search?client=chrome&q={}", utf8_percent_encode(&query, NON_ALPHANUMERIC));
            
            if let Ok(resp) = client.get(api_url).send() {
                if let Ok(json_val) = resp.json::<serde_json::Value>() {
                    if let serde_json::Value::Array(root) = &json_val {
                        if let Some(suggestions_val) = root.get(1) {
                            if let serde_json::Value::Array(suggestions) = suggestions_val {
                                for s in suggestions {
                                    if results.len() >= 10 { break; }
                                    if let serde_json::Value::String(s_str) = s {
                                        results.push(Suggestion {
                                            title: s_str.clone(),
                                            url: None,
                                            suggestion_type: "search".to_string(),
                                            tab_id: None,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = proxy.send_event(UserEvent::SuggestionResults(results));
        });
    }
}
