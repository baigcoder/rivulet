//! High-level operations on the IPTV source registry.
//!
//! The `iptv_sources` table holds one row per logical provider. After
//! the Premium TV rewrite, the only kind that lives here is the built-in
//! `free-m3u` row; Premium M3U and Premium Xtream have moved to the
//! `premium/` module and a separate `iptv_premium.db`. Each source has
//! a kind and a status; `active` is the one the user is currently
//! browsing. Activation is atomic (see `db::activate_source`).

use rusqlite::Connection;

use super::db;
use super::errors::IptvError;

/// The fixed source id of the built-in free-TV provider. Always active on
/// first launch; the user cannot delete it.
pub const FREE_TV_SOURCE_ID: &str = "free:iptv-org";

/// Ensure the free-TV source row exists. The M3U importer reuses this id
/// when it stages the iptv-org playlist, so a clean install is one source
/// row pointing at a populated channel table.
pub fn ensure_free_source(conn: &Connection) -> Result<(), IptvError> {
    let exists = db::get_source(conn, FREE_TV_SOURCE_ID)?.is_some();
    if !exists {
        db::upsert_source(
            conn,
            FREE_TV_SOURCE_ID,
            "free-m3u",
            "Free TV",
            "active",
            "{}",
        )?;
    }
    Ok(())
}

/// Whether the free source's channels came from a playlist we no longer use.
///
/// The boot import only fires on an empty channel table, so an install that
/// imported the old list would keep browsing it forever — dead links and all
/// — until someone found the Refresh button. `stamp_free_playlist` records
/// the URL an import actually used, and a mismatch means re-import once.
///
/// A plain substring test rather than parsed JSON: the field holds one key,
/// and a URL cannot appear in it by accident.
pub fn free_playlist_changed(conn: &Connection, playlist: &str) -> Result<bool, IptvError> {
    let stamped = db::get_source(conn, FREE_TV_SOURCE_ID)?
        .map(|s| s.config_json)
        .unwrap_or_default();
    Ok(!stamped.contains(playlist))
}

/// Record the playlist the channel table now holds. Called after an import
/// finishes, never before: a stamp written up front and then a failed
/// download would leave the old channels looking current.
pub fn stamp_free_playlist(conn: &Connection, playlist: &str) -> Result<(), IptvError> {
    let config = serde_json::json!({ "playlist": playlist }).to_string();
    conn.execute(
        "UPDATE iptv_sources SET config_json=?2 WHERE id=?1",
        rusqlite::params![FREE_TV_SOURCE_ID, config],
    )
    .map_err(|e| IptvError::Database(e.to_string()))?;
    Ok(())
}

/// Delete every source row in this database that is not the built-in free
/// one, along with the channels, stats and per-channel state behind it.
/// Returns how many rows went.
///
/// `iptv.db` can only hold the free source now — the Premium rewrite moved
/// M3U and Xtream to `premium/` and a separate `iptv_premium.db`, and no
/// code path left in this module writes any other kind. An install that ran
/// an older build still *has* the rows it wrote then, though, and they are
/// not inert: `list_sources` has no retired filter, so *Live TV → Manage
/// sources* draws a card per leftover with `display_name` as its heading —
/// and for a pasted Xtream playlist that name is the whole URL, username and
/// password included. A retired provider's credentials on screen, and in
/// every backup taken since.
///
/// SQLite keeps the freed pages rather than returning them to the
/// filesystem; a `VACUUM` would, but it is minutes of blocked I/O on a
/// gigabyte file and not something to do while the app is starting. The
/// pages are reused by the next import instead, so the file stops growing
/// rather than shrinking.
pub fn prune_foreign_sources(conn: &Connection) -> Result<usize, IptvError> {
    let ids: Vec<String> = db::list_sources(conn)?
        .into_iter()
        .map(|s| s.id)
        .filter(|id| id != FREE_TV_SOURCE_ID)
        .collect();
    for id in &ids {
        // Only `iptv_channels` has the CASCADE, so the rest is by hand.
        db::delete_source_channels(conn, id)?;
        db::delete_source_state(conn, id)?;
        db::delete_source(conn, id)?;
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::run_schema_pub(&conn).unwrap();
        conn
    }

    #[test]
    fn a_new_playlist_url_asks_for_one_re_import() {
        let conn = db();
        ensure_free_source(&conn).unwrap();
        // Nothing stamped yet: an install that imported the old list.
        assert!(free_playlist_changed(&conn, "http://example.com/new.m3u8").unwrap());
        stamp_free_playlist(&conn, "http://example.com/new.m3u8").unwrap();
        assert!(!free_playlist_changed(&conn, "http://example.com/new.m3u8").unwrap());
        assert!(free_playlist_changed(&conn, "http://example.com/newer.m3u8").unwrap());
    }

    #[test]
    fn prune_keeps_the_built_in_source() {
        let conn = db();
        ensure_free_source(&conn).unwrap();
        assert_eq!(prune_foreign_sources(&conn).unwrap(), 0);
        assert!(db::get_source(&conn, FREE_TV_SOURCE_ID).unwrap().is_some());
    }

    #[test]
    fn prune_takes_a_pasted_playlist_and_everything_behind_it() {
        let conn = db();
        ensure_free_source(&conn).unwrap();
        // What an older build wrote: the pasted URL as the display name.
        let stale = "m3u:00000000deadbeef";
        db::upsert_source(
            &conn,
            stale,
            "premium-m3u",
            "http://example.com/get.php?username=u&password=p&type=m3u_plus",
            "superseded",
            "{}",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO iptv_favorites (source_id, channel_id, added_at) VALUES (?1, 'c1', 0)",
            [stale],
        )
        .unwrap();
        assert_eq!(db::list_sources(&conn).unwrap().len(), 2);

        assert_eq!(prune_foreign_sources(&conn).unwrap(), 1);

        let left = db::list_sources(&conn).unwrap();
        assert_eq!(left.len(), 1, "only the built-in source survives");
        assert_eq!(left[0].id, FREE_TV_SOURCE_ID);
        let favs: i64 = conn
            .query_row(
                "SELECT count(*) FROM iptv_favorites WHERE source_id=?1",
                [stale],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(favs, 0, "per-channel state goes with the source");
    }
}
