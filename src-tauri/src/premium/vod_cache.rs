//! In-memory cache for Xtream VOD catalog responses.
//!
//! The panel returns the whole movie or series list in one JSON blob;
//! pagination is local. Without a cache every page turn and every tab
//! switch re-downloads thousands of rows from the provider.

use std::collections::HashMap;
use std::sync::Mutex;
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
        }
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
    }
}
