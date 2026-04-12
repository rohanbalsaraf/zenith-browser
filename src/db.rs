use crate::config::{load_bookmarks, load_recent_sites, BookmarkSite, RecentSite};
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::path::Path;
use std::str::FromStr;

pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(profile_dir: &Path) -> Result<Self, sqlx::Error> {
        let db_path = profile_dir.join("zenith.db");
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", db_path.display()))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

        let pool = SqlitePool::connect_with(options).await?;

        let db = Self { pool };
        db.init().await?;
        Ok(db)
    }

    async fn init(&self) -> Result<(), sqlx::Error> {
        // Create History Table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                visited_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        // Create Bookmarks Table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS bookmarks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        // Create Downloads Table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS downloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                path TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&self.pool)
        .await?;

        // Add index for fast searching
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_history_url ON history(url)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bookmarks_url ON bookmarks(url)")
            .execute(&self.pool)
            .await?;

        // Create Sessions Table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS session_tabs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                title TEXT NOT NULL,
                is_active BOOLEAN NOT NULL,
                position INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn save_session(
        &self,
        tabs: Vec<crate::config::SessionTab>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM session_tabs")
            .execute(&mut *tx)
            .await?;
        for tab in tabs {
            sqlx::query(
                "INSERT INTO session_tabs (url, title, is_active, position) VALUES (?, ?, ?, ?)",
            )
            .bind(&tab.url)
            .bind(&tab.title)
            .bind(tab.is_active)
            .bind(tab.position)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_session(&self) -> Result<Vec<crate::config::SessionTab>, sqlx::Error> {
        sqlx::query_as::<_, crate::config::SessionTab>(
            "SELECT url, title, is_active, position FROM session_tabs ORDER BY position ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn migrate_from_json(&self) -> Result<(), sqlx::Error> {
        // Only migrate if tables are empty
        let history_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM history")
            .fetch_one(&self.pool)
            .await?;
        if history_count.0 == 0 {
            let recent = load_recent_sites();
            for site in recent {
                let _ = self.add_history(&site.url, &site.title).await;
            }
        }

        let bookmark_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM bookmarks")
            .fetch_one(&self.pool)
            .await?;
        if bookmark_count.0 == 0 {
            let bookmarks = load_bookmarks();
            for b in bookmarks {
                let _ = self.add_bookmark(&b.url, &b.title).await;
            }
        }

        Ok(())
    }

    pub async fn add_history(&self, url: &str, title: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO history (url, title, visited_at) VALUES (?, ?, CURRENT_TIMESTAMP)"
        )
        .bind(url)
        .bind(title)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn add_bookmark(&self, url: &str, title: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO bookmarks (url, title) VALUES (?, ?)")
            .bind(url)
            .bind(title)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn remove_bookmark(&self, url: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM bookmarks WHERE url = ?")
            .bind(url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_bookmarks(&self) -> Result<Vec<BookmarkSite>, sqlx::Error> {
        sqlx::query_as::<_, BookmarkSite>(
            "SELECT url, title FROM bookmarks ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_recent_history(&self, limit: i64) -> Result<Vec<RecentSite>, sqlx::Error> {
        sqlx::query_as::<_, RecentSite>(
            "SELECT url, title FROM history ORDER BY visited_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn search_suggestions(
        &self,
        query: &str,
    ) -> Result<Vec<crate::ipc::Suggestion>, sqlx::Error> {
        let pattern = format!("%{}%", query);
        let mut results = Vec::new();

        // Search Bookmarks
        let bookmarks = sqlx::query_as::<_, BookmarkSite>(
            "SELECT url, title FROM bookmarks WHERE url LIKE ? OR title LIKE ? LIMIT 5",
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        for b in bookmarks {
            results.push(crate::ipc::Suggestion {
                title: b.title,
                url: Some(b.url),
                suggestion_type: "bookmark".to_string(),
                tab_id: None,
            });
        }

        // Search History
        let history = sqlx::query_as::<_, RecentSite>(
            "SELECT url, title FROM history WHERE url LIKE ? OR title LIKE ? LIMIT 10",
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&self.pool)
        .await?;

        for h in history {
            if !results.iter().any(|r| r.url.as_ref() == Some(&h.url)) {
                results.push(crate::ipc::Suggestion {
                    title: h.title,
                    url: Some(h.url),
                    suggestion_type: "history".to_string(),
                    tab_id: None,
                });
            }
        }

        Ok(results)
    }

    pub async fn add_download(
        &self,
        url: &str,
        path: &str,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR REPLACE INTO downloads (url, path, status) VALUES (?, ?, ?)")
            .bind(url)
            .bind(path)
            .bind(status)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_download_status(&self, url: &str, status: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE downloads SET status = ? WHERE url = ?")
            .bind(status)
            .bind(url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_downloads(&self) -> Result<Vec<crate::config::DownloadEntry>, sqlx::Error> {
        sqlx::query_as::<_, crate::config::DownloadEntry>(
            "SELECT url, path, status FROM downloads ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn clear_history(&self) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM history")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_downloads(&self) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM downloads")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
