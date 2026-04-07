use serde::{Deserialize, Serialize};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
    dpi::{LogicalPosition, LogicalSize},
};
use wry::{WebViewBuilder, Rect};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "payload")]
enum IpcMsg {
    #[serde(rename = "load_url")]
    LoadUrl(String),
    #[serde(rename = "back")]
    Back,
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "reload")]
    Reload,
}

enum UserEvent {
    Ipc(IpcMsg),
    UrlChanged(String),
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    
    let window = WindowBuilder::new()
        .with_title("Zenith Browser")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .build(&event_loop)
        .unwrap();

    let toolbar_height = 38;

    // UI Assets
    let ui_html = include_str!("ui/ui.html");
    let ui_css = include_str!("ui/ui.css");
    
    // Inject CSS into HTML
    let final_ui_html = ui_html.replace(
        "<link rel=\"stylesheet\" href=\"ui.css\">",
        &format!("<style>{}</style>", ui_css),
    );

    // 1. UI WebView (Address Bar)
    let proxy_ui = proxy.clone();
    let ui_webview = WebViewBuilder::new()
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(window.inner_size().width as f64, toolbar_height as f64).into(),
        })
        .with_html(final_ui_html)
        .with_ipc_handler(move |request| {
            let msg = request.body();
            if let Ok(ipc_msg) = serde_json::from_str::<IpcMsg>(msg) {
                let _ = proxy_ui.send_event(UserEvent::Ipc(ipc_msg));
            }
        })
        .build(&window)?;

    // 2. Content WebView
    let proxy_content = proxy.clone();
    let content_webview = WebViewBuilder::new()
        .with_bounds(Rect {
            position: LogicalPosition::new(0.0, toolbar_height as f64).into(),
            size: LogicalSize::new(window.inner_size().width as f64, (window.inner_size().height - toolbar_height as u32) as f64).into(),
        })
        .with_url("https://www.google.com")
        .with_on_page_load_handler(move |_event, url| {
            let _ = proxy_content.send_event(UserEvent::UrlChanged(url));
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(UserEvent::Ipc(msg)) => match msg {
                IpcMsg::LoadUrl(url) => {
                    let _ = content_webview.load_url(&url);
                }
                IpcMsg::Back => {
                    // Logic for back (requires history or just javascript call)
                    let _ = content_webview.evaluate_script("window.history.back()");
                }
                IpcMsg::Forward => {
                    let _ = content_webview.evaluate_script("window.history.forward()");
                }
                IpcMsg::Reload => {
                    let _ = content_webview.evaluate_script("window.location.reload()");
                }
            },

            Event::UserEvent(UserEvent::UrlChanged(url)) => {
                // Update Address Bar in UI
                let script = format!("window.postMessage(JSON.stringify({{ type: 'update_url', payload: '{}' }}), '*')", url);
                let _ = ui_webview.evaluate_script(&script);
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,

            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                let size = window.inner_size();
                let _ = ui_webview.set_bounds(Rect {
                    position: LogicalPosition::new(0.0, 0.0).into(),
                    size: LogicalSize::new(size.width as f64, toolbar_height as f64).into(),
                });
                
                let _ = content_webview.set_bounds(Rect {
                    position: LogicalPosition::new(0.0, toolbar_height as f64).into(),
                    size: LogicalSize::new(size.width as f64, (size.height - toolbar_height as u32) as f64).into(),
                });
            }

            _ => (),
        }
    });

    Ok(())
}
