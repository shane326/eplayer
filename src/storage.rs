use crate::model::{PlayHistory, SkipConfig};
use crate::paths;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

pub fn default_db_path() -> PathBuf {
    paths::app_data_dir().join("moontv-client.db")
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        if !path.exists() {
            if let Some(legacy_path) = paths::legacy_current_dir_file("moontv-client.db") {
                if legacy_path.exists() && legacy_path != path {
                    fs::copy(&legacy_path, path).with_context(|| {
                        format!("copy {} to {}", legacy_path.display(), path.display())
                    })?;
                }
            }
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        storage.init()?;
        Ok(storage)
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS skip_configs (
                source TEXT NOT NULL,
                video_id TEXT NOT NULL,
                intro_end_sec INTEGER NOT NULL DEFAULT 0,
                outro_offset_sec INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (source, video_id)
            );

            CREATE TABLE IF NOT EXISTS play_history (
                source TEXT NOT NULL,
                video_id TEXT NOT NULL,
                episode_index INTEGER NOT NULL,
                progress_sec INTEGER NOT NULL DEFAULT 0,
                duration_sec INTEGER NOT NULL DEFAULT 0,
                title TEXT NOT NULL DEFAULT '',
                episode_title TEXT NOT NULL DEFAULT '',
                poster TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (source, video_id)
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            "#,
        )?;
        add_column_if_missing(
            &conn,
            "play_history",
            "duration_sec",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        add_column_if_missing(
            &conn,
            "play_history",
            "episode_title",
            "TEXT NOT NULL DEFAULT ''",
        )?;
        add_column_if_missing(&conn, "play_history", "poster", "TEXT NOT NULL DEFAULT ''")?;
        Ok(())
    }

    pub fn get_skip_config(&self, source: &str, video_id: &str) -> Result<Option<SkipConfig>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.query_row(
            r#"
            SELECT source, video_id, intro_end_sec, outro_offset_sec, enabled, updated_at
            FROM skip_configs
            WHERE source = ?1 AND video_id = ?2
            "#,
            params![source, video_id],
            |row| {
                Ok(SkipConfig {
                    source: row.get(0)?,
                    video_id: row.get(1)?,
                    intro_end_sec: row.get(2)?,
                    outro_offset_sec: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn save_skip_config(&self, config: &SkipConfig) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            r#"
            INSERT INTO skip_configs
                (source, video_id, intro_end_sec, outro_offset_sec, enabled, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(source, video_id) DO UPDATE SET
                intro_end_sec = excluded.intro_end_sec,
                outro_offset_sec = excluded.outro_offset_sec,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
            params![
                config.source,
                config.video_id,
                config.intro_end_sec,
                config.outro_offset_sec,
                i64::from(config.enabled),
                config.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn save_history(&self, history: &PlayHistory) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        delete_same_title_history(&conn, history)?;
        conn.execute(
            r#"
            INSERT INTO play_history
                (source, video_id, episode_index, progress_sec, duration_sec, title, episode_title, poster, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(source, video_id) DO UPDATE SET
                episode_index = excluded.episode_index,
                progress_sec = excluded.progress_sec,
                duration_sec = excluded.duration_sec,
                title = excluded.title,
                episode_title = excluded.episode_title,
                poster = excluded.poster,
                updated_at = excluded.updated_at
            "#,
            params![
                history.source,
                history.video_id,
                history.episode_index,
                history.progress_sec,
                history.duration_sec,
                history.title,
                history.episode_title,
                history.poster,
                history.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_history(&self, limit: i64) -> Result<Vec<PlayHistory>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT source, video_id, episode_index, progress_sec, duration_sec, title, episode_title, poster, updated_at
            FROM play_history
            ORDER BY updated_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(PlayHistory {
                source: row.get(0)?,
                video_id: row.get(1)?,
                episode_index: row.get(2)?,
                progress_sec: row.get(3)?,
                duration_sec: row.get(4)?,
                title: row.get(5)?,
                episode_title: row.get(6)?,
                poster: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_history(&self, source: &str, video_id: &str) -> Result<Option<PlayHistory>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.query_row(
            r#"
            SELECT source, video_id, episode_index, progress_sec, duration_sec, title, episode_title, poster, updated_at
            FROM play_history
            WHERE source = ?1 AND video_id = ?2
            "#,
            params![source, video_id],
            |row| {
                Ok(PlayHistory {
                    source: row.get(0)?,
                    video_id: row.get(1)?,
                    episode_index: row.get(2)?,
                    progress_sec: row.get(3)?,
                    duration_sec: row.get(4)?,
                    title: row.get(5)?,
                    episode_title: row.get(6)?,
                    poster: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn find_history_by_title(&self, title: &str) -> Result<Option<PlayHistory>> {
        let normalized_title = normalized_history_title(title);
        if normalized_title.is_empty() {
            return Ok(None);
        }

        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare(
            r#"
            SELECT source, video_id, episode_index, progress_sec, duration_sec, title, episode_title, poster, updated_at
            FROM play_history
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PlayHistory {
                source: row.get(0)?,
                video_id: row.get(1)?,
                episode_index: row.get(2)?,
                progress_sec: row.get(3)?,
                duration_sec: row.get(4)?,
                title: row.get(5)?,
                episode_title: row.get(6)?,
                poster: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        for row in rows {
            let history = row?;
            if normalized_history_title(&history.title) == normalized_title {
                return Ok(Some(history));
            }
        }
        Ok(None)
    }

    pub fn clear_history(&self) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute("DELETE FROM play_history", [])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            r#"
            INSERT INTO app_settings (key, value)
            VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
            params![key, value],
        )?;
        Ok(())
    }
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|name| name == column) {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn normalized_history_title(title: &str) -> String {
    title
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !"()（）[]【】{}「」『』<>《》".contains(*ch)
                && (ch.is_ascii_alphanumeric() || *ch == '_' || is_cjk(*ch))
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn delete_same_title_history(conn: &Connection, history: &PlayHistory) -> Result<()> {
    let normalized_title = normalized_history_title(&history.title);
    let mut stmt = conn.prepare("SELECT source, video_id, title FROM play_history")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (source, video_id, title) in rows {
        if source == history.source && video_id == history.video_id {
            continue;
        }
        if normalized_history_title(&title) == normalized_title {
            conn.execute(
                "DELETE FROM play_history WHERE source = ?1 AND video_id = ?2",
                params![source, video_id],
            )?;
        }
    }
    Ok(())
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_history_removes_all_rows() {
        let storage = Storage {
            conn: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        };
        storage.init().unwrap();
        storage
            .save_history(&PlayHistory {
                source: "s1".to_string(),
                video_id: "v1".to_string(),
                episode_index: 0,
                progress_sec: 12,
                duration_sec: 120,
                title: "主角".to_string(),
                episode_title: "第1集".to_string(),
                poster: String::new(),
                updated_at: now_ts(),
            })
            .unwrap();
        assert_eq!(storage.list_history(20).unwrap().len(), 1);

        storage.clear_history().unwrap();

        assert!(storage.list_history(20).unwrap().is_empty());
    }

    #[test]
    fn save_history_replaces_same_title_from_other_source() {
        let storage = Storage {
            conn: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        };
        storage.init().unwrap();

        let mut first = PlayHistory {
            source: "s1".to_string(),
            video_id: "v1".to_string(),
            episode_index: 0,
            progress_sec: 12,
            duration_sec: 120,
            title: "错付".to_string(),
            episode_title: "第1集".to_string(),
            poster: String::new(),
            updated_at: now_ts(),
        };
        storage.save_history(&first).unwrap();
        first.source = "s2".to_string();
        first.video_id = "v2".to_string();
        first.episode_index = 4;
        first.progress_sec = 45;
        first.episode_title = "第5集".to_string();
        first.updated_at += 1;
        storage.save_history(&first).unwrap();

        let rows = storage.list_history(20).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "s2");
        assert_eq!(rows[0].video_id, "v2");
        assert_eq!(rows[0].episode_index, 4);
    }

    #[test]
    fn save_history_replaces_normalized_same_title() {
        let storage = Storage {
            conn: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        };
        storage.init().unwrap();

        let mut first = PlayHistory {
            source: "bfzy".to_string(),
            video_id: "1".to_string(),
            episode_index: 7,
            progress_sec: 660,
            duration_sec: 2400,
            title: "错 付".to_string(),
            episode_title: "第8集".to_string(),
            poster: String::new(),
            updated_at: now_ts(),
        };
        storage.save_history(&first).unwrap();
        first.source = "dyttzy".to_string();
        first.video_id = "2".to_string();
        first.title = "错付".to_string();
        first.progress_sec = 1200;
        first.updated_at += 1;
        storage.save_history(&first).unwrap();

        let rows = storage.list_history(20).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "dyttzy");
        assert_eq!(rows[0].progress_sec, 1200);
    }
}
