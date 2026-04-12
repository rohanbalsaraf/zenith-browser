use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct RecentSite {
    pub url: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct BookmarkSite {
    pub url: String,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct DownloadEntry {
    pub url: String,
    pub path: String,
    pub status: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
    pub is_active: bool,
    pub position: i32,
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
