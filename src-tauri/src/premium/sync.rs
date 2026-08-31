//! Catalog and EPG import: the only code that writes the cache tables.
//!
//! **This is the module that was missing.** Everything downstream reads
//! SQLite; nothing downstream contacts a provider. So if nothing ever
//! calls a provider and writes what it returns, every read path answers
//! correctly and answers nothing — which is exactly what Premium TV did
//! before this file existed.
//!
//! Two imports, two lifetimes. The catalog (categories + channels) is
//! effectively static: a provider's lineup changes when they add a
//! channel, so it is refetched twice a day. The guide moves constantly
//! and is refetched hourly. They are separate for that reason alone —
//! re-downloading a 500,000-line playlist to find out what is on BBC One
//! next is the difference between a feature and a data bill.
//!
//! Every import is bounded, every import is interruptible by nothing
//! (there is no cancel — a sync either finishes or fails), and exactly
//! one runs at a time per process. A second `/refresh` arriving mid-run
//! is answered `AlreadySyncing` rather than starting a competing
//! download of the same several hundred megabytes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::errors::PremiumError;
use super::factory;
use super::models::{CatalogState, EpgProgram, PremiumAccount, SyncReport};
use super::provider::IPTVProvider;
use super::repository::{self, PremiumRepository};
use super::storage::{self, PremiumState};

/// How long a catalog stays fresh: 12 hours. A provider's channel list
/// changes when they add or drop a channel, which is not an hourly
/// event, and re-importing is the single most expensive thing this
/// module does.
pub const CATALOG_TTL_SECS: i64 = 12 * 60 * 60;

/// How long the guide stays fresh: 1 hour. Shorter than the catalog on
/// purpose — "what's on now" is wrong within minutes of being stale,
/// and an XMLTV document is a fraction of a playlist's size.
pub const EPG_TTL_SECS: i64 = 60 * 60;

/// How many programmes the per-channel fallback asks for. Enough for a
/// now/next line plus a few hours of "up next", and small enough that
/// the request is not worth caching harder than the hour above.
pub const FALLBACK_EPG_LIMIT: usize = 24;

/// Ceiling on the rows one bulk EPG import may write.
///
/// A guide covering 20,000 channels a week deep is tens of millions of
/// rows, and the parse holds them in memory before the transaction. The
/// cap is far past any real provider's useful horizon; hitting it means
/// the guide is broader than the catalog it is being joined to, and the
/// rows past it would be for channels the user cannot watch.
const MAX_EPG_ROWS: usize = 1_000_000;

/// Held for the duration of an import, released on drop.
///
/// `Drop` rather than a `store(false)` at the end of the function: an
/// early `?` return is the *likely* exit from a sync, not the unlikely
/// one, and a flag left set by a failed import means the user cannot
/// retry until they restart the app.
struct SyncGuard(Arc<AtomicBool>);

impl SyncGuard {
    /// `AlreadySyncing` if an import is running. The compare-exchange is
    /// what makes this a lock and not a check — two `/refresh` requests
    /// arriving in the same millisecond both read `false` otherwise.
    fn acquire(flag: &Arc<AtomicBool>) -> Result<Self, PremiumError> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| PremiumError::AlreadySyncing)?;
        Ok(Self(flag.clone()))
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Authenticate, then import everything. What `/connect` runs.
///
/// The account snapshot is returned alongside the report because the
/// connect screen shows both: who you are signed in as, and what came
/// down. Authentication happens first so a wrong password is one failed
/// request rather than a failed multi-megabyte download.
pub async fn connect(
    state: Arc<PremiumState>,
    connection_id: &str,
) -> Result<(PremiumAccount, SyncReport), PremiumError> {
    let _guard = SyncGuard::acquire(&state.syncing)?;
    let provider = factory::provider_for(state.clone(), connection_id)?;
    let mut account = provider.authenticate().await?;

    // Persist what the provider said about the account. `expires_at`
    // and the connection counts are the two things a user actually
    // wants from this screen, and they are the two a provider is most
    // likely to omit — `update_account` COALESCEs, so a `None` leaves
    // whatever a previous connect learned.
    {
        let conn = lock(&state)?;
        storage::update_account(
            &conn,
            connection_id,
            account.account_name.as_deref(),
            account.expires_at.as_deref(),
            account.is_trial,
            account.active_connections,
            account.max_connections,
        )?;
    }

    let report = import(state.clone(), connection_id, provider.as_ref()).await?;

    // The row's own status is the authority on "connected", and the
    // account we hand back should agree with it rather than with
    // whatever string the adapter chose.
    {
        let conn = lock(&state)?;
        storage::set_status(&conn, connection_id, "active")?;
    }
    account.status = "active".to_string();
    Ok((account, report))
}

