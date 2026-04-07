use reqwest::Url;
use std::borrow::Cow;
use std::io::Read;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{
    WebViewBuilder,
    http::{Request, Response, Uri, header},
};

enum UserEvent {
    UpdateAddressBar(String),
    NewTab,
    Navigate(String),
}

fn is_supported_target_scheme(scheme: &str) -> bool {
    matches!(scheme, "http" | "https")
}

fn default_port_for_scheme(scheme: &str) -> u16 {
    match scheme {
        "http" => 80,
        "https" => 443,
        _ => 0,
    }
}

fn decode_proxy_host(host: &str) -> Option<(String, String, u16)> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 4 || parts.last().copied()? != "z" {
        return None;
    }

    let scheme = parts[0];
    if !is_supported_target_scheme(scheme) {
        return None;
    }

    let port = parts[parts.len() - 2].parse::<u16>().ok()?;
    let target_host = parts[1..parts.len() - 2].join(".");
    if target_host.is_empty() {
        return None;
    }

    Some((scheme.to_string(), target_host, port))
}

fn extract_legacy_target_url(uri: &Uri) -> Option<Url> {
    let query = uri.query()?;
    for pair in query.split('&') {
        if let Some(encoded) = pair.strip_prefix("url=") {
            let decoded = percent_encoding::percent_decode_str(encoded)
                .decode_utf8()
                .ok()?;
            let parsed = Url::parse(decoded.as_ref()).ok()?;
            if is_supported_target_scheme(parsed.scheme()) {
                return Some(parsed);
            }
        }
    }
    None
}

fn extract_target_url(uri: &Uri) -> Option<Url> {
    let host = uri.host()?;

    if host == "proxy" {
        return extract_legacy_target_url(uri);
    }

    let (scheme, target_host, port) = decode_proxy_host(host)?;
    let mut target = format!("{}://{}", scheme, target_host);
    if port != default_port_for_scheme(&scheme) {
        target.push(':');
        target.push_str(&port.to_string());
    }
    target.push_str(uri.path());
    if let Some(query) = uri.query() {
        target.push('?');
        target.push_str(query);
    }

    let parsed = Url::parse(&target).ok()?;
    if !is_supported_target_scheme(parsed.scheme()) {
        return None;
    }
    Some(parsed)
}

fn is_excluded_request_header(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "content-length"
            | "connection"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "cookie"
            | "origin"
            | "referer"
            | "sec-fetch-site"
            | "sec-fetch-mode"
            | "sec-fetch-dest"
            | "sec-fetch-user"
            | "sec-ch-ua"
            | "sec-ch-ua-mobile"
            | "sec-ch-ua-platform"
    )
}

fn is_http_like_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn origin_for_target_url(url: &Url) -> String {
    let scheme = url.scheme();
    let host = url.host_str().unwrap_or_default();
    let port = url
        .port_or_known_default()
        .unwrap_or_else(|| default_port_for_scheme(scheme));
    let default_port = default_port_for_scheme(scheme);
    if port != 0 && port != default_port {
        format!("{scheme}://{host}:{port}")
    } else {
        format!("{scheme}://{host}")
    }
}

