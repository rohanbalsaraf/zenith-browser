use serde::{Deserialize, Serialize};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
    dpi::{LogicalPosition, LogicalSize},
};
use wry::{WebViewBuilder, Rect};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct TabInfo {
    id: u32,
    title: String,
    url: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "payload")]
enum IpcMsg {
    #[serde(rename = "load_url")] LoadUrl(String),
    #[serde(rename = "back")] Back,
    #[serde(rename = "forward")] Forward,
    #[serde(rename = "reload")] Reload,
    #[serde(rename = "new_tab")] NewTab,
    #[serde(rename = "switch_tab")] SwitchTab(u32),
    #[serde(rename = "close_tab")] CloseTab(u32),
    #[serde(rename = "new_window")] NewWindow,
    #[serde(rename = "debug")] Debug(String),
}

enum UserEvent {
    Ipc(IpcMsg),
    UrlChanged(u32, String),
    TitleChanged(u32, String),
}

fn main() {
    println!("Zenith Browser starting (Ultimate Multi-Tab Fix)...");

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    
    let window = WindowBuilder::new()
        .with_title("Zenith Browser")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .build(&event_loop).unwrap();

    let s = window.scale_factor();
    let sz: LogicalSize<f64> = window.inner_size().to_logical(s);
    let ui_h = 76.0;

    // Assets
    let ui_html = include_str!("ui/ui.html");
    let ui_css = include_str!("ui/ui.css");
    let home_html = include_str!("ui/home.html");
    let final_ui_html = ui_html.replace("<link rel=\"stylesheet\" href=\"ui.css\">", &format!("<style>{}</style>", ui_css));

    // State
    let mut tabs: HashMap<u32, wry::WebView> = HashMap::new();
    let mut tab_infos: Vec<TabInfo> = Vec::new();
    let mut active_id: u32 = 0;
    let mut next_id: u32 = 0;

    // 1. Build initial Content WebView
    let id = next_id;
    let p_u = proxy.clone();
    let p_t = proxy.clone();
    let content_wv = WebViewBuilder::new()
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, ui_h).into(),
            size: LogicalSize::new(sz.width, sz.height - ui_h).into(),
        })
        .with_html(home_html)
        .with_on_page_load_handler(move |_, url| { let _ = p_u.send_event(UserEvent::UrlChanged(id, url)); })
        .with_document_title_changed_handler(move |title| { let _ = p_t.send_event(UserEvent::TitleChanged(id, title)); })
        .build(&window).unwrap();
    
    tabs.insert(id, content_wv);
    tab_infos.push(TabInfo { id, title: "Zenith Home".into(), url: "about:blank".into() });
    active_id = id;
    next_id += 1;

    // 2. Build UI WebView LAST (Top Layer)
    let proxy_ui = proxy.clone();
    let ui_webview = WebViewBuilder::new()
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(sz.width, ui_h).into(),
        })
        .with_html(final_ui_html)
        .with_ipc_handler(move |request| {
            let msg = request.body();
            if let Ok(ipc_msg) = serde_json::from_str::<IpcMsg>(msg) {
                let _ = proxy_ui.send_event(UserEvent::Ipc(ipc_msg));
            }
        })
        .build(&window).unwrap();

    update_ui(&ui_webview, &tab_infos, active_id);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc(msg)) => match msg {
                IpcMsg::NewTab => {
                    let id = next_id; next_id += 1;
                    let sc = window.scale_factor();
                    let lsz: LogicalSize<f64> = window.inner_size().to_logical(sc);
                    let p_u = proxy.clone(); let p_t = proxy.clone();
                    
                    // Create new tab (this will sadly jump to top on macOS)
                    if let Ok(wv) = WebViewBuilder::new()
                        .with_bounds(Rect { position: LogicalPosition::new(0.0, ui_h).into(), size: LogicalSize::new(lsz.width, lsz.height - ui_h).into() })
                        .with_html(home_html)
                        .with_on_page_load_handler(move |_, url| { let _ = p_u.send_event(UserEvent::UrlChanged(id, url)); })
                        .with_document_title_changed_handler(move |title| { let _ = p_t.send_event(UserEvent::TitleChanged(id, title)); })
                        .build(&window) {
                            if let Some(old) = tabs.get(&active_id) { let _ = old.set_visible(false); }
                            tabs.insert(id, wv);
                            tab_infos.push(TabInfo { id, title: "New Tab".into(), url: "about:blank".into() });
                            active_id = id;
                            
                            // CRITICAL: Bring UI to front by toggling visibility
                            let _ = ui_webview.set_visible(false);
                            let _ = ui_webview.set_visible(true);
                            let _ = ui_webview.focus();
                            
                            update_ui(&ui_webview, &tab_infos, active_id);
                        }
                }
                IpcMsg::SwitchTab(id) => {
                    if id != active_id && tabs.contains_key(&id) {
                        if let Some(wv) = tabs.get(&active_id) { let _ = wv.set_visible(false); }
                        active_id = id;
                        if let Some(wv) = tabs.get(&active_id) { 
                            let _ = wv.set_visible(true); 
                            let _ = wv.focus(); 
                        }
                        
                        // Bring UI to front again
                        let _ = ui_webview.set_visible(false);
                        let _ = ui_webview.set_visible(true);
                        update_ui(&ui_webview, &tab_infos, active_id);
                    }
                }
                IpcMsg::LoadUrl(url) => if let Some(wv) = tabs.get(&active_id) { let _ = wv.load_url(&url); },
                IpcMsg::Back => if let Some(wv) = tabs.get(&active_id) { let _ = wv.evaluate_script("window.history.back()"); },
                IpcMsg::Forward => if let Some(wv) = tabs.get(&active_id) { let _ = wv.evaluate_script("window.history.forward()"); },
                IpcMsg::Reload => if let Some(wv) = tabs.get(&active_id) { let _ = wv.evaluate_script("window.location.reload()"); },
                IpcMsg::CloseTab(id) => {
                    if tabs.len() > 1 {
                        tabs.remove(&id); tab_infos.retain(|t| t.id != id);
                        if active_id == id {
                            active_id = tab_infos[0].id;
                            if let Some(wv) = tabs.get(&active_id) { let _ = wv.set_visible(true); }
                        }
                        update_ui(&ui_webview, &tab_infos, active_id);
                    }
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::UrlChanged(id, url)) => {
                if let Some(info) = tab_infos.iter_mut().find(|t| t.id == id) {
                    info.url = url.clone();
                    if id == active_id { let _ = ui_webview.evaluate_script(&format!("window.zenithUpdateUrl('{}')", url)); }
                }
            }
            Event::UserEvent(UserEvent::TitleChanged(id, title)) => {
                if let Some(info) = tab_infos.iter_mut().find(|t| t.id == id) {
                    info.title = title;
                    update_ui(&ui_webview, &tab_infos, active_id);
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { event: WindowEvent::Resized(ps), .. } => {
                let sc = window.scale_factor();
                let lsz: LogicalSize<f64> = ps.to_logical(sc);
                let _ = ui_webview.set_bounds(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(lsz.width, ui_h).into() });
                for wv in tabs.values() {
                    let _ = wv.set_bounds(Rect { position: LogicalPosition::new(0.0, ui_h).into(), size: LogicalSize::new(lsz.width, lsz.height - ui_h).into() });
                }
            }
            _ => ()
        }
    });
}

fn update_ui(ui: &wry::WebView, infos: &Vec<TabInfo>, active: u32) {
    let json = serde_json::json!({ "tabs": infos, "active_id": active });
    let _ = ui.evaluate_script(&format!("window.zenithUpdateTabs({})", json));
}