/// Re-import the catalog and the guide. What `/refresh` runs.
///
/// No `authenticate` call: `get_catalog` sends the same credentials and
/// fails the same way, so a separate handshake would double the round
/// trips to learn the same thing.
pub async fn sync_catalog(
    state: Arc<PremiumState>,
    connection_id: &str,
) -> Result<SyncReport, PremiumError> {
    let _guard = SyncGuard::acquire(&state.syncing)?;
    let provider = factory::provider_for(state.clone(), connection_id)?;
    import(state.clone(), connection_id, provider.as_ref()).await
}

/// Import the catalog, then try the guide. Assumes the caller holds a
/// `SyncGuard`.
///
/// A failed EPG import is not a failed sync. The catalog is what the app
/// is unusable without; a guide is an enhancement, and a provider with a
/// broken `xmltv.php` should still leave the user with channels they can
/// watch. So the EPG error is swallowed *here specifically* — the one
/// place in the module where that is the right call — and reported as
/// `epg_available: false` rather than propagated.
async fn import(
    state: Arc<PremiumState>,
    connection_id: &str,
    provider: &dyn IPTVProvider,
) -> Result<SyncReport, PremiumError> {
    let catalog = provider.get_catalog().await?;

    // One lock for both writes: a reader that saw the new channels
    // against the old categories would render a sidebar that does not
    // match its grid.
    {
        let conn = lock(&state)?;
        storage::replace_categories(&conn, connection_id, &catalog.categories)?;
        storage::replace_channels(&conn, connection_id, &catalog.channels)?;
        storage::mark_catalog_synced(&conn, connection_id)?;
    }

    let synced_at = {
        let conn = lock(&state)?;
        storage::sync_times(&conn, connection_id)?.0.unwrap_or(0)
    };

    let (programs, epg_available) =
        match import_epg(state.clone(), connection_id, provider).await {
            Ok(v) => v,
            Err(e) => {
                // Not silent: the reason is on stderr for adb and for a
                // desktop log, and the UI is told the guide is absent.
                // Both matter — "no EPG" and "EPG broke" look identical
                // on screen and need different answers from us.
                eprintln!("[premium-sync] EPG import failed: {e}");
                (0, false)
            }
        };

    Ok(SyncReport {
        categories: catalog.categories.len(),
        channels: catalog.channels.len(),
        programs,
        epg_available,
        synced_at,
    })
}

/// Import the guide. `(rows written, whether the provider has one)`.
///
/// The bulk path is preferred and the per-channel path is not a
/// substitute for it: `get_epg` costs one request per channel, so
/// running it across a catalog is thousands of requests and a
/// rate-limit. When there is no bulk guide this returns
/// `(0, false)` and the watch page fetches the one channel it needs
/// through `sync_channel_epg`.
async fn import_epg(
    state: Arc<PremiumState>,
    connection_id: &str,
    provider: &dyn IPTVProvider,
) -> Result<(usize, bool), PremiumError> {
    let Some(bytes) = provider.get_bulk_epg().await? else {
        return Ok((0, false));
    };
    // `from_utf8_lossy`: an XMLTV document that is 99% valid UTF-8 with
    // one bad byte in one description is still a usable guide, and
    // refusing the whole document over it would be the wrong trade.
    let xml = String::from_utf8_lossy(&bytes);
    let raw = repository::parse_xmltv(&xml)?;
    drop(bytes);
    if raw.is_empty() {
        return Ok((0, false));
    }

    let repo = PremiumRepository::new(state.clone());
    let programs = remap_to_channel_ids(&repo.epg_id_map(connection_id)?, raw);
    if programs.is_empty() {
        // The provider has a guide; none of it names a channel we
        // imported. That is a provider-side mismatch (commonly an M3U
        // with no `tvg-id` attributes at all), and it is not something
        // a retry fixes — so it reports as "no guide", not as an error.
        return Ok((0, false));
    }

    let written = programs.len();
    repo.replace_epg(connection_id, &programs)?;
    {
        let conn = lock(&state)?;
        storage::mark_epg_synced(&conn, connection_id)?;
    }
    Ok((written, true))
}

