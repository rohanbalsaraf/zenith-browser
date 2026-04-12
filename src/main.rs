// let_chains is stable in Rust 1.88.0+

mod app;
mod assets;
mod config;
mod db;
mod ipc;
mod menu;
mod tab;
mod ui_handler;
mod utils;

use app::BrowserApp;
use ipc::{BrowserAction, UserEvent};
use muda::ContextMenu;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use utils::{
    fallback_title_for_url, resolved_tab_title, should_track_recent_site,
    should_warmup_youtube_account_sync,
};

#[tokio::main]
async fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let profile_dir = config::profile_directory();
    let db = std::sync::Arc::new(
        db::Database::new(&profile_dir)
            .await
            .expect("Failed to init database"),
    );
    if let Err(e) = db.migrate_from_json().await {
        eprintln!("[DB] Migration failed: {e}");
    }

    let mut app = BrowserApp::new(&event_loop, &proxy, db.clone());
    app.initial_load(&proxy).await;

    event_loop.run(move |event, event_loop_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        if let Ok(m_event) = muda::MenuEvent::receiver().try_recv() {
            let _ = proxy.send_event(UserEvent::MenuAction(m_event.id().clone()));
        }

        if let Event::UserEvent(UserEvent::MenuAction(ref menu_id)) = event {
            if *menu_id == app.menu.m_new_tab.id() {
                app.new_tab(None, true, &proxy, false);
            } else if *menu_id == app.menu.m_new_incognito_tab.id() {
                app.new_tab(None, true, &proxy, true);
            } else if menu_id == app.menu.m_close_tab.id() {
                app.close_tab(app.active_tab_id, control_flow);
            } else if menu_id == app.menu.m_bookmark.id() {
                app.toggle_bookmark(app.active_tab_id);
            } else if menu_id == app.menu.m_history.id() {
                app.new_tab(Some(utils::HISTORY_URL.to_string()), true, &proxy, false);
            } else if menu_id == app.menu.m_downloads.id() {
                app.new_tab(Some(utils::DOWNLOADS_URL.to_string()), true, &proxy, false);
            } else if menu_id == app.menu.m_find.id() {
                let _ = proxy.send_event(UserEvent::OpenFindBar);
            } else if menu_id == app.menu.m_reload.id() {
                app.tab_action(app.active_tab_id, BrowserAction::Reload);
            } else if menu_id == app.menu.m_theme.id() {
                let next = if app.current_theme == "light" { "dark" } else { "light" };
                let _ = proxy.send_event(UserEvent::SettingsChanged { key: "theme".to_string(), value: next.to_string() });
            } else if menu_id == app.menu.m_settings.id() {
                app.new_tab(Some(utils::SETTINGS_URL.to_string()), true, &proxy, false);
            } else if *menu_id == app.menu.img_save.id() {
                let img_data = if let Ok(guard) = app.img_ctx.lock() { guard.clone() } else { None };
                if let Some((url, filename)) = img_data {
                    let _ = proxy.send_event(UserEvent::SaveImage { url, filename });
                }
            } else if *menu_id == app.menu.img_open.id() {
                let img_data = if let Ok(guard) = app.img_ctx.lock() { guard.clone() } else { None };
                if let Some((url, _)) = img_data {
                    app.new_tab(Some(url), true, &proxy, false);
                }
            } else if *menu_id == app.menu.m_inspect.id() {
                if let Some(tab_id) = app.active_tab_id {
                    if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                        tab.webview.open_devtools();
                    }
                }
            }
        }

        match &event {
            Event::UserEvent(UserEvent::GetSuggestions(_)) | Event::UserEvent(UserEvent::SuggestionResults(_)) => {}
            Event::UserEvent(ue) => println!("[IPC] Incoming - {:?}", ue),
            _ => {}
        }

        match event {
            Event::UserEvent(UserEvent::ChromeReady) => {
                app.chrome_ready = true;
                app.sync_chrome_ready(&proxy);
            }
            Event::UserEvent(UserEvent::NewTab { url, activate, is_incognito }) => {
                app.new_tab(url, activate, &proxy, is_incognito);
            }
            Event::UserEvent(UserEvent::SwitchTab(tab_id)) => {
                app.switch_tab(tab_id);
            }
            Event::UserEvent(UserEvent::CloseTab(tab_id)) => {
                app.close_tab(tab_id, control_flow);
            }
            Event::UserEvent(UserEvent::NavigateTab { tab_id, url }) => {
                app.navigate_tab(tab_id, url, &proxy);
            }
            Event::UserEvent(UserEvent::TabAction { tab_id, action }) => {
                app.tab_action(tab_id, action);
            }
            Event::UserEvent(UserEvent::OpenSettingsTab) => {
                app.new_tab(Some(utils::SETTINGS_URL.to_string()), true, &proxy, false);
            }
            Event::UserEvent(UserEvent::OpenHistoryTab) => {
                app.new_tab(Some(utils::HISTORY_URL.to_string()), true, &proxy, false);
            }
            Event::UserEvent(UserEvent::OpenDownloadsTab) => {
                app.new_tab(Some(utils::DOWNLOADS_URL.to_string()), true, &proxy, false);
            }
            Event::UserEvent(UserEvent::BookmarkActiveTab(tab_id)) => {
                app.toggle_bookmark(tab_id);
            }
            Event::UserEvent(UserEvent::DownloadStarted { url, path }) => {
                let db = app.db.clone();
                let url_c = url.clone();
                let path_c = path.clone();
                tokio::spawn(async move {
                    let _ = db.add_download(&url_c, &path_c, "in_progress").await;
                });
                app.sync_all_tabs_data(&proxy);
                let filename = std::path::Path::new(&path).file_name().and_then(|s| s.to_str()).unwrap_or("file");
                app.show_toast(&format!("Downloading {}", filename), "info");
            }
            Event::UserEvent(UserEvent::DownloadCompleted { url, path, success }) => {
                let db = app.db.clone();
                let url_c = url.clone();
                let status = if success { "completed" } else { "failed" };
                tokio::spawn(async move {
                    let _ = db.update_download_status(&url_c, status).await;
                });
                app.sync_all_tabs_data(&proxy);
                let filename = path.as_ref().and_then(|p| std::path::Path::new(p).file_name()).and_then(|s| s.to_str()).unwrap_or("file");
                let (msg, toast_type) = if success {
                    (format!("Finished downloading {}", filename), "success")
                } else {
                    (format!("Failed to download {}", filename), "error")
                };
                app.show_toast(&msg, toast_type);
            }
            Event::UserEvent(UserEvent::ChromeStateResult(json)) => {
                let js = format!("if(window.zenithSetState) window.zenithSetState({json});");
                let _ = app.chrome_webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::TabDataResult { index, payload }) => {
                if let Some(tab) = app.tabs.get(index) {
                    let _ = tab.webview.evaluate_script(&payload);
                }
            }
            Event::UserEvent(UserEvent::ClearHistory) => {
                let db = app.db.clone();
                tokio::spawn(async move {
                    let _ = db.clear_history().await;
                });
                app.sync_all_tabs_data(&proxy);
                app.show_toast("History cleared", "info");
            }
            Event::UserEvent(UserEvent::ClearDownloads) => {
                let db = app.db.clone();
                tokio::spawn(async move {
                    let _ = db.clear_downloads().await;
                });
                app.sync_all_tabs_data(&proxy);
                app.show_toast("Downloads cleared", "info");
            }
            Event::UserEvent(UserEvent::OpenAuthWindow(url)) => {
                if let Ok(auth_window) = tao::window::WindowBuilder::new()
                    .with_title("Zenith Sign In")
                    .with_inner_size(LogicalSize::new(980.0, 760.0))
                    .build(event_loop_target)
                {
                    let auth_proxy = proxy.clone();
                    if let Ok(auth_webview) = wry::WebViewBuilder::new_with_web_context(&mut app.web_context)
                        .with_user_agent(utils::CUSTOM_USER_AGENT)
                        .with_url(&url)
                        .with_initialization_script(&tab::get_user_agent_data_js())
                        .with_new_window_req_handler(move |next, _| {
                            let _ = auth_proxy.send_event(UserEvent::OpenAuthWindow(next));
                            wry::NewWindowResponse::Deny
                        })
                        .build(&auth_window)
                    {
                        app.auth_windows.push(app::AuthWindow {
                            window: auth_window,
                            _webview: auth_webview,
                        });
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenBackgroundAuthSync(url)) => {
                if let Some(bg) = app.background_sync_webview.as_ref() {
                    let _ = bg.load_url(&url);
                } else {
                    let sync_proxy = proxy.clone();
                    if let Ok(bg_webview) = wry::WebViewBuilder::new_with_web_context(&mut app.web_context)
                        .with_user_agent(utils::CUSTOM_USER_AGENT)
                        .with_url(&url)
                        .with_initialization_script(&tab::get_user_agent_data_js())
                        .with_new_window_req_handler(move |next, _| {
                            let _ = sync_proxy.send_event(UserEvent::OpenBackgroundAuthSync(next));
                            wry::NewWindowResponse::Deny
                        })
                        .build_as_child(&app.window)
                    {
                        let _ = bg_webview.set_visible(false);
                        app.background_sync_webview = Some(bg_webview);
                    }
                }
            }
            Event::UserEvent(UserEvent::TabPermissionChanged { tab_id, permission, granted }) => {
                if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == tab_id) {
                    let p = permission.to_lowercase();
                    if granted {
                        if !tab.active_permissions.contains(&p) { tab.active_permissions.push(p); }
                    } else {
                        tab.active_permissions.retain(|perm| perm != &p);
                    }
                    if app.chrome_ready { app.sync_chrome_state(&proxy); }
                    app.save_session();
                }
            }
            Event::UserEvent(UserEvent::TabUrlChanged { tab_id, url }) => {
                if let Some(index) = app.tabs.iter().position(|t| t.id == tab_id) {
                    let tab = &mut app.tabs[index];
                    let old_fallback = fallback_title_for_url(&tab.url);
                    tab.url = url;
                    if tab.title.trim().is_empty() || tab.title == "Zenith" || tab.title == old_fallback {
                        tab.title = fallback_title_for_url(&tab.url);
                    }
                    if should_warmup_youtube_account_sync(&tab.url) {
                        let _ = proxy.send_event(UserEvent::OpenBackgroundAuthSync(
                            "https://accounts.google.com/RotateCookiesPage".to_string(),
                        ));
                    }
                    BrowserApp::apply_theme_to_webview(&tab.webview, &app.current_theme);
                    if should_track_recent_site(&tab.url) {
                        let db = app.db.clone();
                        let url_c = tab.url.clone();
                        let title_c = tab.title.clone();
                        tokio::spawn(async move {
                            let _ = db.add_history(&url_c, &title_c).await;
                        });
                        app.sync_all_tabs_data(&proxy);
                    } else {
                        app.sync_tab_data(index, &proxy);
                    }
                    if app.chrome_ready { app.sync_chrome_state(&proxy); }
                    app.save_session();
                }
            }
            Event::UserEvent(UserEvent::TabTitleChanged { tab_id, title }) => {
                if let Some(index) = app.tabs.iter().position(|t| t.id == tab_id) {
                    let tab = &mut app.tabs[index];
                    tab.title = resolved_tab_title(&title, &tab.url);
                    if should_track_recent_site(&tab.url) {
                        let db = app.db.clone();
                        let url_c = tab.url.clone();
                        let title_c = tab.title.clone();
                        tokio::spawn(async move {
                            let _ = db.add_history(&url_c, &title_c).await;
                        });
                        app.sync_all_tabs_data(&proxy);
                    } else {
                        app.sync_tab_data(index, &proxy);
                    }
                    if app.chrome_ready { app.sync_chrome_state(&proxy); }
                    app.save_session();
                }
            }
            Event::UserEvent(UserEvent::SettingsChanged { key, value }) => {
                if key == "theme" {
                    app.current_theme = value.clone();
                    for tab in &app.tabs { BrowserApp::apply_theme_to_webview(&tab.webview, &app.current_theme); }
                } else if key == "searchEngine" || key == "search_engine" {
                    app.current_search_url = match value.as_str() {
                        "duckduckgo" => "https://duckduckgo.com/?q={}".to_string(),
                        "bing" => "https://www.bing.com/search?q={}".to_string(),
                        _ => "https://www.google.com/search?q={}".to_string(),
                    };
                }
                let k_json = serde_json::to_string(&key).unwrap();
                let v_json = serde_json::to_string(&value).unwrap();
                let _ = app.chrome_webview.evaluate_script(&format!("if(window.zenithApplySetting) window.zenithApplySetting({k_json}, {v_json});"));
            }
            Event::WindowEvent { window_id, event, .. } => match event {
                WindowEvent::CloseRequested if window_id == app.window.id() => { *control_flow = ControlFlow::Exit; }
                WindowEvent::CloseRequested => { app.auth_windows.retain(|w| w.window.id() != window_id); }
                WindowEvent::Resized(_) if window_id == app.window.id() => { app.update_bounds(); }
                _ => {}
            },
            Event::UserEvent(UserEvent::ShowThreeDotsMenu { x, y }) => {
                #[cfg(target_os = "macos")]
                unsafe {
                    use tao::platform::macos::WindowExtMacOS;
                    app.menu.dots_menu.show_context_menu_for_nsview(app.window.ns_view() as _, Some(tao::dpi::Position::Logical(tao::dpi::LogicalPosition::new(x, y))));
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (x, y); // Suppress unused variable warnings
            }
            Event::UserEvent(UserEvent::FindInPage { query, forward }) => {
                if let Some(tab_id) = app.active_tab_id {
                    if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                        let backwards = if forward { "false" } else { "true" };
                        let escaped = serde_json::to_string(&query).unwrap();
                        let _ = tab.webview.evaluate_script(&format!("window.find({}, false, {}, true, false, false, false);", escaped, backwards));
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenFindBar) => {
                if let Some(tab_id) = app.active_tab_id {
                    if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                        let find_js = include_str!("ui/find_bar.js");
                        let _ = tab.webview.evaluate_script(find_js);
                    }
                }
            }
            Event::UserEvent(UserEvent::ImageContextMenu { url, filename, x, y }) => {
                if let Ok(mut guard) = app.img_ctx.lock() { *guard = Some((url, filename)); }
                #[cfg(target_os = "macos")]
                unsafe {
                    use tao::platform::macos::WindowExtMacOS;
                    app.menu.img_menu.show_context_menu_for_nsview(app.window.ns_view() as _, Some(tao::dpi::Position::Logical(tao::dpi::LogicalPosition::new(x, y))));
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (x, y); // Suppress unused variable warnings
            }
            Event::UserEvent(UserEvent::SaveImage { url, filename }) => {
                if let Some(tab_id) = app.active_tab_id {
                    if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                        let escaped_url = serde_json::to_string(&url).unwrap();
                        let escaped_filename = serde_json::to_string(&filename).unwrap();
                        let js = format!(
                            r#"(function() {{
                                fetch({}, {{ cache: 'force-cache' }})
                                    .then(r => r.blob())
                                    .then(blob => {{
                                        const blobUrl = URL.createObjectURL(blob);
                                        const a = document.createElement('a');
                                        a.href = blobUrl;
                                        a.download = {};
                                        document.body.appendChild(a);
                                        a.click();
                                        document.body.removeChild(a);
                                        setTimeout(() => URL.revokeObjectURL(blobUrl), 10000);
                                    }})
                                    .catch(e => {{
                                        console.error('Save failed', e);
                                        // Fallback: simple click if fetch fails (CORS)
                                        const a = document.createElement('a');
                                        a.href = {};
                                        a.download = {};
                                        a.click();
                                    }});
                            }})();"#,
                            escaped_url, escaped_filename, escaped_url, escaped_filename
                        );
                        let _ = tab.webview.evaluate_script(&js);
                    }
                }
            }
            Event::UserEvent(UserEvent::PermissionRequest { tab_id, url, permission, request_id }) => {
                let js = format!("if (window.showPermissionPrompt) window.showPermissionPrompt({}, {}, '{}', '{}');", 
                    tab_id, serde_json::to_string(&url).unwrap(), permission, request_id);
                let _ = app.chrome_webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::PermissionDecision { tab_id, decision, request_id }) => {
                if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                    let js = format!("if (window._zenith_grant_permission) window._zenith_grant_permission('{}', '{}');", request_id, decision);
                    let _ = tab.webview.evaluate_script(&js);
                }
            }
            Event::UserEvent(UserEvent::GetSuggestions(query)) => {
                app.fetch_suggestions(query, proxy.clone());
            }

            Event::UserEvent(UserEvent::SuggestionResults(results)) => {
                let results_json = serde_json::to_string(&results).unwrap();
                // Send to Chrome UI
                let js_chrome = format!("if (window.zenithSetSuggestions) window.zenithSetSuggestions({});", results_json);
                let _ = app.chrome_webview.evaluate_script(&js_chrome);

                // Broadcast to active tab specifically for Home Page unity
                if let Some(tab_id) = app.active_tab_id {
                    if let Some(tab) = app.tabs.iter().find(|t| t.id == tab_id) {
                        let js_tab = format!("window.postMessage({{ type: 'suggestion-results', results: {} }}, '*');", results_json);
                        let _ = tab.webview.evaluate_script(&js_tab);
                    }
                }
            }
            _ => {}
        }
    });
}
