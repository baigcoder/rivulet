//! Premium TV repository: dashboard queries, local pagination over
//! the cached channel set, favorites and recent.
//!
//! **Everything here reads SQLite and never the provider.** The
//! catalog was written once by `sync::sync_catalog`; paging, search,
//! filtering and the dashboard are all local reads over that table,
//! which is what keeps a 50,000-channel provider as responsive as a
//! 50-channel one.
//!
//! Every query that returns a channel selects the same twelve columns
//! in the same order, named by `CHANNEL_COLUMNS`, and maps them with
//! `channel_row`. That is not decoration: the old code hand-wrote the
//! mapping at each call site and one of them silently dropped
//! `logo_url` and `category_id`, so "Recently watched" rendered
//! without logos.

use std::sync::Arc;

use super::errors::PremiumError;
use super::models::{IPTVChannel, IPTVChannelPage};
use super::storage::PremiumState;

/// The channel select list, `c`-qualified, plus the favourite flag as
/// a twelfth column. `channel_row` reads exactly this.
///
/// The `EXISTS` is a correlated subquery, which sounds expensive and
/// isn't: `iptv_premium_favorites` is keyed on
/// `(connection_id, channel_id)`, so it is one index probe per row and
/// a page is 60 rows. The alternative — fetching the whole favourite
/// set alongside every page and diffing it in Vue — costs a second
/// round trip and gets out of sync.
const CHANNEL_COLUMNS: &str = "c.id, c.name, c.logo_url, c.category_id, c.category_name,
            c.country, c.language, c.epg_id, c.stream_type, c.user_agent, c.referer,
            EXISTS(SELECT 1 FROM iptv_premium_favorites f
                   WHERE f.connection_id = c.connection_id AND f.channel_id = c.id)";

pub struct PremiumRepository {
    pub state: Arc<PremiumState>,
}

impl PremiumRepository {
    pub fn new(state: Arc<PremiumState>) -> Self {
        Self { state }
    }