/// Rewrite XMLTV programmes from guide ids onto our channel ids.
///
/// XMLTV keys a programme by the guide's own channel id — `tvg-id` in an
/// M3U, `epg_channel_id` in Xtream — which is a different namespace from
/// our row ids and is routinely shared by the HD and SD variants of one
/// channel. So this is a fan-out, not a rename: one programme becomes
/// one row per channel carrying that guide id, which is what gives both
/// variants a guide instead of only whichever one happened to be
/// imported last.
///
/// Matching is case- and whitespace-insensitive. Guide ids are supposed
/// to be exact, and in practice a playlist writes `tvg-id="BBCOne.uk"`
/// against an XMLTV that says `BBCone.uk`.
fn remap_to_channel_ids(
    epg_ids: &std::collections::HashMap<String, Vec<String>>,
    raw: Vec<EpgProgram>,
) -> Vec<EpgProgram> {
    if epg_ids.is_empty() {
        return Vec::new();
    }
    let folded: std::collections::HashMap<String, &Vec<String>> = epg_ids
        .iter()
        .map(|(k, v)| (k.trim().to_lowercase(), v))
        .collect();
    let mut out: Vec<EpgProgram> = Vec::new();
    for p in raw {
        let Some(channels) = folded.get(p.channel_id.trim().to_lowercase().as_str()) else {
            continue;
        };
        for id in channels.iter() {
            if out.len() >= MAX_EPG_ROWS {
                return out;
            }
            out.push(EpgProgram {
                channel_id: id.clone(),
                ..p.clone()
            });
        }
    }
    out
}

/// Fetch the guide for one channel through the per-channel endpoint and
/// merge it in. The fallback for a provider with no bulk XMLTV.
///
/// Called for the channel being watched and for nothing else. It is a
/// `merge`, not a `replace`, precisely because of that: replacing would
/// delete every other channel's guide to add one channel's.
pub async fn sync_channel_epg(
    state: Arc<PremiumState>,
    connection_id: &str,
    channel_id: &str,
) -> Result<usize, PremiumError> {
    let provider = factory::provider_for(state.clone(), connection_id)?;
    let programs = provider.get_epg(channel_id, FALLBACK_EPG_LIMIT).await?;
    if programs.is_empty() {
        return Ok(0);
    }
    // The adapter is contracted to key these on the id it was asked
    // about, but this is the write that would corrupt the table if one
    // ever didn't, so it is normalized here as well.
    let owned: Vec<EpgProgram> = programs
        .into_iter()
        .map(|p| EpgProgram {
            channel_id: channel_id.to_string(),
            ..p
        })
        .collect();
    PremiumRepository::new(state).merge_epg(connection_id, &owned)
}

/// What `/status` reports: how much is cached and how old it is.
pub fn catalog_state(
    state: &PremiumState,
    connection_id: &str,
) -> Result<CatalogState, PremiumError> {
    let conn = lock(state)?;
    let (catalog_synced_at, epg_synced_at) = storage::sync_times(&conn, connection_id)?;
    Ok(CatalogState {
        channels: storage::channel_count(&conn, connection_id)?,
        categories: storage::category_count(&conn, connection_id)?,
        catalog_synced_at,
        epg_synced_at,
        syncing: state.syncing.load(Ordering::SeqCst),
    })
}

/// Whether the catalog and the guide are past their TTLs. A catalog that
/// has never imported is stale by definition, which is what makes a
/// connection interrupted mid-import recover on the next visit instead
/// of sitting empty forever.
pub fn staleness(state: &PremiumState, connection_id: &str) -> Result<(bool, bool), PremiumError> {
    let conn = lock(state)?;
    let (catalog_at, epg_at) = storage::sync_times(&conn, connection_id)?;
    drop(conn);
    let now = now_secs();
    let stale = |at: Option<i64>, ttl: i64| at.map(|t| now - t > ttl).unwrap_or(true);
    Ok((
        stale(catalog_at, CATALOG_TTL_SECS),
        stale(epg_at, EPG_TTL_SECS),
    ))
}

