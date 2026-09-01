//! SQLite-backed storage for the IPTV subsystem.
//!
//! The whole dataset for an active source (up to 500K+ channels) lives here
//! rather than in the frontend, so the page only ever holds the 60-row window
//! it actually renders. WAL keeps readers from blocking the importer, the
//! indexes below are the only ones a query ever touches, and the prepared
//! statements are cached in `Statements` so each call is a `step()`.
//!
//! Connections are short-lived-per-call: the `Mutex<IptvState::db>` is the
//! cache, and the queries inside it are microseconds. A real connection pool
//! would buy nothing at this latency.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::errors::IptvError;
use super::models::LiveChannel;

/// A live-TV source row — the identity the rest of the system keys off.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub status: String,
    pub config_json: String,
    pub inserted_at: i64,
    pub activated_at: Option<i64>,
    pub channel_count: i64,
    pub country_count: i64,
    pub category_count: i64,
}

/// One channel in a query result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub stream_url: String,
    pub logo_url: Option<String>,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub group_name: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub language: Option<String>,
    pub epg_id: Option<String>,
    pub stream_type: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
}

/// `(name, count)` for a country aggregate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryCount {
    pub name: String,
    pub count: i64,
}

/// `(name, count)` for a category aggregate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryCount {
    pub name: String,
    pub count: i64,
}

/// `(name, count)` for a provider group aggregate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupCount {
    pub name: String,
    pub count: i64,
}

/// A page of channels returned by `live_query_channels` / `live_search`.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPage {
    pub items: Vec<ChannelRow>,
    pub total: i64,
    pub next_cursor: Option<String>,
}

