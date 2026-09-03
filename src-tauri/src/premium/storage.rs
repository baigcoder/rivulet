//! Premium TV's own SQLite database, kept separate from the Free TV
//! `iptv.db`. The reason for a separate file is the same reason
//! Free TV and Premium TV live in separate Rust modules: a future
//! change to one shouldn't lock the other, and a backup of one
//! shouldn't carry the other.
//!
//! The schema is intentionally simple. Every table is keyed by a
//! provider-internal `channel_id` (the Xtream stream id, or the
//! M3U `tvg-id` / URL fragment). The `connection_id` is the
//! `iptv_premium_connections` row the channel belongs to — a user
//! might have both a Xtream and an M3U provider active in some
//! future build, and the keying is set up for that already.
//!
//! The master key for `encrypted_config` is in
//! `CredentialVault`; `connection_id` is just a short random hex
//! string the UI uses as a stable identity.

use std::path::Path;
use std::sync::Arc;

use rand::RngCore;
use rusqlite::Connection;

use super::crypto::{CredentialVault, EncryptedBlob};
use super::errors::PremiumError;
use super::models::{IPTVCategory, IPTVChannel};
use super::vod_cache::VodCache;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS iptv_premium_connections (
  id TEXT PRIMARY KEY,
  provider_type TEXT NOT NULL,
  display_name TEXT NOT NULL,
  account_name TEXT,
  expires_at TEXT,
  is_trial INTEGER,
  active_connections INTEGER,
  max_connections INTEGER,
  status TEXT NOT NULL DEFAULT 'active',
  inserted_at INTEGER NOT NULL,
  activated_at INTEGER,
  catalog_synced_at INTEGER,
  epg_synced_at INTEGER
);

