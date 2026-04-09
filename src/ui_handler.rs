use std::borrow::Cow;
use wry::http::{Request, Response, header};

pub fn handle_zenith_request(ui_html: &str, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
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