/// Import only what has gone stale. `None` when nothing had.
///
/// This is what a page load calls. It is why opening Premium TV is a
/// SQLite read and not a network round trip on every visit, and why the
/// first visit after twelve hours quietly catches itself up.
pub async fn refresh_if_stale(
    state: Arc<PremiumState>,
    connection_id: &str,
) -> Result<Option<SyncReport>, PremiumError> {
    let (catalog_stale, epg_stale) = staleness(&state, connection_id)?;
    if catalog_stale {
        return sync_catalog(state, connection_id).await.map(Some);
    }
    if !epg_stale {
        return Ok(None);
    }
    // Guide only. The catalog is fresh, so re-downloading the playlist
    // to reach the `x-tvg-url` on its first line is the cost here, and
    // it is still an order of magnitude cheaper than a full import.
    let _guard = SyncGuard::acquire(&state.syncing)?;
    let provider = factory::provider_for(state.clone(), connection_id)?;
    let (programs, epg_available) = import_epg(state.clone(), connection_id, provider.as_ref())
        .await
        .unwrap_or_else(|e| {
            eprintln!("[premium-sync] EPG refresh failed: {e}");
            (0, false)
        });
    let conn = lock(&state)?;
    let counts = (
        storage::category_count(&conn, connection_id)?,
        storage::channel_count(&conn, connection_id)?,
        storage::sync_times(&conn, connection_id)?.0.unwrap_or(0),
    );
    drop(conn);
    Ok(Some(SyncReport {
        categories: counts.0,
        channels: counts.1,
        programs,
        epg_available,
        synced_at: counts.2,
    }))
}

fn lock(
    state: &PremiumState,
) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, PremiumError> {
    state
        .db
        .lock()
        .map_err(|e| PremiumError::Database(format!("lock: {e}")))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(guide_id: &str, start: i64) -> EpgProgram {
        EpgProgram {
            channel_id: guide_id.to_string(),
            title: "Show".to_string(),
            description: None,
            start,
            stop: Some(start + 1800),
        }
    }

    #[test]
    fn remap_fans_out_to_every_channel_sharing_a_guide_id() {
        let mut map = std::collections::HashMap::new();
        map.insert(
            "bbcone.uk".to_string(),
            vec!["101".to_string(), "102".to_string()],
        );
        let out = remap_to_channel_ids(&map, vec![program("bbcone.uk", 100)]);
        assert_eq!(out.len(), 2, "HD and SD both get the programme");
        assert_eq!(out[0].channel_id, "101");
        assert_eq!(out[1].channel_id, "102");
    }

    #[test]
    fn remap_folds_case_and_whitespace() {
        let mut map = std::collections::HashMap::new();
        map.insert("BBCOne.uk".to_string(), vec!["101".to_string()]);
        let out = remap_to_channel_ids(&map, vec![program(" bbcone.UK ", 100)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].channel_id, "101");
    }

    #[test]
    fn remap_drops_guide_ids_we_have_no_channel_for() {
        let mut map = std::collections::HashMap::new();
        map.insert("bbcone.uk".to_string(), vec!["101".to_string()]);
        let out = remap_to_channel_ids(&map, vec![program("itv.uk", 100)]);
        assert!(out.is_empty());
    }

    #[test]
    fn remap_without_any_epg_ids_is_empty() {
        let out = remap_to_channel_ids(&Default::default(), vec![program("bbcone.uk", 100)]);
        assert!(out.is_empty(), "an M3U with no tvg-id has no joinable guide");
    }

    #[test]
    fn sync_guard_is_exclusive_and_releases() {
        let flag = Arc::new(AtomicBool::new(false));
        {
            let _held = SyncGuard::acquire(&flag).expect("first acquire");
            assert!(SyncGuard::acquire(&flag).is_err(), "second is refused");
        }
        assert!(
            SyncGuard::acquire(&flag).is_ok(),
            "the flag is clear again after the guard drops"
        );
    }

    #[test]
    fn epg_ttl_is_shorter_than_the_catalog_ttl() {
        // The whole reason the two imports are separate.
        assert!(EPG_TTL_SECS < CATALOG_TTL_SECS);
    }
}