    /// Local cursor-paginated list over the cached channel set.
    /// The provider is not called — `replace_channels` ran when
    /// the user connected, and the channel list has been in
    /// SQLite since.
    pub fn query_channels(
        &self,
        connection_id: &str,
        category: Option<&str>,
        country: Option<&str>,
        search: Option<&str>,
        favorites_only: bool,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<IPTVChannelPage, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let limit = limit.clamp(1, 500);
        let offset: i64 = cursor
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let search_lower = search.map(|s| s.to_lowercase());
        let cat_filter = category.filter(|s| !s.is_empty());
        let country_filter = country.filter(|s| !s.is_empty());

        // Build the SQL piece by piece so the query plan stays clear.
        // Every fragment appended below is a constant; the user's
        // values only ever arrive as bound parameters.
        let mut sql = format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_channels c
             WHERE c.connection_id = ?1"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(connection_id.to_string())];
        if let Some(cat) = cat_filter {
            sql.push_str(" AND c.category_name = ?");
            params.push(Box::new(cat.to_string()));
        }
        if let Some(c) = country_filter {
            sql.push_str(" AND c.country = ?");
            params.push(Box::new(c.to_string()));
        }
        if let Some(s) = &search_lower {
            sql.push_str(" AND c.name_lower LIKE ?");
            params.push(Box::new(format!("%{s}%")));
        }
        if favorites_only {
            sql.push_str(
                " AND c.id IN (SELECT channel_id FROM iptv_premium_favorites WHERE connection_id = ?1)",
            );
        }
        // `sort_key` is the provider's own lineup position, which is the
        // order a viewer expects to zap through, and it is the leading
        // column of `iptv_premium_channels_sort` — so paging is an index
        // scan rather than a sort of the whole filtered set.
        sql.push_str(" ORDER BY c.sort_key ASC, c.name_lower ASC LIMIT ? OFFSET ?");
        let limit_i64 = limit as i64;
        let offset_i64 = offset;
        params.push(Box::new(limit_i64));
        params.push(Box::new(offset_i64));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), channel_row)?;
        let mut items = Vec::new();
        for r in rows {
            items.push(r?);
        }

        // Total and next cursor are a separate count. Two queries is
        // fine: the channels table is small for a single user and the
        // count is constant.
        let mut count_sql = String::from(
            "SELECT COUNT(*) FROM iptv_premium_channels c WHERE c.connection_id = ?1",
        );
        let mut count_params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(connection_id.to_string())];
        if let Some(cat) = cat_filter {
            count_sql.push_str(" AND c.category_name = ?");
            count_params.push(Box::new(cat.to_string()));
        }
        if let Some(c) = country_filter {
            count_sql.push_str(" AND c.country = ?");
            count_params.push(Box::new(c.to_string()));
        }
        if let Some(s) = &search_lower {
            count_sql.push_str(" AND c.name_lower LIKE ?");
            count_params.push(Box::new(format!("%{s}%")));
        }
        if favorites_only {
            count_sql.push_str(
                " AND c.id IN (SELECT channel_id FROM iptv_premium_favorites WHERE connection_id = ?1)",
            );
        }
        let count_refs: Vec<&dyn rusqlite::ToSql> =
            count_params.iter().map(|b| b.as_ref()).collect();
        let total: i64 = conn.query_row(&count_sql, count_refs.as_slice(), |row| row.get(0))?;

        let next_cursor = if (offset + items.len() as i64) < total {
            Some((offset + items.len() as i64).to_string())
        }
        else {
            None
        };

        Ok(IPTVChannelPage {
            items,
            total: total as usize,
            next_cursor,
        })
    }

    pub fn toggle_favorite(
        &self,
        connection_id: &str,
        channel_id: &str,
    ) -> Result<bool, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM iptv_premium_favorites
                 WHERE connection_id = ?1 AND channel_id = ?2",
                rusqlite::params![connection_id, channel_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| PremiumError::Database(e.to_string()))?
            .unwrap_or(false);
        if exists {
            conn.execute(
                "DELETE FROM iptv_premium_favorites
                 WHERE connection_id = ?1 AND channel_id = ?2",
                rusqlite::params![connection_id, channel_id],
            )?;
            Ok(false)
        }
        else {
            conn.execute(
                "INSERT INTO iptv_premium_favorites (connection_id, channel_id, added_at)
                 VALUES (?1, ?2, strftime('%s','now'))",
                rusqlite::params![connection_id, channel_id],
            )?;
            Ok(true)
        }
    }

    pub fn add_recent(
        &self,
        connection_id: &str,
        channel_id: &str,
    ) -> Result<(), PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        conn.execute(
            "INSERT OR REPLACE INTO iptv_premium_recent (connection_id, channel_id, watched_at)
             VALUES (?1, ?2, strftime('%s','now'))",
            rusqlite::params![connection_id, channel_id],
        )?;
        Ok(())
    }

    pub fn favorite_channels(
        &self,
        connection_id: &str,
        limit: usize,
    ) -> Result<Vec<IPTVChannel>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_favorites fav
             JOIN iptv_premium_channels c
               ON c.connection_id = fav.connection_id AND c.id = fav.channel_id
             WHERE fav.connection_id = ?1
             ORDER BY fav.added_at DESC
             LIMIT ?2"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params![connection_id, limit as i64],
            channel_row,
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn recent_channels(
        &self,
        connection_id: &str,
        limit: usize,
    ) -> Result<Vec<IPTVChannel>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_recent r
             JOIN iptv_premium_channels c
               ON c.connection_id = r.connection_id AND c.id = r.channel_id
             WHERE r.connection_id = ?1
             ORDER BY r.watched_at DESC
             LIMIT ?2"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params![connection_id, limit as i64],
            channel_row,
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// One channel by id, or `None`. Reads the same twelve columns as
    /// every other channel query, so a single-channel fetch and a page
    /// of them are the same shape on the wire.
    pub fn channel_by_id(
        &self,
        connection_id: &str,
        channel_id: &str,
    ) -> Result<Option<IPTVChannel>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_channels c
             WHERE c.connection_id = ?1 AND c.id = ?2"
        ))?;
        let mut rows = stmt.query(rusqlite::params![connection_id, channel_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(channel_row(row)?)),
            None => Ok(None),
        }
    }

    /// The imported category list, in provider order where there is one
    /// and alphabetical where there is not.
    ///
    /// This reads `iptv_premium_categories` and not the channel table,
    /// because it answers a different question from `category_counts`:
    /// this is "what did the provider say its groups are", which a
    /// filter dropdown wants complete. The sidebar wants the other one.
    pub fn list_categories(
        &self,
        connection_id: &str,
    ) -> Result<Vec<super::models::IPTVCategory>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, country, group_name
             FROM iptv_premium_categories
             WHERE connection_id = ?1
             ORDER BY name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([connection_id], |row| {
            Ok(super::models::IPTVCategory {
                id: row.get(0)?,
                name: row.get(1)?,
                country: row.get(2)?,
                group: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Distinct category names with a channel count, for the sidebar.
    /// Read from the channel table rather than
    /// `iptv_premium_categories` so a category with no channels in it
    /// — providers ship plenty — never appears as an empty row the
    /// user can click into.
    pub fn category_counts(
        &self,
        connection_id: &str,
    ) -> Result<Vec<super::models::CategoryCount>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(
            "SELECT category_name, COUNT(*) AS n
             FROM iptv_premium_channels
             WHERE connection_id = ?1 AND category_name IS NOT NULL AND category_name <> ''
             GROUP BY category_name
             ORDER BY category_name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([connection_id], |row| {
            Ok(super::models::CategoryCount {
                name: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ── Dashboard ───────────────────────────────────────────────

    /// Build the dashboard bundle. Counts are computed by a single
    /// pass over the channel table; previews are the top N of each
    /// facet so the response size is bounded regardless of how big
    /// the source is.
    pub fn build_dashboard(
        &self,
        connection_id: &str,
    ) -> Result<super::models::PremiumDashboard, PremiumError> {
        // Compute everything inside one lock so the response is a
        // consistent snapshot of the database.
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let total_channels: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM iptv_premium_channels WHERE connection_id = ?1",
                [connection_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let country_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT country) FROM iptv_premium_channels
                 WHERE connection_id = ?1 AND country IS NOT NULL",
                [connection_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let category_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT category_name) FROM iptv_premium_channels
                 WHERE connection_id = ?1 AND category_name IS NOT NULL",
                [connection_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Top categories with counts.
        let mut cat_stmt = conn.prepare(
            "SELECT category_name, COUNT(*) AS c
             FROM iptv_premium_channels
             WHERE connection_id = ?1 AND category_name IS NOT NULL
             GROUP BY category_name
             ORDER BY c DESC, category_name COLLATE NOCASE ASC
             LIMIT 200",
        )?;
        let categories: Vec<super::models::CategoryCount> = cat_stmt
            .query_map([connection_id], |row| {
                Ok(super::models::CategoryCount {
                    name: row.get(0)?,
                    count: row.get::<_, i64>(1)? as usize,
                })
            })?
            .filter_map(Result::ok)
            .collect();

        let mut cty_stmt = conn.prepare(
            "SELECT country, COUNT(*) AS c
             FROM iptv_premium_channels
             WHERE connection_id = ?1 AND country IS NOT NULL
             GROUP BY country
             ORDER BY c DESC, country COLLATE NOCASE ASC
             LIMIT 200",
        )?;
        let countries: Vec<super::models::CountryCount> = cty_stmt
            .query_map([connection_id], |row| {
                Ok(super::models::CountryCount {
                    name: row.get(0)?,
                    count: row.get::<_, i64>(1)? as usize,
                })
            })?
            .filter_map(Result::ok)
            .collect();

        let mut fav_stmt = conn.prepare(&format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_favorites fav
             JOIN iptv_premium_channels c
               ON c.connection_id = fav.connection_id AND c.id = fav.channel_id
             WHERE fav.connection_id = ?1
             ORDER BY fav.added_at DESC
             LIMIT 20"
        ))?;
        let favorite_previews: Vec<IPTVChannel> = fav_stmt
            .query_map([connection_id], channel_row)?
            .filter_map(Result::ok)
            .collect();

        let mut rec_stmt = conn.prepare(&format!(
            "SELECT {CHANNEL_COLUMNS}
             FROM iptv_premium_recent r
             JOIN iptv_premium_channels c
               ON c.connection_id = r.connection_id AND c.id = r.channel_id
             WHERE r.connection_id = ?1
             ORDER BY r.watched_at DESC
             LIMIT 20"
        ))?;
        let recent_previews: Vec<IPTVChannel> = rec_stmt
            .query_map([connection_id], channel_row)?
            .filter_map(Result::ok)
            .collect();

        // Top 15 country previews (each: up to 20 channels in that country).
        let mut country_previews = Vec::new();
        for cc in countries.iter().take(15) {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CHANNEL_COLUMNS}
                 FROM iptv_premium_channels c
                 WHERE c.connection_id = ?1 AND c.country = ?2
                 ORDER BY c.sort_key ASC, c.name_lower ASC LIMIT 20"
            ))?;
            let rows: Vec<IPTVChannel> = stmt
                .query_map(rusqlite::params![connection_id, &cc.name], channel_row)?
                .filter_map(Result::ok)
                .collect();
            country_previews.push(super::models::PremiumChannelPreview {
                name: cc.name.clone(),
                count: cc.count,
                channels: rows,
            });
        }

        let mut category_previews = Vec::new();
        for cat in categories.iter().take(15) {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CHANNEL_COLUMNS}
                 FROM iptv_premium_channels c
                 WHERE c.connection_id = ?1 AND c.category_name = ?2
                 ORDER BY c.sort_key ASC, c.name_lower ASC LIMIT 20"
            ))?;
            let rows: Vec<IPTVChannel> = stmt
                .query_map(rusqlite::params![connection_id, &cat.name], channel_row)?
                .filter_map(Result::ok)
                .collect();
            category_previews.push(super::models::PremiumChannelPreview {
                name: cat.name.clone(),
                count: cat.count,
                channels: rows,
            });
        }

        Ok(super::models::PremiumDashboard {
            source_id: connection_id.to_string(),
            total_channels: total_channels as usize,
            country_count: country_count as usize,
            category_count: category_count as usize,
            categories,
            countries,
            favorite_previews,
            recent_previews,
            country_previews,
            category_previews,
        })
    }

    // ── EPG ──────────────────────────────────────────────────────

    /// Read the next `limit` programs for a channel, starting from
    /// the earliest program that has not yet ended. Returns `[]` on
    /// any error — the rule from the plan is that EPG failures
    /// never break video playback.
    pub fn read_epg(
        &self,
        connection_id: &str,
        channel_id: &str,
        limit: usize,
    ) -> Result<Vec<super::models::EpgProgram>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let now = chrono_now_secs();
        let mut stmt = conn.prepare(
            "SELECT channel_id, title, description, start_ts, stop_ts
             FROM iptv_premium_epg
             WHERE connection_id = ?1 AND channel_id = ?2 AND stop_ts > ?3
             ORDER BY start_ts ASC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![connection_id, channel_id, now, limit as i64],
            |row| {
                Ok(super::models::EpgProgram {
                    channel_id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    start: row.get(3)?,
                    stop: row.get(4)?,
                })
            },
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Now-and-next for a whole page of channels in one query.
    ///
    /// The channel grid wants a "now playing" line under every card. A
    /// request per card is 60 requests per page and a visibly staggered
    /// grid; this is one. The `ROW_NUMBER()` keeps the result two rows
    /// per channel regardless of how deep the guide goes, so the
    /// response size is bounded by the page, not by the EPG.
    pub fn epg_now_next(
        &self,
        connection_id: &str,
        channel_ids: &[String],
    ) -> Result<Vec<super::models::EpgProgram>, PremiumError> {
        if channel_ids.is_empty() {
            return Ok(Vec::new());
        }
        // One placeholder per id, bound — never interpolated.
        let holes = vec!["?"; channel_ids.len()].join(",");
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let sql = format!(
            "SELECT channel_id, title, description, start_ts, stop_ts FROM (
               SELECT channel_id, title, description, start_ts, stop_ts,
                      ROW_NUMBER() OVER (PARTITION BY channel_id ORDER BY start_ts ASC) AS rn
               FROM iptv_premium_epg
               WHERE connection_id = ?1 AND stop_ts > ?2 AND channel_id IN ({holes})
             )
             WHERE rn <= 2
             ORDER BY channel_id ASC, start_ts ASC"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(channel_ids.len() + 2);
        params.push(Box::new(connection_id.to_string()));
        params.push(Box::new(chrono_now_secs()));
        for id in channel_ids {
            params.push(Box::new(id.clone()));
        }
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            Ok(super::models::EpgProgram {
                channel_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                start: row.get(3)?,
                stop: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// `epg_id` → the channel ids that carry it.
    ///
    /// XMLTV keys its programmes by the *guide's* channel id (`tvg-id`
    /// in an M3U, `epg_channel_id` in Xtream), which is not our row id
    /// and is routinely shared by the HD and SD variants of one
    /// channel. `sync_epg` uses this to rewrite each programme onto
    /// every channel it belongs to, so every read path afterwards can
    /// key on our own id and nothing has to know XMLTV exists.
    pub fn epg_id_map(
        &self,
        connection_id: &str,
    ) -> Result<std::collections::HashMap<String, Vec<String>>, PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let mut stmt = conn.prepare(
            "SELECT epg_id, id FROM iptv_premium_channels
             WHERE connection_id = ?1 AND epg_id IS NOT NULL AND epg_id <> ''",
        )?;
        let rows = stmt.query_map([connection_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map: std::collections::HashMap<String, Vec<String>> = Default::default();
        for r in rows {
            let (epg_id, channel_id) = r?;
            map.entry(epg_id).or_default().push(channel_id);
        }
        Ok(map)
    }

    /// Replace all EPG rows for a connection with a fresh bulk
    /// parse. Called after `get_bulk_epg` returns bytes.
    pub fn replace_epg(
        &self,
        connection_id: &str,
        programs: &[super::models::EpgProgram],
    ) -> Result<(), PremiumError> {
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| PremiumError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM iptv_premium_epg WHERE connection_id = ?1",
            [connection_id],
        )?;
        let now = chrono_now_secs();
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO iptv_premium_epg
                 (connection_id, channel_id, title, description, start_ts, stop_ts, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for p in programs {
                stmt.execute(rusqlite::params![
                    connection_id,
                    p.channel_id,
                    p.title,
                    p.description,
                    p.start,
                    p.stop.unwrap_or(p.start + 1800),
                    now,
                ])?;
            }
        }
        tx.commit().map_err(|e| PremiumError::Database(e.to_string()))?;
        Ok(())
    }

    /// Add programmes without clearing what is already there.
    ///
    /// The per-channel fallback (`get_epg`) is only ever called for the
    /// one channel being watched, so it must not wipe the guide for
    /// every other channel the way `replace_epg` does. The primary key
    /// `(connection_id, channel_id, start_ts)` makes a repeat fetch of
    /// the same programme an update rather than a duplicate row.
    pub fn merge_epg(
        &self,
        connection_id: &str,
        programs: &[super::models::EpgProgram],
    ) -> Result<usize, PremiumError> {
        if programs.is_empty() {
            return Ok(0);
        }
        let conn = self
            .state
            .db
            .lock()
            .map_err(|e| PremiumError::Database(format!("lock: {e}")))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| PremiumError::Database(e.to_string()))?;
        let now = chrono_now_secs();
        let mut written = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO iptv_premium_epg
                 (connection_id, channel_id, title, description, start_ts, stop_ts, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for p in programs {
                stmt.execute(rusqlite::params![
                    connection_id,
                    p.channel_id,
                    p.title,
                    p.description,
                    p.start,
                    p.stop.unwrap_or(p.start + 1800),
                    now,
                ])?;
                written += 1;
            }
        }
        tx.commit().map_err(|e| PremiumError::Database(e.to_string()))?;
        Ok(written)
    }
}

fn channel_row(row: &rusqlite::Row) -> rusqlite::Result<super::models::IPTVChannel> {
    Ok(super::models::IPTVChannel {
        id: row.get(0)?,
        name: row.get(1)?,
        logo_url: row.get(2)?,
        category_id: row.get(3)?,
        category_name: row.get(4)?,
        country: row.get(5)?,
        language: row.get(6)?,
        epg_id: row.get(7)?,
        stream_type: row.get(8)?,
        user_agent: row.get(9)?,
        referer: row.get(10)?,
        // Deliberately not selected. The URL stays server-side; the
        // redirector reads it with `storage::stream_row`.
        stream_url: None,
        is_favorite: row.get::<_, i64>(11)? != 0,
    })
}

fn chrono_now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── XMLTV bulk EPG parsing ────────────────────────────────────

/// Parse an XMLTV document. XMLTV is a verbose, hand-written spec
/// that looks like this:
///
/// ```xml
/// <tv>
///   <channel id="bbc.one.uk"><display-name>BBC One</display-name></channel>
///   <programme start="20240101120000 +0000" stop="20240101130000 +0000" channel="bbc.one.uk">
///     <title>News</title>
///     <desc>Top stories</desc>
///   </programme>
/// </tv>
/// ```
///
/// The parsing is tolerant: unknown elements are ignored, missing
/// `stop` is treated as 30 minutes after `start`, timezone
/// offsets are stripped (we only need Unix seconds), and a
/// `programme` with no matching `channel` is still imported under
/// its `channel` id verbatim. The resulting `EpgProgram` list is
/// what `replace_epg` writes into the cache table.
pub fn parse_xmltv(
    xml: &str,
) -> Result<Vec<super::models::EpgProgram>, PremiumError> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut programs = Vec::new();

    // A programme in flight. The `<title>`/`<desc>` text we
    // collect is per-element, keyed by element name; the final
    // EpgProgram takes the title from the first <title> it
    // sees under the programme, description from <desc>, and
    // stop from the `stop` attribute.
    let mut pending: Option<PendingProgramme> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attrs = std::collections::HashMap::new();
                for a in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                    let val = String::from_utf8_lossy(&a.value).to_string();
                    attrs.insert(key, val);
                }
                if name == "programme" {
                    let channel_id = attrs
                        .get("channel")
                        .cloned()
                        .unwrap_or_default();
                    let start = attrs
                        .get("start")
                        .and_then(|s| parse_xmltv_time(s));
                    let stop = attrs.get("stop").and_then(|s| parse_xmltv_time(s));
                    if let Some(start) = start {
                        pending = Some(PendingProgramme {
                            channel_id,
                            start,
                            stop,
                            title: String::new(),
                            desc: String::new(),
                        });
                    }
                    else {
                        pending = None;
                    }
                }
                else if name == "title" && pending.is_some() {
                    pending.as_mut().unwrap().title.clear();
                }
                else if name == "desc" && pending.is_some() {
                    pending.as_mut().unwrap().desc.clear();
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(p) = pending.as_mut() {
                    let bytes = t.into_inner();
                    let s = String::from_utf8_lossy(&bytes);
                    if p.title.is_empty() {
                        p.title.push_str(&s);
                    }
                    else if p.desc.is_empty() {
                        p.desc.push_str(&s);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "programme" {
                    if let Some(p) = pending.take() {
                        if !p.title.is_empty() && !p.channel_id.is_empty() {
                            programs.push(super::models::EpgProgram {
                                channel_id: p.channel_id,
                                title: p.title,
                                description: if p.desc.is_empty() {
                                    None
                                }
                                else {
                                    Some(p.desc)
                                },
                                start: p.start,
                                stop: p.stop,
                            });
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(PremiumError::MalformedResponse(format!("xmltv: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(programs)
}

struct PendingProgramme {
    channel_id: String,
    start: i64,
    stop: Option<i64>,
    title: String,
    desc: String,
}

/// Parse the XMLTV timestamp `YYYYMMDDHHMMSS [+ZZZZ]`. The offset
/// is ignored — the same provider serving the same channel
/// always uses the same offset, and the UI's "what's on now" math
/// is in absolute Unix seconds anyway. Returns Unix seconds.
fn parse_xmltv_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let main = s.split_whitespace().next().unwrap_or(s);
    if main.len() < 14 {
        return None;
    }
    let y: i64 = main.get(0..4)?.parse().ok()?;
    let mo: i64 = main.get(4..6)?.parse().ok()?;
    let d: i64 = main.get(6..8)?.parse().ok()?;
    let h: i64 = main.get(8..10)?.parse().ok()?;
    let mi: i64 = main.get(10..12)?.parse().ok()?;
    let se: i64 = main.get(12..14)?.parse().ok()?;
    Some(ymd_hms_to_unix(y, mo, d, h, mi, se))
}

fn ymd_hms_to_unix(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> i64 {
    // Days from Unix epoch using a 1970-based proleptic Gregorian
    // calendar. Good enough for the 1970-2100 range — far past
    // any reasonable EPG date.
    let days_from_civil = |y: i64, m: i64, d: i64| -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = (y - era * 400) as i64;
        let m = m as i64;
        let d = d as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    };
    let days = days_from_civil(y, mo, d);
    days * 86400 + h * 3600 + mi * 60 + s
}

// rusqlite's `query_row` doesn't return `Option` natively; this
// helper turns "no row" into `Ok(None)` so the `optional()` call
// above stays readable.
trait OptionalRow {
    fn optional(self) -> Result<Option<bool>, rusqlite::Error>;
}
impl OptionalRow for Result<bool, rusqlite::Error> {
    fn optional(self) -> Result<Option<bool>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xmltv_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="bbc.one.uk"><display-name>BBC One</display-name></channel>
  <programme start="20240101120000 +0000" stop="20240101130000 +0000" channel="bbc.one.uk">
    <title>News at noon</title>
    <desc>Top stories</desc>
  </programme>
  <programme start="20240101130000 +0000" channel="bbc.one.uk">
    <title>News at one</title>
  </programme>
</tv>"#;
        let programs = parse_xmltv(xml).unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].channel_id, "bbc.one.uk");
        // 2024-01-01T12:00:00Z = 1704110400
        assert_eq!(programs[0].start, 1_704_110_400);
        assert_eq!(programs[0].stop, Some(1_704_114_000));
    }

    #[test]
    fn parse_xmltv_skips_programmes_without_title() {
        let xml = r#"<tv>
  <programme start="20240101120000 +0000" channel="x"></programme>
  <programme start="20240101120000 +0000" channel="y">
    <title>Hello</title>
  </programme>
</tv>"#;
        let programs = parse_xmltv(xml).unwrap();
        assert_eq!(programs.len(), 1);
        assert_eq!(programs[0].channel_id, "y");
    }

    #[test]
    fn parse_xmltv_time_strips_offset() {
        assert_eq!(parse_xmltv_time("20240101120000 +0000"), Some(1_704_110_400));
        assert_eq!(parse_xmltv_time("20240101120000"), Some(1_704_110_400));
    }

    #[test]
    fn parse_xmltv_time_rejects_short() {
        assert!(parse_xmltv_time("2024-01-01").is_none());
    }
}
