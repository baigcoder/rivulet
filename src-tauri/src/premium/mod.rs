//! Premium TV — the user-added provider path that lives outside the
//! built-in free playlist. Every channel, every EPG listing, every
//! cached category here is something the user brought in by connecting
//! a Xtream account or importing an M3U.
//!
//! The module owns its own SQLite database (`iptv_premium.db`) and its
//! own credential vault. The HTTP API in `src-tauri/src/api/` is the
//! only thing outside this module that talks to it; the Tauri command
//! surface is free of it.

pub mod crypto;
pub mod errors;
pub mod factory;
pub mod m3u;
pub mod models;
pub mod names;
pub mod player;
pub mod provider;
pub mod repository;
pub mod storage;
pub mod sync;
pub mod xtream;

// Only the two types every caller outside this module needs are
// re-exported. The models are reached as `premium::models::X` — a
// second name for each of them here was twelve re-exports nothing
// imported.
pub use errors::PremiumError;
pub use storage::PremiumState;
