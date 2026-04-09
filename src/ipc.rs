use serde::{Deserialize, Serialize};
use tao::event_loop::EventLoopProxy;

#[derive(Debug, Clone, Copy, Serialize)]
pub enum BrowserAction {
    Back,
    Forward,
    Reload,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "type")]
    pub suggestion_type: String, // "history", "bookmark", "search", "tab"
    #[serde(default)]
    pub tab_id: Option<u32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChromeTabState {
    pub id: u32,
    pub title: String,
    pub url: String,
    pub is_bookmarked: bool,
    pub active_permissions: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChromeState {
    pub tabs: Vec<ChromeTabState>,
    pub active_id: Option<u32>,
}

pub enum UserEvent {
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
        decision: String,
        request_id: String,
    },
    GetSuggestions(String),
    SuggestionResults(Vec<Suggestion>),
    SuggestionsShown,
    SuggestionsHidden,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(default)]
    pub tab_id: Option<u32>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub forward: Option<bool>,
    #[serde(default)]
    pub filename: Option<String>,
    // Permission related
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub granted: Option<bool>,
}

pub fn dispatch_ipc_message(
    raw: &str,
    proxy: &EventLoopProxy<UserEvent>,
    fallback_tab_id: Option<u32>,
) {
    println!("DEBUG [IPC]: Incoming - {}", raw);
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
        "open_settings_tab" | "open-settings" => {
            let _ = proxy.send_event(UserEvent::OpenSettingsTab);
        }
        "open_history_tab" | "open-history" => {
            let _ = proxy.send_event(UserEvent::OpenHistoryTab);
        }
        "open_downloads_tab" | "open-downloads" => {
            let _ = proxy.send_event(UserEvent::OpenDownloadsTab);
        }
        "bookmark_active_tab" | "bookmark-page" => {
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
                let _ = proxy.send_event(UserEvent::PermissionRequest {
                    tab_id: id,
                    url,
                    permission,
                    request_id,
                });
            }
        }
        "permission_decision" => {
            if let (Some(id), Some(_permission), Some(decision), Some(request_id)) = (message.tab_id, message.permission, message.decision, message.request_id) {
                let _ = proxy.send_event(UserEvent::PermissionDecision {
                    tab_id: id,
                    decision,
                    request_id,
                });
            }
        }
        "get_suggestions" => {
            if let Some(q) = message.query {
                let _ = proxy.send_event(UserEvent::GetSuggestions(q));
            }
        }
        "hide_suggestions" => {
            let _ = proxy.send_event(UserEvent::GetSuggestions("".to_string()));
        }
        "suggestions_shown" => {
            let _ = proxy.send_event(UserEvent::SuggestionsShown);
        }
        "suggestions_hidden" => {
            let _ = proxy.send_event(UserEvent::SuggestionsHidden);
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
        "find_in_page" | "find-in-page" => {
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
