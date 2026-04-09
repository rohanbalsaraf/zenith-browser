use percent_encoding::{utf8_percent_encode};
pub use percent_encoding::NON_ALPHANUMERIC;
use url::Url;

pub const CUSTOM_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

pub const HOME_URL: &str = "zenith://assets/home";
pub const SETTINGS_URL: &str = "zenith://assets/settings";
pub const HISTORY_URL: &str = "zenith://assets/history";
pub const DOWNLOADS_URL: &str = "zenith://assets/downloads";

pub fn is_http_like_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

pub fn is_assets_url(url: &str) -> bool {
    url.starts_with("zenith://assets/") || url == "zenith://assets"
}

pub fn fallback_title_for_url(raw_url: &str) -> String {
    if raw_url.starts_with(SETTINGS_URL) {
        return "Settings".to_string();
    }
    if raw_url.starts_with(HISTORY_URL) {
        return "History".to_string();
    }
    if raw_url.starts_with(DOWNLOADS_URL) {
        return "Downloads".to_string();
    }
    if raw_url.starts_with(HOME_URL) {
        return "New Tab".to_string();
    }

    if let Ok(url) = Url::parse(raw_url) {
        if let Some(host) = url.host_str() {
            let host = host.strip_prefix("www.").unwrap_or(host);
            if !host.is_empty() {
                return host.to_string();
            }
        }
    }

    "Zenith".to_string()
}

pub fn resolved_tab_title(raw_title: &str, current_url: &str) -> String {
    let trimmed = raw_title.trim();
    if trimmed.is_empty() {
        return fallback_title_for_url(current_url);
    }

    let lower = trimmed.to_ascii_lowercase();
    let generic = matches!(
        lower.as_str(),
        "zenith" | "zenith browser" | "about:blank" | "new tab"
    );
    if generic && is_http_like_url(current_url) {
        return fallback_title_for_url(current_url);
    }

    trimmed.to_string()
}

pub fn normalize_user_input_url(raw: &str, search_engine_url: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return HOME_URL.to_string();
    }

    if trimmed.starts_with("zenith://") || is_http_like_url(trimmed) {
        return trimmed.to_string();
    }

    if (trimmed.contains('.') || trimmed.starts_with("localhost")) && !trimmed.contains(' ') {
        return format!("https://{trimmed}");
    }

    let q = utf8_percent_encode(trimmed, NON_ALPHANUMERIC).to_string();
    search_engine_url.replace("{}", &q)
}

pub fn is_auth_host(host: &str) -> bool {
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

pub fn has_auth_markers(url: &Url) -> bool {
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

pub fn looks_like_oauth_exchange(url: &Url) -> bool {
    let query = url.query().unwrap_or_default().to_ascii_lowercase();
    (query.contains("client_id=") || query.contains("appid=") || query.contains("scope="))
        && (query.contains("redirect_uri=")
            || query.contains("response_type=")
            || query.contains("code_challenge="))
}

pub fn should_open_auth_window(raw_url: &str) -> bool {
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

pub fn is_background_google_account_sync_url(raw_url: &str) -> bool {
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

pub fn should_warmup_youtube_account_sync(raw_url: &str) -> bool {
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

pub fn should_track_recent_site(raw_url: &str) -> bool {
    if !is_http_like_url(raw_url) {
        return false;
    }

    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }

    if host == "accounts.google.com" {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_user_input_uses_https_for_domains() {
        assert_eq!(
            normalize_user_input_url("example.com", "https://www.google.com/search?q={}"),
            "https://example.com".to_string()
        );
    }

    #[test]
    fn normalize_user_input_uses_google_for_queries() {
        let out = normalize_user_input_url("rust browser project", "https://www.google.com/search?q={}");
        assert!(out.starts_with("https://www.google.com/search?"));
        assert!(out.contains("q=rust%20browser%20project"));
    }

    #[test]
    fn fallback_title_uses_hostname() {
        assert_eq!(
            fallback_title_for_url("https://www.youtube.com/watch?v=1"),
            "youtube.com"
        );
        assert_eq!(fallback_title_for_url("zenith://assets/history"), "History");
    }

    #[test]
    fn resolved_title_falls_back_for_generic_browser_title() {
        assert_eq!(
            resolved_tab_title("Zenith", "https://www.youtube.com/"),
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

    #[test]
    fn tracks_recent_sites_only_for_regular_http_pages() {
        assert!(should_track_recent_site("https://example.com/a"));
        assert!(!should_track_recent_site("zenith://assets/home"));
        assert!(!should_track_recent_site(
            "https://accounts.google.com/ServiceLogin"
        ));
    }
}