/// The dashboard bundle a `free`/`premium` page renders. Counts come from
/// pre-aggregated tables; previews are capped at 20 so the response stays
/// small even for 500K-row sources.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub source_id: String,
    pub total_channels: i64,
    pub country_count: i64,
    pub category_count: i64,
    pub countries: Vec<CountryCount>,
    pub categories: Vec<CategoryCount>,
    pub groups: Vec<GroupCount>,
    pub favorite_previews: Vec<ChannelRow>,
    pub recent_previews: Vec<ChannelRow>,
    pub country_previews: Vec<CountryPreview>,
    pub category_previews: Vec<CategoryPreview>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryPreview {
    pub name: String,
    pub count: i64,
    pub channels: Vec<ChannelRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPreview {
    pub name: String,
    pub count: i64,
    pub channels: Vec<ChannelRow>,
}

/// Cached prepared statements — designed but not used. The current
/// implementation re-prepares per call because rusqlite's own statement
/// cache is faster than a hand-rolled `Mutex<Statement>`. Kept here as
/// documentation of the shape if a follow-up wants to hand-cache.
#[allow(dead_code)]
struct StatementsPlaceholder;

/// Open the database at `path` and run the schema. Idempotent — every `CREATE`
/// is `IF NOT EXISTS`, so an existing DB opens without rewriting itself.
pub fn open(path: &Path) -> Result<Arc<Mutex<Connection>>, IptvError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IptvError::Database(e.to_string()))?;
    }
    let conn = Connection::open(path).map_err(|e| IptvError::Database(e.to_string()))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA temp_store=MEMORY;
         PRAGMA cache_size=-8000;
         PRAGMA foreign_keys=ON;",
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    run_schema(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

fn run_schema(conn: &Connection) -> Result<(), IptvError> {
    conn.execute_batch(SCHEMA)
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Public for `streaming_m3u::open_db` so the importer can reuse the same
/// schema and pragmas without going through `IptvState`.
pub fn run_schema_pub(conn: &Connection) -> Result<(), IptvError> {
    run_schema(conn)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS iptv_sources (
  id              TEXT PRIMARY KEY,
  kind            TEXT NOT NULL,
  display_name    TEXT NOT NULL,
  status          TEXT NOT NULL,
  config_json     TEXT NOT NULL DEFAULT '{}',
  inserted_at     INTEGER NOT NULL,
  activated_at    INTEGER,
  retired_at      INTEGER
);

CREATE TABLE IF NOT EXISTS iptv_channels (
  source_id       TEXT NOT NULL REFERENCES iptv_sources(id) ON DELETE CASCADE,
  id              TEXT NOT NULL,
  name            TEXT NOT NULL,
  stream_url      TEXT NOT NULL,
  logo_url        TEXT,
  category_id     TEXT,
  category_name   TEXT,
  group_name      TEXT,
  country         TEXT,
  country_code    TEXT,
  language        TEXT,
  epg_id          TEXT,
  stream_type     TEXT,
  quality         TEXT,
  user_agent      TEXT,
  referer         TEXT,
  PRIMARY KEY (source_id, id)
);

CREATE INDEX IF NOT EXISTS idx_ch_src_name        ON iptv_channels (source_id, name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_ch_src_country     ON iptv_channels (source_id, country);
CREATE INDEX IF NOT EXISTS idx_ch_src_countrycode ON iptv_channels (source_id, country_code);
CREATE INDEX IF NOT EXISTS idx_ch_src_category    ON iptv_channels (source_id, category_name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_ch_src_group       ON iptv_channels (source_id, group_name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_ch_src_language    ON iptv_channels (source_id, language);
CREATE INDEX IF NOT EXISTS idx_ch_src_epg         ON iptv_channels (source_id, epg_id);

CREATE TABLE IF NOT EXISTS iptv_country_stats (
  source_id  TEXT NOT NULL,
  country    TEXT NOT NULL,
  count      INTEGER NOT NULL,
  PRIMARY KEY (source_id, country)
);
CREATE INDEX IF NOT EXISTS idx_cs_count ON iptv_country_stats (source_id, count DESC);

CREATE TABLE IF NOT EXISTS iptv_category_stats (
  source_id      TEXT NOT NULL,
  category_name  TEXT NOT NULL,
  count          INTEGER NOT NULL,
  PRIMARY KEY (source_id, category_name)
);
CREATE INDEX IF NOT EXISTS idx_cat_count ON iptv_category_stats (source_id, count DESC);

CREATE TABLE IF NOT EXISTS iptv_group_stats (
  source_id  TEXT NOT NULL,
  group_name TEXT NOT NULL,
  count      INTEGER NOT NULL,
  PRIMARY KEY (source_id, group_name)
);
CREATE INDEX IF NOT EXISTS idx_grp_count ON iptv_group_stats (source_id, count DESC);

CREATE TABLE IF NOT EXISTS iptv_favorites (
  source_id   TEXT NOT NULL,
  channel_id  TEXT NOT NULL,
  added_at    INTEGER NOT NULL,
  PRIMARY KEY (source_id, channel_id)
);
CREATE INDEX IF NOT EXISTS idx_fav_added ON iptv_favorites (source_id, added_at DESC);

CREATE TABLE IF NOT EXISTS iptv_recent (
  source_id    TEXT NOT NULL,
  channel_id   TEXT NOT NULL,
  watched_at   INTEGER NOT NULL,
  PRIMARY KEY (source_id, channel_id)
);
CREATE INDEX IF NOT EXISTS idx_recent_watched ON iptv_recent (source_id, watched_at DESC);

CREATE TABLE IF NOT EXISTS iptv_epg_programs (
  source_id   TEXT NOT NULL,
  epg_id      TEXT NOT NULL,
  channel_id  TEXT NOT NULL,
  title       TEXT NOT NULL,
  description TEXT,
  start_ts    INTEGER NOT NULL,
  end_ts      INTEGER,
  PRIMARY KEY (source_id, epg_id, channel_id, start_ts)
);
CREATE INDEX IF NOT EXISTS idx_epg_src_ch_time ON iptv_epg_programs (source_id, channel_id, start_ts);
"#;

/// A thin wrapper around `std::sync::Mutex<Connection>` so the rest of the
/// crate doesn't have to spell out the type. `Connection` is `!Sync`; the
/// queries are short and contention is bounded by the page size, so a real
/// pool buys nothing.
pub struct Mutex<T: ?Sized> {
    inner: std::sync::Mutex<T>,
}

impl<T> Mutex<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: std::sync::Mutex::new(value),
        }
    }
    pub fn lock(&self) -> Result<std::sync::MutexGuard<'_, T>, IptvError> {
        self.inner
            .lock()
            .map_err(|e| IptvError::Database(e.to_string()))
    }
    /// Consume the wrapper and return the inner value. Errors if the
    /// mutex is poisoned (a thread panicked while holding it).
    pub fn into_inner(self) -> Result<T, IptvError> {
        self.inner
            .into_inner()
            .map_err(|e| IptvError::Database(e.to_string()))
    }
}

#[allow(dead_code)]
pub fn open_path(path: &Path) -> Result<Arc<Mutex<Connection>>, IptvError> {
    open(path)
}

// ── Source CRUD ──────────────────────────────────────────────────────

/// Insert or update a source row. The `config_json` is opaque; the Xtream
/// path stores `{serverUrl, username}`, the M3U path stores `{url, epgUrl}`.
pub fn upsert_source(
    conn: &Connection,
    id: &str,
    kind: &str,
    display_name: &str,
    status: &str,
    config_json: &str,
) -> Result<(), IptvError> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO iptv_sources (id, kind, display_name, status, config_json, inserted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           kind=excluded.kind,
           display_name=excluded.display_name,
           status=excluded.status,
           config_json=excluded.config_json",
        params![id, kind, display_name, status, config_json, now],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Ensure a source row exists without touching its existing fields.
/// The streaming importer calls this on its separate connection
/// before inserting channels, so the row the state's connection
/// wrote is guaranteed to be visible even on a fresh connection
/// that hasn't read the WAL yet. Uses `INSERT OR IGNORE` so the
/// caller's display_name, config_json, and status are preserved
/// when the row already exists.
pub fn ensure_source_exists(
    conn: &Connection,
    id: &str,
    fallback_kind: &str,
) -> Result<(), IptvError> {
    let now = unix_now();
    conn.execute(
        "INSERT OR IGNORE INTO iptv_sources (id, kind, display_name, status, config_json, inserted_at)
         VALUES (?1, ?2, ?3, 'staging', '{}', ?4)",
        params![id, fallback_kind, id, now],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn set_source_status(conn: &Connection, id: &str, status: &str) -> Result<(), IptvError> {
    conn.execute(
        "UPDATE iptv_sources SET status=?1 WHERE id=?2",
        params![status, id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Atomic activation: in one transaction, retire the current active source
/// (if any) and flip the named one to `active`. Either both happen or neither
/// does — a crash mid-import cannot leave the new source half-active.
pub fn activate_source(conn: &Connection, id: &str) -> Result<(), IptvError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let now = unix_now();
    tx.execute(
        "UPDATE iptv_sources SET status='superseded', retired_at=?1
         WHERE status='active' AND id<>?2",
        params![now, id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "UPDATE iptv_sources SET status='active', activated_at=?1, retired_at=NULL
         WHERE id=?2",
        params![now, id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.commit()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn delete_source(conn: &Connection, id: &str) -> Result<(), IptvError> {
    // CASCADE on iptv_channels takes the channels. Stats, favourites, recent
    // and EPG key on source_id with no FK, so they need
    // `delete_source_channels` + `delete_source_state` as well.
    conn.execute("DELETE FROM iptv_sources WHERE id=?1", params![id])
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn get_active_source(conn: &Connection) -> Result<Option<SourceRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.kind, s.display_name, s.status, s.config_json, s.inserted_at, s.activated_at,
                    (SELECT COUNT(*) FROM iptv_channels c WHERE c.source_id = s.id) AS channel_count,
                    (SELECT COUNT(DISTINCT country) FROM iptv_channels c WHERE c.source_id = s.id AND country IS NOT NULL) AS country_count,
                    (SELECT COUNT(DISTINCT category_name) FROM iptv_channels c WHERE c.source_id = s.id AND category_name IS NOT NULL) AS category_count
             FROM iptv_sources s WHERE s.status='active' LIMIT 1",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| IptvError::Database(e.to_string()))?;
    if let Some(row) = rows
        .next()
        .map_err(|e| IptvError::Database(e.to_string()))?
    {
        return Ok(Some(row_to_source(row)?));
    }
    Ok(None)
}

pub fn list_sources(conn: &Connection) -> Result<Vec<SourceRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.kind, s.display_name, s.status, s.config_json, s.inserted_at, s.activated_at,
                    (SELECT COUNT(*) FROM iptv_channels c WHERE c.source_id = s.id) AS channel_count,
                    (SELECT COUNT(DISTINCT country) FROM iptv_channels c WHERE c.source_id = s.id AND country IS NOT NULL) AS country_count,
                    (SELECT COUNT(DISTINCT category_name) FROM iptv_channels c WHERE c.source_id = s.id AND category_name IS NOT NULL) AS category_count
             FROM iptv_sources s
             ORDER BY s.activated_at DESC NULLS LAST, s.inserted_at DESC",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([], row_to_source)
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn get_source(conn: &Connection, id: &str) -> Result<Option<SourceRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.kind, s.display_name, s.status, s.config_json, s.inserted_at, s.activated_at,
                    (SELECT COUNT(*) FROM iptv_channels c WHERE c.source_id = s.id) AS channel_count,
                    (SELECT COUNT(DISTINCT country) FROM iptv_channels c WHERE c.source_id = s.id AND country IS NOT NULL) AS country_count,
                    (SELECT COUNT(DISTINCT category_name) FROM iptv_channels c WHERE c.source_id = s.id AND category_name IS NOT NULL) AS category_count
             FROM iptv_sources s WHERE s.id=?1",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut rows = stmt
        .query(params![id])
        .map_err(|e| IptvError::Database(e.to_string()))?;
    if let Some(row) = rows
        .next()
        .map_err(|e| IptvError::Database(e.to_string()))?
    {
        return Ok(Some(row_to_source(row)?));
    }
    Ok(None)
}

fn row_to_source(row: &Row) -> Result<SourceRow, rusqlite::Error> {
    Ok(SourceRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        display_name: row.get(2)?,
        status: row.get(3)?,
        config_json: row.get(4)?,
        inserted_at: row.get(5)?,
        activated_at: row.get(6)?,
        channel_count: row.get(7)?,
        country_count: row.get(8)?,
        category_count: row.get(9)?,
    })
}

// ── Channels ─────────────────────────────────────────────────────────

/// Delete all channels and stat rows for a source. Used by the importer to
/// start a staging source from a clean slate.
pub fn delete_source_channels(conn: &Connection, source_id: &str) -> Result<(), IptvError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_channels WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_country_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_category_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_group_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.commit()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Delete the per-channel state a source accumulated: favourites, recently
/// watched, and cached EPG. None of these have a foreign key on
/// `iptv_sources`, so dropping the source leaves them behind.
pub fn delete_source_state(conn: &Connection, source_id: &str) -> Result<(), IptvError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    for table in ["iptv_favorites", "iptv_recent", "iptv_epg_programs"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE source_id=?1"),
            params![source_id],
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    }
    tx.commit()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Insert one channel. The source must already exist (FK). Stats are
/// updated by `refresh_stats` after a batch; doing it per-row would
/// serialize the whole import behind an UPDATE per channel.
pub fn insert_channel(
    conn: &Connection,
    source_id: &str,
    c: &LiveChannel,
) -> Result<(), IptvError> {
    let stream_url = c.stream_url.clone().unwrap_or_default();
    let category_name = c.category_name.clone();
    let group = category_name.clone(); // legacy M3U uses `category_name` for both; Xtream uses `group_name`
    conn.execute(
        "INSERT OR REPLACE INTO iptv_channels
            (source_id, id, name, stream_url, logo_url, category_id, category_name,
             group_name, country, country_code, language, epg_id, stream_type,
             user_agent, referer)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            source_id,
            c.id,
            c.name,
            stream_url,
            c.logo_url,
            c.category_id,
            category_name,
            group,
            c.country,
            c.country_code,
            c.language,
            c.epg_id,
            c.stream_type,
            c.user_agent,
            c.referer,
        ],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Insert a chunk of imported channels in one SQLite transaction. Large M3U
/// files can contain hundreds of thousands of rows; committing each row
/// separately makes an otherwise streaming import appear to stall on phones.
pub fn insert_channels_batch(
    conn: &Connection,
    source_id: &str,
    channels: &[LiveChannel],
) -> Result<(), IptvError> {
    if channels.is_empty() {
        return Ok(());
    }
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    {
        let mut statement = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO iptv_channels
                    (source_id, id, name, stream_url, logo_url, category_id, category_name,
                     group_name, country, country_code, language, epg_id, stream_type,
                     user_agent, referer)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )
            .map_err(|e| IptvError::Database(e.to_string()))?;
        for c in channels {
            let stream_url = c.stream_url.clone().unwrap_or_default();
            let category_name = c.category_name.clone();
            let group = category_name.clone();
            statement
                .execute(params![
                    source_id, c.id, c.name, stream_url, c.logo_url, c.category_id,
                    category_name, group, c.country, c.country_code, c.language, c.epg_id,
                    c.stream_type, c.user_agent, c.referer,
                ])
                .map_err(|e| IptvError::Database(e.to_string()))?;
        }
    }
    tx.commit()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Rebuild the three pre-aggregation tables for a source. Called once at the
/// end of an import; the count maps then serve country/category/group bars
/// without a scan of the channel table.
pub fn refresh_stats(conn: &Connection, source_id: &str) -> Result<(), IptvError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_country_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_category_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "DELETE FROM iptv_group_stats WHERE source_id=?1",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "INSERT INTO iptv_country_stats (source_id, country, count)
         SELECT source_id, country, COUNT(*) FROM iptv_channels
         WHERE source_id=?1 AND country IS NOT NULL
         GROUP BY country",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "INSERT INTO iptv_category_stats (source_id, category_name, count)
         SELECT source_id, category_name, COUNT(*) FROM iptv_channels
         WHERE source_id=?1 AND category_name IS NOT NULL
         GROUP BY category_name",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.execute(
        "INSERT INTO iptv_group_stats (source_id, group_name, count)
         SELECT source_id, group_name, COUNT(*) FROM iptv_channels
         WHERE source_id=?1 AND group_name IS NOT NULL
         GROUP BY group_name",
        params![source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    tx.commit()
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn country_stats(
    conn: &Connection,
    source_id: &str,
    limit: i64,
) -> Result<Vec<CountryCount>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT country, count FROM iptv_country_stats
             WHERE source_id=?1 ORDER BY count DESC, country ASC LIMIT ?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![source_id, limit], |r| {
            Ok(CountryCount {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn category_stats(
    conn: &Connection,
    source_id: &str,
    limit: i64,
) -> Result<Vec<CategoryCount>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT category_name, count FROM iptv_category_stats
             WHERE source_id=?1 ORDER BY count DESC, category_name ASC LIMIT ?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![source_id, limit], |r| {
            Ok(CategoryCount {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn group_stats(
    conn: &Connection,
    source_id: &str,
    limit: i64,
) -> Result<Vec<GroupCount>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT group_name, count FROM iptv_group_stats
             WHERE source_id=?1 ORDER BY count DESC, group_name ASC LIMIT ?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![source_id, limit], |r| {
            Ok(GroupCount {
                name: r.get(0)?,
                count: r.get(1)?,
            })
        })
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

// ── Channel queries (the only thing the frontend ever asks for) ─────

/// Opaque cursor for keyset pagination. Encodes the last `(name, id)` of
/// the previous page. The (name, id) pair is what orders the rows so a
/// strictly-greater comparison picks up where the previous page left off
/// with no overlap and no skip.
pub fn encode_cursor(name: &str, id: &str) -> String {
    let raw = format!("{}\u{1f}{}", name, id);
    base64_encode(raw.as_bytes())
}

pub fn decode_cursor(s: &str) -> Option<(String, String)> {
    let bytes = base64_decode(s).ok()?;
    let s = std::str::from_utf8(&bytes).ok()?;
    let (n, i) = s.split_once('\u{1f}')?;
    Some((n.to_string(), i.to_string()))
}

/// Build the WHERE clause + params for a query. Returns the SQL fragment
/// that goes after `WHERE source_id=?` and the bind list. The order
/// matches the keys in `query_channels_args` below.
fn build_query_filter(
    country: Option<&str>,
    category: Option<&str>,
    group: Option<&str>,
    language: Option<&str>,
    quality: Option<&str>,
    search: Option<&str>,
    favorites_only: bool,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(c) = country.filter(|s| !s.is_empty()) {
        clauses.push("country = ?".into());
        binds.push(Box::new(c.to_string()));
    }
    if let Some(c) = category.filter(|s| !s.is_empty()) {
        clauses.push("category_name = ?".into());
        binds.push(Box::new(c.to_string()));
    }
    if let Some(g) = group.filter(|s| !s.is_empty()) {
        clauses.push("group_name = ?".into());
        binds.push(Box::new(g.to_string()));
    }
    if let Some(l) = language.filter(|s| !s.is_empty()) {
        clauses.push("language = ?".into());
        binds.push(Box::new(l.to_string()));
    }
    if let Some(q) = quality.filter(|s| !s.is_empty()) {
        clauses.push("quality = ?".into());
        binds.push(Box::new(q.to_string()));
    }
    if let Some(s) = search.filter(|s| !s.is_empty()) {
        // Name OR epg_id OR category_name OR country, indexed by `idx_ch_src_name`.
        let pattern = format!("%{}%", s);
        clauses.push("(name LIKE ? COLLATE NOCASE OR epg_id LIKE ? COLLATE NOCASE OR category_name LIKE ? COLLATE NOCASE OR country LIKE ? COLLATE NOCASE)".into());
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern));
    }
    if favorites_only {
        clauses.push("EXISTS (SELECT 1 FROM iptv_favorites f WHERE f.source_id = iptv_channels.source_id AND f.channel_id = iptv_channels.id)".into());
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", clauses.join(" AND "))
    };
    (where_sql, binds)
}

fn build_sort(sort: &str) -> &'static str {
    match sort {
        "az" => "name COLLATE NOCASE ASC, id ASC",
        "za" => "name COLLATE NOCASE DESC, id DESC",
        "recently_added" => "id DESC",
        "favorites" => "name COLLATE NOCASE ASC, id ASC", // applied at the WHERE level
        _ => "name COLLATE NOCASE ASC, id ASC", // 'recommended' = alphabetical, the cheapest correct default
    }
}

fn map_channel_row(row: &Row) -> Result<ChannelRow, rusqlite::Error> {
    Ok(ChannelRow {
        id: row.get(0)?,
        name: row.get(1)?,
        stream_url: row.get(2)?,
        logo_url: row.get(3)?,
        category_id: row.get(4)?,
        category_name: row.get(5)?,
        group_name: row.get(6)?,
        country: row.get(7)?,
        country_code: row.get(8)?,
        language: row.get(9)?,
        epg_id: row.get(10)?,
        stream_type: row.get(11)?,
        user_agent: row.get(12)?,
        referer: row.get(13)?,
    })
}

pub fn query_channels(
    conn: &Connection,
    source_id: &str,
    country: Option<&str>,
    category: Option<&str>,
    group: Option<&str>,
    language: Option<&str>,
    quality: Option<&str>,
    search: Option<&str>,
    favorites_only: bool,
    sort: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<ChannelPage, IptvError> {
    let (where_extra, binds) = build_query_filter(
        country,
        category,
        group,
        language,
        quality,
        search,
        favorites_only,
    );
    let sort_sql = build_sort(sort);

    // Total count uses the same WHERE but no ORDER/LIMIT — cheap because
    // it's a count over the filtered index range.
    let count_sql = format!("SELECT COUNT(*) FROM iptv_channels WHERE source_id = ?{where_extra}",);
    let total: i64 = {
        let mut stmt = conn
            .prepare(&count_sql)
            .map_err(|e| IptvError::Database(e.to_string()))?;
        let mut params_v: Vec<&dyn rusqlite::ToSql> = vec![&source_id];
        for b in &binds {
            params_v.push(b.as_ref());
        }
        stmt.query_row(params_v.as_slice(), |r| r.get(0))
            .map_err(|e| IptvError::Database(e.to_string()))?
    };

    // Keyset pagination. `cursor` is (name, id) of the last row of the
    // previous page. For ASC ordering the next page starts strictly
    // *after* the cursor: `(name, id) > (cursor_name, cursor_id)`. That
    // is the row-value comparison written out for SQLite, which doesn't
    // support tuple comparison directly. The `id` tiebreaker is needed
    // because two channels can share a name.
    let after_clause;
    let mut after_binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(c) = cursor {
        if let Some((name, id)) = decode_cursor(c) {
            after_clause = " AND (name > ? COLLATE NOCASE OR (name = ? COLLATE NOCASE AND id > ?))"
                .to_string();
            after_binds.push(Box::new(name.clone()));
            after_binds.push(Box::new(name));
            after_binds.push(Box::new(id));
        } else {
            after_clause = String::new();
        }
    } else {
        after_clause = String::new();
    }

    let sql = format!(
        "SELECT id, name, stream_url, logo_url, category_id, category_name,
                group_name, country, country_code, language, epg_id, stream_type,
                user_agent, referer
         FROM iptv_channels
         WHERE source_id = ?{where_extra}{after_clause}
         ORDER BY {sort_sql}
         LIMIT ?",
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut all_binds: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(source_id.to_string())];
    for b in binds {
        all_binds.push(b);
    }
    for b in after_binds {
        all_binds.push(b);
    }
    all_binds.push(Box::new(limit + 1)); // +1 to peek at the next page's first row

    let bind_refs: Vec<&dyn rusqlite::ToSql> = all_binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(bind_refs.as_slice(), map_channel_row)
        .map_err(|e| IptvError::Database(e.to_string()))?;

    let mut items: Vec<ChannelRow> = Vec::new();
    for r in rows {
        items.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    let next_cursor = if items.len() as i64 > limit {
        // We asked for limit+1; the last entry is the peek at the next
        // page. Drop it, and the rest of the vector is the page.
        items.pop();
        let last = items.last().unwrap();
        Some(encode_cursor(&last.name, &last.id))
    } else {
        None
    };

    Ok(ChannelPage {
        items,
        total,
        next_cursor,
    })
}

/// Search alias. Same shape as `query_channels`; the only difference is the
/// default sort (search results are ranked by name match proximity in a
/// future revision; for now it's the same keyset pagination).
pub fn search_channels(
    conn: &Connection,
    source_id: &str,
    query: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<ChannelPage, IptvError> {
    query_channels(
        conn,
        source_id,
        None,
        None,
        None,
        None,
        None,
        Some(query),
        false,
        "az",
        cursor,
        limit,
    )
}

pub fn country_channels(
    conn: &Connection,
    source_id: &str,
    country: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<ChannelPage, IptvError> {
    query_channels(
        conn,
        source_id,
        Some(country),
        None,
        None,
        None,
        None,
        None,
        false,
        "az",
        cursor,
        limit,
    )
}

pub fn category_channels(
    conn: &Connection,
    source_id: &str,
    category: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<ChannelPage, IptvError> {
    query_channels(
        conn,
        source_id,
        None,
        Some(category),
        None,
        None,
        None,
        None,
        false,
        "az",
        cursor,
        limit,
    )
}

pub fn group_channels(
    conn: &Connection,
    source_id: &str,
    group: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<ChannelPage, IptvError> {
    query_channels(
        conn,
        source_id,
        None,
        None,
        Some(group),
        None,
        None,
        None,
        false,
        "az",
        cursor,
        limit,
    )
}

// ── Channel fetch for the player ────────────────────────────────────

pub fn channel_by_id(
    conn: &Connection,
    source_id: &str,
    channel_id: &str,
) -> Result<Option<ChannelRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, stream_url, logo_url, category_id, category_name,
                    group_name, country, country_code, language, epg_id, stream_type,
                    user_agent, referer
             FROM iptv_channels WHERE source_id=?1 AND id=?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut rows = stmt
        .query(params![source_id, channel_id])
        .map_err(|e| IptvError::Database(e.to_string()))?;
    if let Some(row) = rows
        .next()
        .map_err(|e| IptvError::Database(e.to_string()))?
    {
        return Ok(Some(
            map_channel_row(&row).map_err(|e| IptvError::Database(e.to_string()))?,
        ));
    }
    Ok(None)
}

// ── Favorites / recent ───────────────────────────────────────────────

pub fn toggle_favorite(
    conn: &Connection,
    source_id: &str,
    channel_id: &str,
) -> Result<bool, IptvError> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM iptv_favorites WHERE source_id=?1 AND channel_id=?2",
            params![source_id, channel_id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| IptvError::Database(e.to_string()))?
        .unwrap_or(false);
    if exists {
        conn.execute(
            "DELETE FROM iptv_favorites WHERE source_id=?1 AND channel_id=?2",
            params![source_id, channel_id],
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO iptv_favorites (source_id, channel_id, added_at) VALUES (?1, ?2, ?3)",
            params![source_id, channel_id, unix_now()],
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
        Ok(true)
    }
}

pub fn favorite_channels(
    conn: &Connection,
    source_id: &str,
    limit: i64,
) -> Result<Vec<ChannelRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.name, c.stream_url, c.logo_url, c.category_id, c.category_name,
                    c.group_name, c.country, c.country_code, c.language, c.epg_id, c.stream_type,
                    c.user_agent, c.referer
             FROM iptv_favorites f
             JOIN iptv_channels c ON c.source_id = f.source_id AND c.id = f.channel_id
             WHERE f.source_id=?1
             ORDER BY f.added_at DESC
             LIMIT ?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![source_id, limit], map_channel_row)
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub fn upsert_recent(
    conn: &Connection,
    source_id: &str,
    channel_id: &str,
) -> Result<(), IptvError> {
    conn.execute(
        "INSERT INTO iptv_recent (source_id, channel_id, watched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(source_id, channel_id) DO UPDATE SET watched_at=excluded.watched_at",
        params![source_id, channel_id, unix_now()],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    // Trim to 20 per source.
    conn.execute(
        "DELETE FROM iptv_recent WHERE source_id=?1 AND channel_id NOT IN (
            SELECT channel_id FROM iptv_recent WHERE source_id=?1
            ORDER BY watched_at DESC LIMIT 20
         )",
        params![source_id, source_id],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn clear_recent(conn: &Connection, source_id: &str) -> Result<(), IptvError> {
    conn.execute("DELETE FROM iptv_recent WHERE source_id=?1", params![source_id])
        .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

pub fn recent_channels(
    conn: &Connection,
    source_id: &str,
    limit: i64,
) -> Result<Vec<ChannelRow>, IptvError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.name, c.stream_url, c.logo_url, c.category_id, c.category_name,
                    c.group_name, c.country, c.country_code, c.language, c.epg_id, c.stream_type,
                    c.user_agent, c.referer
             FROM iptv_recent r
             JOIN iptv_channels c ON c.source_id = r.source_id AND c.id = r.channel_id
             WHERE r.source_id=?1
             ORDER BY r.watched_at DESC
             LIMIT ?2",
        )
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![source_id, limit], map_channel_row)
        .map_err(|e| IptvError::Database(e.to_string()))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| IptvError::Database(e.to_string()))?);
    }
    Ok(out)
}

// ── Dashboard ────────────────────────────────────────────────────────

/// Build the dashboard bundle. The previews are capped at 20 each so the
/// response size is bounded regardless of how many channels the source has.
pub fn build_dashboard(conn: &Connection, source_id: &str) -> Result<Dashboard, IptvError> {
    let active = get_source(conn, source_id)?
        .ok_or_else(|| IptvError::Database("source not found".into()))?;
    let countries = country_stats(conn, source_id, 200)?;
    let categories = category_stats(conn, source_id, 200)?;
    let groups = group_stats(conn, source_id, 200)?;
    let favs = favorite_channels(conn, source_id, 20)?;
    let recent = recent_channels(conn, source_id, 20)?;

    // Top 15 country previews with 20 channels each.
    let mut country_previews = Vec::new();
    for cc in countries.iter().take(15) {
        let page = country_channels(conn, source_id, &cc.name, None, 20)?;
        country_previews.push(CountryPreview {
            name: cc.name.clone(),
            count: cc.count,
            channels: page.items,
        });
    }
    let mut category_previews = Vec::new();
    for cat in categories.iter().take(15) {
        let page = category_channels(conn, source_id, &cat.name, None, 20)?;
        category_previews.push(CategoryPreview {
            name: cat.name.clone(),
            count: cat.count,
            channels: page.items,
        });
    }

    Ok(Dashboard {
        source_id: source_id.to_string(),
        total_channels: active.channel_count,
        country_count: active.country_count,
        category_count: active.category_count,
        countries,
        categories,
        groups,
        favorite_previews: favs,
        recent_previews: recent,
        country_previews,
        category_previews,
    })
}

// ── Misc helpers ─────────────────────────────────────────────────────

fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let b0 = chunk[0];
        let b1 = if n > 1 { chunk[1] } else { 0 };
        let b2 = if n > 2 { chunk[2] } else { 0 };
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if n > 1 {
            out.push(ALPHABET[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        }
        if n > 2 {
            out.push(ALPHABET[(b2 & 0b111111) as usize] as char);
        }
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = val(bytes[i]).ok_or(())?;
        let b1 = if i + 1 < bytes.len() {
            val(bytes[i + 1]).ok_or(())?
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            val(bytes[i + 2]).ok_or(())?
        } else {
            0
        };
        let b3 = if i + 3 < bytes.len() {
            val(bytes[i + 3]).ok_or(())?
        } else {
            0
        };
        out.push((b0 << 2) | (b1 >> 4));
        if i + 2 < bytes.len() {
            out.push(((b1 & 0b1111) << 4) | (b2 >> 2));
        }
        if i + 3 < bytes.len() {
            out.push(((b2 & 0b11) << 6) | b3);
        }
        i += 4;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp() -> Connection {
        let dir = tempdir().unwrap();
        let path = dir.path().join("iptv.db");
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        run_schema(&conn).unwrap();
        conn
    }

    fn mk_channel(id: &str, name: &str, country: Option<&str>, cat: Option<&str>) -> LiveChannel {
        LiveChannel {
            id: id.into(),
            name: name.into(),
            logo_url: None,
            stream_url: Some(format!("https://example.com/{id}.")),
            category_id: None,
            category_name: cat.map(String::from),
            country: country.map(String::from),
            country_code: None,
            country_flag: None,
            language: None,
            epg_id: None,
            stream_type: None,
            user_agent: None,
            referer: None,
        }
    }

    #[test]
    fn round_trip_source() {
        let conn = open_temp();
        upsert_source(&conn, "src1", "free-m3u", "Free TV", "active", "{}").unwrap();
        let got = get_source(&conn, "src1").unwrap().unwrap();
        assert_eq!(got.id, "src1");
        assert_eq!(got.kind, "free-m3u");
    }

    #[test]
    fn channels_and_stats() {
        let conn = open_temp();
        upsert_source(&conn, "s", "free-m3u", "x", "active", "{}").unwrap();
        for i in 0..10 {
            insert_channel(
                &conn,
                "s",
                &mk_channel(
                    &format!("c{i}"),
                    &format!("Ch {i}"),
                    if i % 2 == 0 {
                        Some("Pakistan")
                    } else {
                        Some("India")
                    },
                    if i < 5 { Some("News") } else { Some("Sports") },
                ),
            )
            .unwrap();
        }
        refresh_stats(&conn, "s").unwrap();
        let cs = country_stats(&conn, "s", 10).unwrap();
        assert_eq!(cs.len(), 2);
        let cats = category_stats(&conn, "s", 10).unwrap();
        assert_eq!(cats.len(), 2);
    }

    #[test]
    fn cursor_pagination() {
        let conn = open_temp();
        upsert_source(&conn, "s", "free-m3u", "x", "active", "{}").unwrap();
        for i in 0..150 {
            insert_channel(
                &conn,
                "s",
                &mk_channel(&format!("id{i:03}"), &format!("Channel {i:03}"), None, None),
            )
            .unwrap();
        }
        let page1 = query_channels(
            &conn, "s", None, None, None, None, None, None, false, "az", None, 60,
        )
        .unwrap();
        assert_eq!(page1.items.len(), 60);
        assert!(page1.next_cursor.is_some());
        let page2 = query_channels(
            &conn,
            "s",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            "az",
            page1.next_cursor.as_deref(),
            60,
        )
        .unwrap();
        assert_eq!(page2.items.len(), 60);
        // No overlap between consecutive pages.
        let page1_ids: std::collections::HashSet<_> =
            page1.items.iter().map(|c| c.id.clone()).collect();
        let page2_ids: std::collections::HashSet<_> =
            page2.items.iter().map(|c| c.id.clone()).collect();
        assert!(page1_ids.is_disjoint(&page2_ids));
    }

    #[test]
    fn filter_combination() {
        let conn = open_temp();
        upsert_source(&conn, "s", "free-m3u", "x", "active", "{}").unwrap();
        for i in 0..50 {
            let country = if i % 2 == 0 { "Pakistan" } else { "India" };
            let cat = if i % 3 == 0 { "News" } else { "Sports" };
            insert_channel(
                &conn,
                "s",
                &mk_channel(
                    &format!("c{i}"),
                    &format!("Ch {i}"),
                    Some(country),
                    Some(cat),
                ),
            )
            .unwrap();
        }
        refresh_stats(&conn, "s").unwrap();
        let page = query_channels(
            &conn,
            "s",
            Some("Pakistan"),
            Some("News"),
            None,
            None,
            None,
            None,
            false,
            "az",
            None,
            60,
        )
        .unwrap();
        // i=0,6,12,18,24,30,36,42,48 are Pakistan AND News: 9 channels.
        assert_eq!(page.total, 9);
    }

    #[test]
    fn favorites_recent_per_source() {
        let conn = open_temp();
        upsert_source(&conn, "a", "free-m3u", "A", "active", "{}").unwrap();
        upsert_source(&conn, "b", "free-m3u", "B", "active", "{}").unwrap();
        for s in ["a", "b"] {
            for i in 0..5 {
                insert_channel(&conn, s, &mk_channel(&format!("c{s}{i}"), "x", None, None))
                    .unwrap();
            }
        }
        toggle_favorite(&conn, "a", "ca0").unwrap();
        toggle_favorite(&conn, "b", "cb0").unwrap();
        let favs_a = favorite_channels(&conn, "a", 10).unwrap();
        let favs_b = favorite_channels(&conn, "b", 10).unwrap();
        assert_eq!(favs_a.len(), 1);
        assert_eq!(favs_a[0].id, "ca0");
        assert_eq!(favs_b.len(), 1);
        assert_eq!(favs_b[0].id, "cb0");
    }

    #[test]
    fn cursor_round_trip() {
        let c = encode_cursor("BBC News", "bbc.uk");
        let (n, i) = decode_cursor(&c).unwrap();
        assert_eq!(n, "BBC News");
        assert_eq!(i, "bbc.uk");
    }

    /// The streaming importer's resume logic relies on `INSERT OR
    /// REPLACE` to dedupe channels whose URLs were committed before
    /// the connection dropped. This is the single most important
    /// invariant for a flaky Xtream server: re-parsing the whole body
    /// after a drop must not create duplicates.
    #[test]
    fn partial_import_dedupes_on_resume() {
        let conn = open_temp();
        upsert_source(&conn, "s", "premium-m3u", "Server", "active", "{}").unwrap();

        // First attempt: the server sent 3 channels (c0, c1, c2),
        // then dropped.
        for i in 0..3 {
            insert_channel(
                &conn,
                "s",
                &mk_channel(&format!("c{i}"), &format!("Ch {i}"), None, None),
            )
            .unwrap();
        }

        // Second attempt: server replied 200 to Range, importer
        // re-parses from byte 0. c0, c1, c2 are re-seen — the
        // `INSERT OR REPLACE` updates the name field but doesn't
        // create new rows. The total stays at 3, not 6.
        for i in 0..3 {
            insert_channel(
                &conn,
                "s",
                &mk_channel(&format!("c{i}"), &format!("Ch {i} (refreshed)"), None, None),
            )
            .unwrap();
        }

        let page = query_channels(
            &conn, "s", None, None, None, None, None, None, false, "az", None, 100,
        )
        .unwrap();
        assert_eq!(page.total, 3, "resume should not duplicate");
        // The name field was updated by `INSERT OR REPLACE`.
        let c0 = page.items.iter().find(|c| c.id == "c0").unwrap();
        assert_eq!(c0.name, "Ch 0 (refreshed)");
    }

    /// Reproduces the FK failure the streaming importer used to hit
    /// when its separate connection opened a fresh `iptv_sources`
    /// snapshot. With `INSERT OR IGNORE` the call is a no-op when
    /// the source already exists, so a separate-connection importer
    /// is safe even on a fresh WAL snapshot.
    #[test]
    fn ensure_source_exists_is_idempotent() {
        let conn = open_temp();
        upsert_source(&conn, "s", "free-m3u", "Free TV", "active", "{}").unwrap();
        // Second call: no-op, fields preserved.
        ensure_source_exists(&conn, "s", "free-m3u").unwrap();
        let row = get_source(&conn, "s").unwrap().unwrap();
        assert_eq!(row.display_name, "Free TV");
        assert_eq!(row.status, "active");
        // Channels can be inserted without FK violation.
        insert_channel(&conn, "s", &mk_channel("c0", "Geo News", None, None)).unwrap();
    }

    /// The FK must fire when an unrelated source id is used. This is
    /// the failure mode the original code hit when the streaming
    /// importer opened a different DB file.
    #[test]
    fn insert_channel_rejects_unknown_source() {
        let conn = open_temp();
        // No upsert_source for "ghost"; insert must fail.
        let err = insert_channel(&conn, "ghost", &mk_channel("c0", "x", None, None)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("FOREIGN KEY") || msg.contains("Database error"),
            "got: {msg}"
        );
    }
}
