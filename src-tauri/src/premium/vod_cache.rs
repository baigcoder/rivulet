//! In-memory cache for Xtream VOD catalog responses.
//!
//! A category list is one JSON blob; "all movies" is many of those
//! walked until the page is full. Without a cache every page turn
//! re-downloads the same category from the provider.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::models::{PremiumSeriesItem, PremiumVodItem, VodCategory};

const TTL: Duration = Duration::from_secs(30 * 60);

struct Timed<T> {
    at: Instant,
    data: T,
}

impl<T> Timed<T> {
    fn fresh(&self) -> bool {
        self.at.elapsed() < TTL
    }
}

type CatKey = String;
type ListKey = (String, String);

pub struct VodCache {
    inner: Mutex<VodCacheInner>,
    /// One lock per in-flight category download so a prefetch and a
    /// tab click do not pull the same JSON twice.
    inflight: Mutex<HashMap<ListKey, Arc<tokio::sync::Mutex<()>>>>,
}

struct VodCacheInner {
    movie_categories: HashMap<CatKey, Timed<Vec<VodCategory>>>,
    series_categories: HashMap<CatKey, Timed<Vec<VodCategory>>>,
    movies: HashMap<ListKey, Timed<Vec<PremiumVodItem>>>,
    series: HashMap<ListKey, Timed<Vec<PremiumSeriesItem>>>,
}

impl VodCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VodCacheInner {
                movie_categories: HashMap::new(),
                series_categories: HashMap::new(),
                movies: HashMap::new(),
                series: HashMap::new(),
            }),
            inflight: Mutex::new(HashMap::new()),
        }
    }

    pub fn list_lock(&self, connection_id: &str, kind: &str, category_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let key = (format!("{connection_id}\0{kind}"), category_id.to_string());
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub fn movie_categories(&self, connection_id: &str) -> Option<Vec<VodCategory>> {
        let inner = self.inner.lock().ok()?;
        inner
            .movie_categories
            .get(connection_id)
            .filter(|e| e.fresh())
            .map(|e| e.data.clone())
    }

    pub fn set_movie_categories(&self, connection_id: &str, cats: Vec<VodCategory>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.movie_categories.insert(
                connection_id.to_string(),
                Timed { at: Instant::now(), data: cats },
            );
        }
    }

    pub fn series_categories(&self, connection_id: &str) -> Option<Vec<VodCategory>> {
        let inner = self.inner.lock().ok()?;
        inner
            .series_categories
            .get(connection_id)
            .filter(|e| e.fresh())
            .map(|e| e.data.clone())
    }

    pub fn set_series_categories(&self, connection_id: &str, cats: Vec<VodCategory>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.series_categories.insert(
                connection_id.to_string(),
                Timed { at: Instant::now(), data: cats },
            );
        }
    }

    pub fn movies(&self, connection_id: &str, category_id: &str) -> Option<Vec<PremiumVodItem>> {
        let inner = self.inner.lock().ok()?;
        inner
            .movies
            .get(&(connection_id.to_string(), category_id.to_string()))
            .filter(|e| e.fresh())
            .map(|e| e.data.clone())
    }

    pub fn set_movies(&self, connection_id: &str, category_id: &str, items: Vec<PremiumVodItem>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.movies.insert(
                (connection_id.to_string(), category_id.to_string()),
                Timed { at: Instant::now(), data: items },
            );
        }
    }

    pub fn series(&self, connection_id: &str, category_id: &str) -> Option<Vec<PremiumSeriesItem>> {
        let inner = self.inner.lock().ok()?;
        inner
            .series
            .get(&(connection_id.to_string(), category_id.to_string()))
            .filter(|e| e.fresh())
            .map(|e| e.data.clone())
    }

    pub fn set_series(&self, connection_id: &str, category_id: &str, items: Vec<PremiumSeriesItem>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.series.insert(
                (connection_id.to_string(), category_id.to_string()),
                Timed { at: Instant::now(), data: items },
            );
        }
    }

    pub fn clear_connection(&self, connection_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.movie_categories.remove(connection_id);
            inner.series_categories.remove(connection_id);
            inner.movies.retain(|(cid, _), _| cid != connection_id);
            inner.series.retain(|(cid, _), _| cid != connection_id);
        }
        if let Ok(mut map) = self.inflight.lock() {
            let prefix = format!("{connection_id}\0");
            map.retain(|(cid, _), _| !cid.starts_with(&prefix));
        }
    }
}
