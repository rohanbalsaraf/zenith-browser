use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget};
use tao::window::Window;
use wry::{
    dpi::{LogicalPosition, LogicalSize as WryLogicalSize},
    Rect, WebContext, WebView, WebViewBuilder,
};

use crate::db::Database;
use crate::ipc::{BrowserAction, ChromeState, ChromeTabState, Suggestion, UserEvent};
use crate::menu::AppMenu;
use crate::tab::{build_browser_tab, BrowserTab};
use crate::ui_handler::handle_zenith_request;
use crate::utils::{
    fallback_title_for_url, is_assets_url, normalize_user_input_url,
    should_warmup_youtube_account_sync, DOWNLOADS_URL, HISTORY_URL, HOME_URL,
};

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
    pub chrome_ready: bool,
    pub current_theme: String,
    pub auth_windows: Vec<AuthWindow>,
    pub background_sync_webview: Option<WebView>,
    pub menu: AppMenu,
    pub img_ctx: Arc<Mutex<Option<(String, String)>>>,
    pub current_search_url: String,
    pub db: Arc<Database>,
    pub proxy: EventLoopProxy<UserEvent>,
}

impl BrowserApp {
    pub fn new(
        event_loop: &EventLoopWindowTarget<UserEvent>,
        proxy: &EventLoopProxy<UserEvent>,
        db: Arc<Database>,
    ) -> Self {
        let profile_dir = crate::config::profile_directory();
        let mut web_context = WebContext::new(Some(profile_dir.join("webview")));

        let current_theme = "dark".to_string();
        let current_search_url = "https://www.google.com/search?q={}".to_string();

        let window = tao::window::WindowBuilder::new()
            .with_title("Zenith")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .build(event_loop)
            .unwrap();

        let menu = AppMenu::new(&current_theme);
        menu.init();

        let chrome_proxy = proxy.clone();
        let chrome_webview = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_transparent(true)
            .with_background_color((0, 0, 0, 0)) // Glass background
            .with_devtools(true)
            .with_bounds(Self::chrome_bounds(&window))
            .with_url("zenith://assets/ui")
            .with_custom_protocol("zenith".into(), move |_id, request| {
                handle_zenith_request("", request)
            })
            .with_ipc_handler(move |request| {
                crate::ipc::dispatch_ipc_message(request.body(), &chrome_proxy, None);
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
            chrome_ready: false,
            current_theme,
            auth_windows: Vec::new(),
            background_sync_webview: None,
            menu,
            img_ctx: Arc::new(Mutex::new(None)),
            current_search_url,
            db,
            proxy: proxy.clone(),
        };

        app
    }

    pub fn chrome_bounds(window: &Window) -> Rect {
        let _size = window.inner_size().to_logical::<u32>(window.scale_factor());
        Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: window.inner_size().into(),
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
        let _ = self
            .chrome_webview
            .set_bounds(Self::chrome_bounds(&self.window));
        let bounds = Self::content_bounds(&self.window);
        for tab in &self.tabs {
            let _ = tab.webview.set_bounds(bounds);
        }
    }

    pub fn new_tab(
        &mut self,
        url: Option<String>,
        activate: bool,
        proxy: &EventLoopProxy<UserEvent>,
    ) {
        let start_url =
            normalize_user_input_url(url.as_deref().unwrap_or(HOME_URL), &self.current_search_url);

        if let Some(tab) = build_browser_tab(
            &self.window,
            &mut self.web_context,
            self.next_tab_id,
            &start_url,
            Self::content_bounds(&self.window),
            proxy,
        ) {
            Self::apply_theme_to_webview(&tab.webview, &self.current_theme);
            self.tabs.push(tab);
            if activate || self.active_tab_id.is_none() {
                self.active_tab_id = Some(self.next_tab_id);
            }

            let new_tab_index = self.tabs.len() - 1;
            self.sync_tab_data(new_tab_index, &self.proxy);

            self.next_tab_id += 1;
            self.apply_tab_visibility();

            if self.chrome_ready {
                self.sync_chrome_state(&self.proxy);
            }
            self.save_session();
        }
    }

    pub fn switch_tab(&mut self, tab_id: u32) {
        if self.tabs.iter().any(|t| t.id == tab_id) {
            self.active_tab_id = Some(tab_id);
            self.apply_tab_visibility();

            if self.chrome_ready {
                self.sync_chrome_state(&self.proxy);
            }
            self.save_session();
        }
    }

    pub fn close_tab(&mut self, tab_id: Option<u32>, control_flow: &mut ControlFlow) {
        if let Some(close_id) = tab_id.or(self.active_tab_id) {
            if let Some(idx) = self.tabs.iter().position(|t| t.id == close_id) {
                let is_last = self.tabs.len() <= 1;
                
                self.tabs.remove(idx);
                
                if is_last {
                    self.new_tab(None, true, &self.proxy.clone());
                } else if self.active_tab_id == Some(close_id) {
                    let next_idx = idx.saturating_sub(1);
                    self.active_tab_id = self.tabs.get(next_idx).map(|t| t.id);
                }
            }

            self.apply_tab_visibility();
            if self.chrome_ready {
                self.sync_chrome_state(&self.proxy);
            }
            self.save_session();
        }
    }

    pub fn navigate_tab(
        &mut self,
        tab_id: Option<u32>,
        url: String,
        proxy: &EventLoopProxy<UserEvent>,
    ) {
        if let Some(target_id) = tab_id.or(self.active_tab_id) {
            let next_url = normalize_user_input_url(&url, &self.current_search_url);
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
                        self.sync_chrome_state(&self.proxy);
                    }
                    self.save_session();
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

    pub fn sync_chrome_state(&self, proxy: &EventLoopProxy<UserEvent>) {
        let db = self.db.clone();
        let tabs_snapshot = self
            .tabs
            .iter()
            .map(|t| {
                (
                    t.id,
                    t.title.clone(),
                    t.url.clone(),
                    t.active_permissions.clone(),
                )
            })
            .collect::<Vec<_>>();
        let active_id = self.active_tab_id;
        let proxy = proxy.clone();

        tokio::spawn(async move {
            let mut tabs_state = Vec::new();
            for (id, title, url, permissions) in tabs_snapshot {
                let is_bm = sqlx::query("SELECT 1 FROM bookmarks WHERE url = ?")
                    .bind(&url)
                    .fetch_optional(&db.pool)
                    .await
                    .unwrap_or_default()
                    .is_some();

                tabs_state.push(ChromeTabState {
                    id,
                    title,
                    url,
                    is_bookmarked: is_bm,
                    active_permissions: permissions,
                });
            }

            let state = ChromeState {
                tabs: tabs_state,
                active_id,
            };

            if let Ok(json) = serde_json::to_string(&state) {
                let _ = proxy.send_event(UserEvent::ChromeStateResult(json));
            }
        });
    }

    pub fn elevate_ui_layers(&self) {
        #[cfg(target_os = "macos")]
        {
            // Removed disruptive focus() call
        }
    }

    pub fn apply_tab_visibility(&self) {
        for tab in &self.tabs {
            let is_active = Some(tab.id) == self.active_tab_id;
            let _ = tab.webview.set_visible(is_active);
            if is_active {
                let _ = tab.webview.focus();
            }
        }
        self.elevate_ui_layers();
    }

    pub fn apply_theme_to_webview(webview: &WebView, theme: &str) {
        let theme_json = serde_json::to_string(theme).unwrap_or_else(|_| "\"dark\"".into());
        let js = format!(
            "if(window.__zenithApplyBrowserTheme) window.__zenithApplyBrowserTheme({theme_json});"
        );
        let _ = webview.evaluate_script(&js);
    }

    pub fn sync_tab_data(&self, index: usize, proxy: &EventLoopProxy<UserEvent>) {
        if let Some(tab) = self.tabs.get(index) {
            let tab_url = tab.url.clone();
            let theme = self.current_theme.clone();
            let db = self.db.clone();
            let proxy = proxy.clone();

            tokio::spawn(async move {
                let mut scripts = Vec::new();
                // Theme
                if let Ok(theme_json) = serde_json::to_string(&theme) {
                    scripts.push(format!(
                        "window.postMessage({{ type: 'theme', theme: {theme_json} }}, '*');"
                    ));
                }

                if tab_url.starts_with(HOME_URL) {
                    if let Ok(recent) = db.get_recent_history(20).await {
                        if let Ok(sites_json) = serde_json::to_string(&recent) {
                            scripts.push(format!("window.postMessage({{ type: 'recent-sites', sites: {sites_json} }}, '*');"));
                        }
                    }
                    if let Ok(bm) = db.get_bookmarks().await {
                        if let Ok(bookmarks_json) = serde_json::to_string(&bm) {
                            scripts.push(format!("window.postMessage({{ type: 'bookmarks-data', bookmarks: {bookmarks_json} }}, '*');"));
                        }
                    }
                }
                if tab_url.starts_with(HISTORY_URL) {
                    if let Ok(recent) = db.get_recent_history(100).await {
                        if let Ok(history_json) = serde_json::to_string(&recent) {
                            scripts.push(format!("window.postMessage({{ type: 'history-data', entries: {history_json} }}, '*');"));
                        }
                    }
                }
                if tab_url.starts_with(DOWNLOADS_URL) {
                    if let Ok(down) = db.get_downloads().await {
                        if let Ok(downloads_json) = serde_json::to_string(&down) {
                            scripts.push(format!("window.postMessage({{ type: 'downloads-data', entries: {downloads_json} }}, '*');"));
                        }
                    }
                }

                let combined_payload = scripts.join("\n");
                let _ = proxy.send_event(UserEvent::TabDataResult {
                    index,
                    payload: combined_payload,
                });
            });
        }
    }

    pub fn sync_all_tabs_data(&self, proxy: &EventLoopProxy<UserEvent>) {
        for i in 0..self.tabs.len() {
            self.sync_tab_data(i, proxy);
        }
    }

    pub fn sync_chrome_ready(&self, proxy: &EventLoopProxy<UserEvent>) {
        self.sync_chrome_state(proxy);
        self.sync_all_tabs_data(proxy);
    }

    pub fn toggle_bookmark(&mut self, tab_id: Option<u32>) {
        if let Some(target_id) = tab_id.or(self.active_tab_id) {
            if let Some(tab) = self.tabs.iter().find(|t| t.id == target_id) {
                let db = self.db.clone();
                let url = tab.url.clone();
                let title = tab.title.clone();
                let proxy = self.proxy.clone();

                tokio::spawn(async move {
                    let is_bm = sqlx::query("SELECT 1 FROM bookmarks WHERE url = ?")
                        .bind(&url)
                        .fetch_optional(&db.pool)
                        .await
                        .unwrap_or_default()
                        .is_some();

                    let res = if is_bm {
                        db.remove_bookmark(&url).await
                    } else {
                        db.add_bookmark(&url, &title).await
                    };

                    if res.is_ok() {
                        let _ = proxy.send_event(UserEvent::ChromeReady); // Re-sync
                    }
                });

                // Optimistic sync trigger for now
                self.sync_chrome_state(&self.proxy);
                self.sync_all_tabs_data(&self.proxy);
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
        let mut tabs_snapshot = Vec::new();
        for t in &self.tabs {
            tabs_snapshot.push(Suggestion {
                title: t.title.clone(),
                url: Some(t.url.clone()),
                suggestion_type: "tab".to_string(),
                tab_id: Some(t.id),
            });
        }

        let db = self.db.clone();
        tokio::spawn(async move {
            let mut results = match db.search_suggestions(&query).await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[DB] Search error: {e}");
                    Vec::new()
                }
            };

            // Add Tab suggestions
            let query_lc = query.to_lowercase();
            for t in tabs_snapshot {
                if results.len() >= 15 {
                    break;
                }
                if t.title.to_lowercase().contains(&query_lc)
                    || t.url
                        .as_ref()
                        .map(|u| u.to_lowercase())
                        .unwrap_or_default()
                        .contains(&query_lc)
                {
                    if !results.iter().any(|r| r.title == t.title) {
                        results.push(t);
                    }
                }
            }

            let _ = proxy.send_event(UserEvent::SuggestionResults(results));
        });
    }

    pub fn save_session(&self) {
        let db = self.db.clone();
        let active_id = self.active_tab_id;
        let tabs_snapshot = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, t)| crate::config::SessionTab {
                url: t.url.clone(),
                title: t.title.clone(),
                is_active: Some(t.id) == active_id,
                position: idx as i32,
            })
            .collect::<Vec<_>>();

        tokio::spawn(async move {
            if let Err(e) = db.save_session(tabs_snapshot).await {
                eprintln!("[DB] Failed to save session: {e}");
            }
        });
    }

    pub async fn initial_load(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        match self.db.get_session().await {
            Ok(tabs) if !tabs.is_empty() => {
                let mut active_tab_id = None;
                for tab_data in tabs {
                    self.new_tab(Some(tab_data.url), false, proxy);
                    if tab_data.is_active {
                        active_tab_id = self.tabs.last().map(|t| t.id);
                    }
                }
                if let Some(id) = active_tab_id {
                    self.switch_tab(id);
                } else if let Some(last) = self.tabs.last() {
                    self.switch_tab(last.id);
                }
            }
            _ => {
                self.new_tab(None, true, proxy);
            }
        }
    }
}