-- The encrypted provider config (server URL + username + password
-- for Xtream, or the M3U URL for M3U). Never clear text.
CREATE TABLE IF NOT EXISTS iptv_premium_secrets (
  connection_id TEXT PRIMARY KEY,
  encrypted_config BLOB NOT NULL,
  FOREIGN KEY (connection_id) REFERENCES iptv_premium_connections(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS iptv_premium_categories (
  connection_id TEXT NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  country TEXT,
  group_name TEXT,
  PRIMARY KEY (connection_id, id)
);
CREATE INDEX IF NOT EXISTS iptv_premium_categories_name
  ON iptv_premium_categories (connection_id, name);

CREATE TABLE IF NOT EXISTS iptv_premium_channels (
  connection_id TEXT NOT NULL,
  id TEXT NOT NULL,
  name TEXT NOT NULL,
  logo_url TEXT,
  category_id TEXT,
  category_name TEXT,
  country TEXT,
  language TEXT,
  epg_id TEXT,
  stream_type TEXT,
  user_agent TEXT,
  referer TEXT,
  stream_url TEXT,
  sort_key INTEGER NOT NULL DEFAULT 0,
  name_lower TEXT NOT NULL,
  PRIMARY KEY (connection_id, id)
);
CREATE INDEX IF NOT EXISTS iptv_premium_channels_cat
  ON iptv_premium_channels (connection_id, category_id);
CREATE INDEX IF NOT EXISTS iptv_premium_channels_country
  ON iptv_premium_channels (connection_id, country);
CREATE INDEX IF NOT EXISTS iptv_premium_channels_name
  ON iptv_premium_channels (connection_id, name_lower);

CREATE TABLE IF NOT EXISTS iptv_premium_favorites (
  connection_id TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (connection_id, channel_id)
);

CREATE TABLE IF NOT EXISTS iptv_premium_recent (
  connection_id TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  watched_at INTEGER NOT NULL,
  PRIMARY KEY (connection_id, channel_id)
);

-- Cached EPG. Replaced wholesale on a fresh XMLTV import; per-channel
-- reads are served by `get_short_epg` and merged in here.
CREATE TABLE IF NOT EXISTS iptv_premium_epg (
  connection_id TEXT NOT NULL,
  channel_id TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  start_ts INTEGER NOT NULL,
  stop_ts INTEGER,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (connection_id, channel_id, start_ts)
);
CREATE INDEX IF NOT EXISTS iptv_premium_epg_range
  ON iptv_premium_epg (connection_id, channel_id, start_ts);
"#;

/// Columns added after the first schema shipped. `CREATE TABLE IF NOT
/// EXISTS` does nothing to a table that already exists, so a database
/// written by an earlier build keeps its old column set and every
/// `INSERT` naming a new column fails. SQLite has no
/// `ADD COLUMN IF NOT EXISTS`, and it is not worth a `user_version`
/// ladder for four columns: run each `ALTER` and let "duplicate column
/// name" pass. Any other error is a real one and is returned.
const ADDED_COLUMNS: &[(&str, &str)] = &[
    ("iptv_premium_channels", "stream_url TEXT"),
    ("iptv_premium_channels", "sort_key INTEGER NOT NULL DEFAULT 0"),
    ("iptv_premium_connections", "catalog_synced_at INTEGER"),
    ("iptv_premium_connections", "epg_synced_at INTEGER"),
    ("iptv_premium_channels", "quality TEXT"),
    ("iptv_premium_channels", "is_adult INTEGER NOT NULL DEFAULT 0"),
];

fn migrate(conn: &Connection) -> Result<(), PremiumError> {
    for (table, column) in ADDED_COLUMNS {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column}");
        match conn.execute(&sql, []) {
            Ok(_) => {}
            Err(e) if is_duplicate_column(&e) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn is_duplicate_column(e: &rusqlite::Error) -> bool {
    e.to_string().contains("duplicate column name")
}

/// Indexes over columns that `migrate` may have just added. They
/// cannot live in `SCHEMA`, which runs first and would fail on a
/// database whose `iptv_premium_channels` predates `sort_key`:
/// `CREATE TABLE IF NOT EXISTS` leaves an existing table alone, so the
/// column is not there yet at that point.
const POST_MIGRATION_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS iptv_premium_channels_sort
  ON iptv_premium_channels (connection_id, sort_key, name_lower);
"#;

/// Shared, mutex-protected state for the Premium TV module.
///
/// One per process. Lives for the whole run, just like the Free TV
/// `IptvState` and the torrent session. The mutex is the same
/// `db::Mutex` type the Free TV module wraps rusqlite's `!Sync`
/// `Connection` in, so the helpers here can `lock()` the same way
/// the rest of the crate does.
pub struct PremiumState {
    pub db: Arc<crate::iptv::db::Mutex<Connection>>,
    pub vault: CredentialVault,
    /// Set while a catalog import is running. A second `/refresh`
    /// arriving mid-import is answered "already syncing" rather than
    /// starting a competing full download — a 500K-line playlist takes
    /// long enough that an impatient double-click is likely.
    pub syncing: Arc<std::sync::atomic::AtomicBool>,
    /// Xtream VOD lists are one giant download; cache them in-process.
    pub vod_cache: Arc<VodCache>,
}

impl PremiumState {
    pub fn open(db_path: &Path) -> Result<Self, PremiumError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        conn.execute_batch(POST_MIGRATION_INDEXES)?;
        let arc = Arc::new(crate::iptv::db::Mutex::new(conn));
        let vault = CredentialVault::load()?;
        Ok(Self {
            db: arc,
            vault,
            syncing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            vod_cache: Arc::new(VodCache::new()),
        })
    }
}

// ── Plain-text provider config the vault encrypts ─────────────────

/// The shape of the encrypted config blob. Xtream stores the
/// server URL, username, and password; M3U stores the URL.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProviderConfig {
    Xtream {
        server_url: String,
        username: String,
        password: String,
    },
    M3u {
        url: String,
    },
}

impl ProviderConfig {
    pub fn encrypt(&self, vault: &CredentialVault) -> Result<EncryptedBlob, PremiumError> {
        let bytes = serde_json::to_vec(self)
            .map_err(|e| PremiumError::CredentialError(format!("serialize: {e}")))?;
        vault.encrypt(&bytes)
    }

    pub fn decrypt(blob: &EncryptedBlob, vault: &CredentialVault) -> Result<Self, PremiumError> {
        let bytes = vault.decrypt(blob)?;
        serde_json::from_slice(&bytes)
            .map_err(|e| PremiumError::CredentialError(format!("deserialize: {e}")))
    }
}

// ── Connection table helpers ──────────────────────────────────────

/// New connection id — a 16-hex-char random string. Short enough to
/// be a URL fragment, long enough that collisions are not a worry.
pub fn new_connection_id() -> String {
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn insert_connection(
    conn: &Connection,
    id: &str,
    provider_type: &str,
    display_name: &str,
) -> Result<(), PremiumError> {
    let tx = conn.unchecked_transaction()?;
    // Exactly one connection is active at a time. `active_connection`
    // reads the most recently activated row, and leaving an older one
    // marked active would make "which provider am I on" depend on a
    // timestamp comparison instead of a flag.
    tx.execute(
        "UPDATE iptv_premium_connections SET status = 'inactive' WHERE status = 'active'",
        [],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO iptv_premium_connections
         (id, provider_type, display_name, status, inserted_at, activated_at)
         VALUES (?1, ?2, ?3, 'active', strftime('%s','now'), strftime('%s','now'))",
        rusqlite::params![id, provider_type, display_name],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn update_account(
    conn: &Connection,
    id: &str,
    account_name: Option<&str>,
    expires_at: Option<&str>,
    is_trial: Option<bool>,
    active_connections: Option<i64>,
    max_connections: Option<i64>,
) -> Result<(), PremiumError> {
    conn.execute(
        "UPDATE iptv_premium_connections SET
          account_name = COALESCE(?2, account_name),
          expires_at = COALESCE(?3, expires_at),
          is_trial = COALESCE(?4, is_trial),
          active_connections = COALESCE(?5, active_connections),
          max_connections = COALESCE(?6, max_connections)
         WHERE id = ?1",
        rusqlite::params![
            id,
            account_name,
            expires_at,
            is_trial,
            active_connections,
            max_connections,
        ],
    )?;
    Ok(())
}

pub fn set_secret(
    conn: &Connection,
    connection_id: &str,
    blob: &EncryptedBlob,
) -> Result<(), PremiumError> {
    let bytes = blob.encode()?;
    conn.execute(
        "INSERT OR REPLACE INTO iptv_premium_secrets (connection_id, encrypted_config)
         VALUES (?1, ?2)",
        rusqlite::params![connection_id, bytes],
    )?;
    Ok(())
}

pub fn get_secret(
    conn: &Connection,
    connection_id: &str,
) -> Result<Option<EncryptedBlob>, PremiumError> {
    let mut stmt = conn.prepare(
        "SELECT encrypted_config FROM iptv_premium_secrets WHERE connection_id = ?1",
    )?;
    let mut rows = stmt.query([connection_id])?;
    if let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(0)?;
        Ok(Some(EncryptedBlob::decode(&bytes)?))
    }
    else {
        Ok(None)
    }
}

pub fn active_connection(conn: &Connection) -> Result<Option<String>, PremiumError> {
    let mut stmt = conn.prepare(
        "SELECT id FROM iptv_premium_connections
         WHERE status = 'active'
         ORDER BY activated_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    }
    else {
        Ok(None)
    }
}

pub fn get_connection(conn: &Connection, id: &str) -> Result<Option<ConnectionRow>, PremiumError> {
    let mut stmt = conn.prepare(
        "SELECT provider_type, display_name, account_name, expires_at,
                is_trial, active_connections, max_connections, status
         FROM iptv_premium_connections WHERE id = ?1",
    )?;
    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ConnectionRow {
            provider_type: row.get(0)?,
            display_name: row.get(1)?,
            account_name: row.get(2)?,
            expires_at: row.get(3)?,
            is_trial: row.get(4)?,
            active_connections: row.get(5)?,
            max_connections: row.get(6)?,
            status: row.get(7)?,
        }))
    }
    else {
        Ok(None)
    }
}

pub fn delete_connection(conn: &Connection, id: &str) -> Result<(), PremiumError> {
    conn.execute("DELETE FROM iptv_premium_connections WHERE id = ?1", [id])?;
    Ok(())
}

/// One stored provider connection, as `/status` reports it.
///
/// The columns this does *not* carry — `id`, `inserted_at`,
/// `activated_at` — are on the table and are read by SQL (the caller
/// already has the id, and `active_connection` orders by
/// `activated_at`), but nothing in Rust ever looks at them. Selecting
/// them into fields no reader touches is how a struct drifts into a
/// mirror of the schema instead of an answer to a question.
#[derive(Debug, Clone)]
pub struct ConnectionRow {
    pub provider_type: String,
    pub display_name: String,
    pub account_name: Option<String>,
    pub expires_at: Option<String>,
    pub is_trial: Option<bool>,
    pub active_connections: Option<i64>,
    pub max_connections: Option<i64>,
    pub status: String,
}

// ── Channel / category helpers ────────────────────────────────────

pub fn replace_categories(
    conn: &Connection,
    connection_id: &str,
    categories: &[IPTVCategory],
) -> Result<(), PremiumError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM iptv_premium_categories WHERE connection_id = ?1",
        [connection_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO iptv_premium_categories
             (connection_id, id, name, country, group_name)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for c in categories {
            stmt.execute(rusqlite::params![
                connection_id,
                c.id,
                c.name,
                c.country,
                c.group,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn replace_channels(
    conn: &Connection,
    connection_id: &str,
    channels: &[IPTVChannel],
) -> Result<(), PremiumError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM iptv_premium_channels WHERE connection_id = ?1",
        [connection_id],
    )?;
    {
        // `INSERT OR REPLACE`, not `INSERT`: a provider listing the
        // same stream id twice is common enough that a hard failure
        // would reject the whole import over one duplicate row.
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO iptv_premium_channels
             (connection_id, id, name, logo_url, category_id, category_name,
              country, language, epg_id, stream_type, user_agent, referer,
              stream_url, sort_key, name_lower, quality, is_adult)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )?;
        for (i, ch) in channels.iter().enumerate() {
            stmt.execute(rusqlite::params![
                connection_id,
                ch.id,
                ch.name,
                ch.logo_url,
                ch.category_id,
                ch.category_name,
                ch.country,
                ch.language,
                ch.epg_id,
                ch.stream_type,
                ch.user_agent,
                ch.referer,
                ch.stream_url,
                i as i64,
                ch.name.to_lowercase(),
                ch.quality,
                ch.is_adult as i64,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The channel's stream URL and its two upstream headers. The
/// redirector's only read — deliberately narrow, so the handler never
/// has a whole channel row (or a credential) in scope.
pub fn stream_row(
    conn: &Connection,
    connection_id: &str,
    channel_id: &str,
) -> Result<Option<StreamRow>, PremiumError> {
    let mut stmt = conn.prepare(
        "SELECT stream_url, user_agent, referer, quality FROM iptv_premium_channels
         WHERE connection_id = ?1 AND id = ?2",
    )?;
    let mut rows = stmt.query(rusqlite::params![connection_id, channel_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(StreamRow {
            stream_url: row.get(0)?,
            user_agent: row.get(1)?,
            referer: row.get(2)?,
            quality: row.get(3).ok(),
        })),
        None => Ok(None),
    }
}

#[derive(Debug, Clone)]
pub struct StreamRow {
    pub stream_url: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub quality: Option<String>,
}

pub fn provider_type(conn: &Connection, connection_id: &str) -> Result<String, PremiumError> {
    conn.query_row(
        "SELECT provider_type FROM iptv_premium_connections WHERE id = ?1",
        [connection_id],
        |row| row.get(0),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => PremiumError::ProviderNotConnected,
        other => PremiumError::Database(other.to_string()),
    })
}

pub fn mark_catalog_synced(conn: &Connection, connection_id: &str) -> Result<(), PremiumError> {
    conn.execute(
        "UPDATE iptv_premium_connections SET catalog_synced_at = strftime('%s','now')
         WHERE id = ?1",
        [connection_id],
    )?;
    Ok(())
}

pub fn mark_epg_synced(conn: &Connection, connection_id: &str) -> Result<(), PremiumError> {
    conn.execute(
        "UPDATE iptv_premium_connections SET epg_synced_at = strftime('%s','now')
         WHERE id = ?1",
        [connection_id],
    )?;
    Ok(())
}

pub fn sync_times(
    conn: &Connection,
    connection_id: &str,
) -> Result<(Option<i64>, Option<i64>), PremiumError> {
    let row = conn.query_row(
        "SELECT catalog_synced_at, epg_synced_at FROM iptv_premium_connections WHERE id = ?1",
        [connection_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );
    match row {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok((None, None)),
        Err(e) => Err(e.into()),
    }
}

pub fn set_status(conn: &Connection, connection_id: &str, status: &str) -> Result<(), PremiumError> {
    conn.execute(
        "UPDATE iptv_premium_connections SET status = ?2 WHERE id = ?1",
        rusqlite::params![connection_id, status],
    )?;
    Ok(())
}

pub fn category_count(conn: &Connection, connection_id: &str) -> Result<usize, PremiumError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM iptv_premium_categories WHERE connection_id = ?1",
        [connection_id],
        |row| row.get(0),
    )?;
    Ok(n as usize)
}

pub fn channel_count(conn: &Connection, connection_id: &str) -> Result<usize, PremiumError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM iptv_premium_channels WHERE connection_id = ?1",
        [connection_id],
        |row| row.get(0),
    )?;
    Ok(n as usize)
}