fn main() {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Zenith")
        .with_inner_size(LogicalSize::new(1280.0, 820.0))
        .build(&event_loop)
        .unwrap();

    let ui_html = include_str!("ui/ui.html");
    let ui_css = include_str!("ui/ui.css");
    let home_html = include_bytes!("ui/home.html");
    let http_client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_store(true)
        .build()
        .unwrap();

    // Unified Architecture: All assets on zenith://assets/...
    let final_ui_html = ui_html.replace(
        "<link rel=\"stylesheet\" href=\"ui.css\">",
        &format!("<style>{}</style>", ui_css),
    );

    let init_script = r#"
        (function() {
            var send = function(data) { try { window.ipc.postMessage(JSON.stringify(data)); } catch(e) {} };

            var fromProxyUrl = function(raw) {
                try {
                    var u = new URL(raw);
                    if (u.protocol !== 'zenith:') return raw;
                    var host = u.hostname || '';
                    var parts = host.split('.');
                    if (parts.length < 4 || parts[parts.length - 1] !== 'z') return raw;
                    var scheme = parts[0];
                    if (scheme !== 'http' && scheme !== 'https') return raw;
                    var port = parts[parts.length - 2];
                    var targetHost = parts.slice(1, -2).join('.');
                    if (!targetHost) return raw;
                    var defaultPort = scheme === 'https' ? '443' : '80';
                    var portPart = (port && port !== defaultPort) ? (':' + port) : '';
                    return scheme + '://' + targetHost + portPart + (u.pathname || '/') + (u.search || '') + (u.hash || '');
                } catch(_) {
                    return raw;
                }
            };

            var toProxyUrl = function(raw) {
                try {
                    var base = fromProxyUrl(window.location.href);
                    var u = new URL(raw, base);
                    if (u.protocol !== 'http:' && u.protocol !== 'https:') return raw;
                    var port = u.port || (u.protocol === 'https:' ? '443' : '80');
                    return 'zenith://' + u.protocol.slice(0, -1) + '.' + u.hostname + '.' + port + '.z' + (u.pathname || '/') + (u.search || '') + (u.hash || '');
                } catch(_) {
                    return raw;
                }
            };

            var rewriteElementUrl = function(el, attr) {
                try {
                    if (!el || !el.getAttribute) return;
                    var value = el.getAttribute(attr);
                    if (!value) return;
                    var proxied = toProxyUrl(value);
                    if (proxied !== value) el.setAttribute(attr, proxied);
                } catch(_) {}
            };

            var rewriteTree = function(root) {
                if (!root || !root.querySelectorAll) return;
                var selectors = [
                    'a[href]',
                    'form[action]',
                    'img[src]',
                    'script[src]',
                    'link[href]',
                    'source[src]',
                    'video[src]',
                    'audio[src]',
                    'iframe[src]'
                ];
                var nodes = root.querySelectorAll(selectors.join(','));
                for (var i = 0; i < nodes.length; i++) {
                    var el = nodes[i];
                    if (el.hasAttribute('href')) rewriteElementUrl(el, 'href');
                    if (el.hasAttribute('src')) rewriteElementUrl(el, 'src');
                    if (el.hasAttribute('action')) rewriteElementUrl(el, 'action');
                }
            };
            
            // 1. History Interceptor (SPA)
            var hook = function() { send({type:'update_address_bar', url: fromProxyUrl(window.location.href)}); };
            var op = history.pushState; if(op) history.pushState = function(){ op.apply(history, arguments); hook(); };
            var or = history.replaceState; if(or) history.replaceState = function(){ or.apply(history, arguments); hook(); };
            window.addEventListener('popstate', hook);

            // 1b. Intercept fetch / XHR so API calls stay on proxy origin.
            var nativeFetch = window.fetch;
            if (nativeFetch) {
                window.fetch = function(input, init) {
                    try {
                        if (typeof input === 'string') {
                            input = toProxyUrl(input);
                        } else if (input && input.url) {
                            var nextUrl = toProxyUrl(input.url);
                            if (nextUrl !== input.url && typeof Request !== 'undefined') {
                                input = new Request(nextUrl, input);
                            }
                        }
                    } catch (_) {}
                    return nativeFetch.call(this, input, init);
                };
            }

            var nativeOpen = XMLHttpRequest.prototype.open;
            XMLHttpRequest.prototype.open = function(method, url) {
                try { url = toProxyUrl(String(url)); } catch (_) {}
                return nativeOpen.apply(this, arguments.length > 2
                    ? [method, url, arguments[2], arguments[3], arguments[4]]
                    : [method, url]);
            };

            // 2. Click Interceptor
            window.addEventListener('click', function(e) {
                var a = e.target.closest('a[href]');
                if (!a || !a.href) return;
                var next = toProxyUrl(a.href);
                if (next !== a.href) {
                    e.preventDefault();
                    window.location.href = next;
                }
            }, true);

            // 3. Form Interceptor
            window.addEventListener('submit', function(e) {
                var form = e.target;
                if (!form || !form.action) return;
                var next = toProxyUrl(form.action);
                if (next !== form.action) {
                    form.action = next;
                }
            }, true);

            // 4. Rewrite existing and dynamic DOM URL attributes.
            var domReady = function() {
                rewriteTree(document);
                var observer = new MutationObserver(function(mutations) {
                    for (var i = 0; i < mutations.length; i++) {
                        var m = mutations[i];
                        if (m.type === 'attributes' && m.target) {
                            rewriteElementUrl(m.target, m.attributeName);
                        } else if (m.type === 'childList') {
                            for (var j = 0; j < m.addedNodes.length; j++) {
                                var node = m.addedNodes[j];
                                if (node && node.nodeType === 1) {
                                    rewriteTree(node);
                                }
                            }
                        }
                    }
                });
                observer.observe(document.documentElement || document, {
                    subtree: true,
                    childList: true,
                    attributes: true,
                    attributeFilter: ['href', 'src', 'action']
                });
            };
            if (document.readyState === 'loading') {
                document.addEventListener('DOMContentLoaded', domReady, { once: true });
            } else {
                domReady();
            }

            hook();
        })();
    "#;

    // Build the WebView
    let nav_proxy = proxy.clone();
    let popup_proxy = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_url("zenith://assets/ui")
        .with_initialization_script(init_script)
        .with_navigation_handler(move |url| {
            if is_http_like_url(&url) {
                let _ = nav_proxy.send_event(UserEvent::Navigate(url));
                return false;
            }
            true
        })
        .with_new_window_req_handler(move |url, _features| {
            if is_http_like_url(&url) || url.starts_with("zenith://") {
                let _ = popup_proxy.send_event(UserEvent::Navigate(url));
            }
            wry::NewWindowResponse::Deny
        })
        .with_custom_protocol("zenith".into(), move |_id, request: Request<Vec<u8>>| {
            let uri = request.uri();
            let host = uri.host().unwrap_or_default();
            let path = uri.path();

            // 1. Serve UI Assets
            if host == "assets" && (path == "/ui" || path == "/ui/") {
                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Cow::Owned(final_ui_html.as_bytes().to_vec()))
                    .unwrap();
            }
            if host == "assets" && (path == "/home" || path == "/home/") {
                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Cow::Borrowed(home_html as &[u8]))
                    .unwrap();
            }

            // 2. Serve Proxy Content
            if request.method() == "OPTIONS" {
                return Response::builder()
                    .status(204)
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Access-Control-Allow-Headers", "*")
                    .header(
                        "Access-Control-Allow-Methods",
                        "GET,POST,PUT,PATCH,DELETE,OPTIONS",
                    )
                    .body(Cow::Borrowed(&[][..]))
                    .unwrap();
            }

            if let Some(target_url) = extract_target_url(uri) {
                let req_method = reqwest::Method::from_bytes(request.method().as_str().as_bytes())
                    .unwrap_or(reqwest::Method::GET);

                let mut outbound = http_client.request(req_method, target_url.clone());
                let target_origin = origin_for_target_url(&target_url);
                outbound = outbound
                    .header("origin", target_origin.clone())
                    .header("referer", format!("{}/", target_origin));

                for (name, value) in request.headers() {
                    let lower = name.as_str().to_ascii_lowercase();
                    if is_excluded_request_header(&lower) {
                        continue;
                    }
                    if let Ok(v) = value.to_str() {
                        outbound = outbound.header(name.as_str(), v);
                    }
                }

                if !request.body().is_empty() {
                    outbound = outbound.body(request.body().clone());
                }

                if let Ok(mut resp) = outbound.send() {
                    let status = resp.status().as_u16();
                    let mut body = Vec::new();
                    let _ = resp.read_to_end(&mut body);

                    let mime = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("application/octet-stream")
                        .to_string();

                    return Response::builder()
                        .status(status)
                        .header(header::CONTENT_TYPE, mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .header("Access-Control-Allow-Headers", "*")
                        .header(
                            "Access-Control-Allow-Methods",
                            "GET,POST,PUT,PATCH,DELETE,OPTIONS",
                        )
                        .header("X-Frame-Options", "ALLOWALL")
                        .body(Cow::Owned(body))
                        .unwrap();
                }
            }
            Response::builder()
                .status(404)
                .body(Cow::Borrowed(&[][..]))
                .unwrap()
        })
        .with_ipc_handler(move |request: Request<String>| {
            let msg = request.body();
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(msg) {
                if data["type"] == "update_address_bar" {
                    let _ = proxy.send_event(UserEvent::UpdateAddressBar(
                        data["url"].as_str().unwrap_or("").to_string(),
                    ));
                } else if data["type"] == "new_tab" {
                    let _ = proxy.send_event(UserEvent::NewTab);
                } else if data["type"] == "navigate" {
                    let _ = proxy.send_event(UserEvent::Navigate(
                        data["url"].as_str().unwrap_or("").to_string(),
                    ));
                }
            }
        })
        .build(&window)
        .unwrap();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::UpdateAddressBar(url)) => {
                let js_url = serde_json::to_string(&url).unwrap_or_else(|_| "\"\"".to_string());
                let js = format!(
                    "if(window.zenithUpdateAddressBar) window.zenithUpdateAddressBar({});",
                    js_url
                );
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(UserEvent::NewTab) => {
                let _ = webview.evaluate_script("if(window.zenithNewTab) window.zenithNewTab();");
            }
            Event::UserEvent(UserEvent::Navigate(url)) => {
                let js_url = serde_json::to_string(&url).unwrap_or_else(|_| "\"\"".to_string());
                let js = format!(
                    "if(window.zenithNavigate) window.zenithNavigate({});",
                    js_url
                );
                let _ = webview.evaluate_script(&js);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => (),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{decode_proxy_host, extract_target_url};
    use wry::http::Uri;

    #[test]
    fn decode_proxy_host_parses_scheme_host_port() {
        let parsed = decode_proxy_host("https.example.com.443.z");
        assert_eq!(
            parsed,
            Some(("https".to_string(), "example.com".to_string(), 443))
        );
    }

    #[test]
    fn extract_target_url_preserves_path_query() {
        let uri: Uri = "zenith://https.example.com.443.z/A/B/C?x=1&y=2"
            .parse()
            .expect("valid uri");
        let target = extract_target_url(&uri).expect("target url should parse");
        assert_eq!(target.as_str(), "https://example.com/A/B/C?x=1&y=2");
    }
}
