use crate::ipc::{dispatch_ipc_message, UserEvent};
use crate::ui_handler::handle_zenith_request;
use crate::utils::{
    is_assets_url, is_background_google_account_sync_url, is_http_like_url,
    should_open_auth_window, CUSTOM_USER_AGENT,
};
use tao::event_loop::EventLoopProxy;
use tao::window::Window;
use wry::{http::Request, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

pub struct BrowserTab {
    pub id: u32,
    pub url: String,
    pub title: String,
    pub webview: WebView,
    pub active_permissions: Vec<String>,
    pub is_incognito: bool,
}

pub fn get_user_agent_data_js() -> String {
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
    "#
    .to_string()
}

pub fn tab_initialization_script(tab_id: u32) -> String {
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
                    try {{
                        var constraints = arguments[0];
                        var type = (constraints && constraints.video) ? 'camera' : 'microphone';
                        
                        var result = await requestPermission(type);
                        if (result === 'granted') {{
                            if (constraints.video) notifyPermission('camera', true);
                            if (constraints.audio) notifyPermission('microphone', true);
                            return originalGUM.apply(navigator.mediaDevices, arguments);
                        }} else {{
                            throw new DOMException("Permission denied by user", "NotAllowedError");
                        }}
                    }} catch (e) {{
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
                    var result = await requestPermission('geolocation');
                    if (result === 'granted') {{
                        notifyPermission('geolocation', true);
                        return originalGCP.apply(navigator.geolocation, arguments);
                    }} else if (error) {{
                        error({{ code: 1, message: "User denied Geolocation" }});
                    }}
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
                        var constraints = arguments[0];
                        var type = (constraints && constraints.video) ? 'camera' : 'microphone';
                        var result = await requestPermission(type);
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

            window.__zenithApplyBrowserTheme = function(theme) {{
                try {{
                    const mode = theme.includes('light') ? 'light' : 'dark';
                    window.__ZENITH_THEME = mode;
                    document.documentElement.style.colorScheme = mode;
                }} catch (_) {{}}
            }};

            if (document.readyState === 'loading') {{
                document.addEventListener('DOMContentLoaded', function() {{
                    notifyUrl();
                }}, {{ once: true }});
            }} else {{
                notifyUrl();
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

pub fn build_browser_tab(
    window: &Window,
    web_context: &mut WebContext,
    tab_id: u32,
    url: &str,
    bounds: Rect,
    proxy: &EventLoopProxy<UserEvent>,
    is_incognito: bool,
) -> Option<BrowserTab> {
    let popup_proxy = proxy.clone();
    let title_proxy = proxy.clone();
    let load_proxy = proxy.clone();
    let ipc_proxy = proxy.clone();
    let download_start_proxy = proxy.clone();
    let download_complete_proxy = proxy.clone();
    let init_script = tab_initialization_script(tab_id);

    let webview_builder = WebViewBuilder::new_with_web_context(web_context)
        .with_incognito(is_incognito)
        .with_user_agent(CUSTOM_USER_AGENT)
        .with_bounds(bounds)
        .with_url(url)
        .with_devtools(true)
        .with_back_forward_navigation_gestures(true)
        .with_initialization_script(&init_script)
        .with_navigation_handler(move |next: String| {
            if next.starts_with("zenith://") {
                return is_assets_url(&next);
            }
            is_http_like_url(&next)
                || next.starts_with("file://")
                || next.starts_with("about:")
                || next.starts_with("data:")
        })
        .with_new_window_req_handler(move |next: String, _features: wry::NewWindowFeatures| {
            let next_url = next.clone();
            if is_background_google_account_sync_url(&next_url) {
                let _ = popup_proxy.send_event(UserEvent::OpenBackgroundAuthSync(next_url));
                wry::NewWindowResponse::Deny
            } else if should_open_auth_window(&next_url) {
                let _ = popup_proxy.send_event(UserEvent::OpenAuthWindow(next_url));
                wry::NewWindowResponse::Deny
            } else if is_http_like_url(&next_url)
                || next_url.starts_with("file://")
                || next_url.starts_with("about:")
                || next_url.starts_with("data:")
            {
                let _ = popup_proxy.send_event(UserEvent::NewTab {
                    url: Some(next_url),
                    activate: true,
                    is_incognito,
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
            if path.as_os_str().is_empty() {
                if let Some(filename) =
                    url.split('/')
                        .last()
                        .and_then(|f: &str| if f.is_empty() { None } else { Some(f) })
                {
                    #[cfg(not(target_os = "windows"))]
                    let dl_dir =
                        dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
                    #[cfg(target_os = "windows")]
                    let dl_dir = dirs::download_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Temp"));
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
        .with_download_completed_handler(
            move |url: String, path: Option<std::path::PathBuf>, success: bool| {
                let _ = download_complete_proxy.send_event(UserEvent::DownloadCompleted {
                    url,
                    path: path.map(|p: std::path::PathBuf| p.to_string_lossy().to_string()),
                    success,
                });
            },
        )
        .with_ipc_handler(move |request| {
            dispatch_ipc_message(request.body(), &ipc_proxy, Some(tab_id));
        })
        .with_custom_protocol("zenith".into(), move |_id, request: Request<Vec<u8>>| {
            handle_zenith_request("", request)
        });

    let webview = webview_builder.build_as_child(window).ok()?;

    Some(BrowserTab {
        id: tab_id,
        url: url.to_string(),
        title: "Zenith".to_string(),
        webview,
        active_permissions: Vec::new(),
        is_incognito,
    })
}
