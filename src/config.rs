use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::utils::resolved_tab_title;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct RecentSite {
    pub url: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BookmarkSite {
    pub url: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DownloadEntry {
    pub url: String,
    pub path: String,
    pub status: String,
}

pub fn profile_directory() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".zenith").join("profile")
    } else {
        std::env::temp_dir().join("zenith-profile")
    }
}

pub fn recent_sites_path() -> PathBuf {
    profile_directory().join("recent-sites.json")
}

pub fn bookmarks_path() -> PathBuf {
    profile_directory().join("bookmarks.json")
}

pub fn downloads_path() -> PathBuf {
    profile_directory().join("downloads.json")
}

pub fn load_recent_sites() -> Vec<RecentSite> {
    let path = recent_sites_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<RecentSite>>(&raw).unwrap_or_default()
}

pub fn load_bookmarks() -> Vec<BookmarkSite> {
    let path = bookmarks_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<BookmarkSite>>(&raw).unwrap_or_default()
}

pub fn load_downloads() -> Vec<DownloadEntry> {
    let path = downloads_path();
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<DownloadEntry>>(&raw).unwrap_or_default()
}

pub fn save_recent_sites(recent_sites: &[RecentSite]) {
    let path = recent_sites_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(recent_sites) {
        let _ = fs::write(path, serialized);
    }
}

pub fn save_bookmarks(bookmarks: &[BookmarkSite]) {
    let path = bookmarks_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(bookmarks) {
        let _ = fs::write(path, serialized);
    }
}

pub fn save_downloads(downloads: &[DownloadEntry]) {
    let path = downloads_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(downloads) {
        let _ = fs::write(path, serialized);
    }
}

pub fn upsert_recent_site(recent_sites: &mut Vec<RecentSite>, raw_url: &str, raw_title: &str) -> bool {
    let title = resolved_tab_title(raw_title, raw_url);
    let mut changed = false;

    if let Some(existing) = recent_sites.iter().position(|s| s.url == raw_url) {
        let item = recent_sites.remove(existing);
        if existing != 0 || item.title != title {
            changed = true;
        }
    } else {
        changed = true;
    }

    recent_sites.insert(
        0,
        RecentSite {
            url: raw_url.to_string(),
            title,
        },
    );

    const MAX_RECENT_SITES: usize = 20;
    if recent_sites.len() > MAX_RECENT_SITES {
        recent_sites.truncate(MAX_RECENT_SITES);
        changed = true;
    }

    changed
}

pub fn toggle_bookmark(bookmarks: &mut Vec<BookmarkSite>, raw_url: &str, raw_title: &str) -> (bool, bool) {
    let title = resolved_tab_title(raw_title, raw_url);
    
    if let Some(existing) = bookmarks.iter().position(|s| s.url == raw_url) {
        bookmarks.remove(existing);
        (true, false) // Changed, was_removed
    } else {
        bookmarks.insert(
            0,
            BookmarkSite {
                url: raw_url.to_string(),
                title,
            },
        );
        const MAX_BOOKMARKS: usize = 50;
        if bookmarks.len() > MAX_BOOKMARKS {
            bookmarks.truncate(MAX_BOOKMARKS);
        }
        (true, true) // Changed, was_added
    }
}

pub fn record_download_started(downloads: &mut Vec<DownloadEntry>, url: &str, path: &str) {
    if url.trim().is_empty() {
        return;
    }

    if let Some(existing) = downloads.iter().position(|d| d.url == url) {
        downloads.remove(existing);
    }

    downloads.insert(
        0,
        DownloadEntry {
            url: url.to_string(),
            path: path.to_string(),
            status: "in_progress".to_string(),
        },
    );
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_recent_site_moves_duplicate_to_front() {
        let mut sites = vec![
            RecentSite {
                url: "https://a.com".to_string(),
                title: "A".to_string(),
            },
            RecentSite {
                url: "https://b.com".to_string(),
                title: "B".to_string(),
            },
        ];
        assert!(upsert_recent_site(&mut sites, "https://a.com", "Site A"));
        assert_eq!(sites[0].url, "https://a.com");
        assert_eq!(sites[0].title, "Site A");
        assert_eq!(sites.len(), 2);
    }

    #[test]
    fn record_download_start_and_completion_status() {
        let mut downloads: Vec<DownloadEntry> = Vec::new();
        record_download_started(
            &mut downloads,
            "https://example.com/file.zip",
            "/tmp/file.zip",
        );
        assert_eq!(downloads[0].status, "in_progress");

        record_download_completed(
            &mut downloads,
            "https://example.com/file.zip",
            Some("/tmp/file.zip".to_string()),
            true,
        );
        assert_eq!(downloads[0].status, "completed");
    }
}
    const MAX_DOWNLOADS: usize = 80;
    if downloads.len() > MAX_DOWNLOADS {
        downloads.truncate(MAX_DOWNLOADS);
    }
}

pub fn record_download_completed(
    downloads: &mut Vec<DownloadEntry>,
    url: &str,
    path: Option<String>,
    success: bool,
) {
    let status = if success { "completed" } else { "failed" }.to_string();
    let final_path = path.unwrap_or_default();

    if let Some(existing) = downloads.iter_mut().find(|d| d.url == url) {
        existing.status = status;
        if !final_path.is_empty() {
            existing.path = final_path;
        }
        return;
    }

    downloads.insert(
        0,
        DownloadEntry {
            url: url.to_string(),
            path: final_path,
            status,
        },
    );

    const MAX_DOWNLOADS: usize = 80;
    if downloads.len() > MAX_DOWNLOADS {
        downloads.truncate(MAX_DOWNLOADS);
    }
}
