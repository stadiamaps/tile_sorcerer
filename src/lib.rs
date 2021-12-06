//! # Tile Sorcerer
//!
//! Tools for modeling and querying vector tile sources.
//!
//! ## Current status
//!
//! This crate should be regarded as stable in terms of code reliability/correctness, but not
//! yet stable in terms of trait and method signatures. While there are a number of
//! known limitations, this code is being deployed at scale already. We are
//! releasing this code in Rust tradition as 0.x until we feel the interface
//! and feature set have stabilized, but welcome usage and contributions from
//! the Rust GIS community.
//!
//! ## Current features
//!
//! Given a PostGIS database and a TileMill source (such as OpenMapTiles data),
//! this crate will help you leverage PostGIS to render Mapbox Vector Tiles.
//!
//! ## Known Limitations
//!
//! The current focus is on high-performance rendering from a single PostGIS database.
//! Other formats are not presently supported, but can be added in the future.
//! As such, the database connection info present in layers is presently ignored, and
//! it is up to the calling application to set up a connection pool pointed at the right
//! database. Projection info is also currently ignored, and your database is assumed to be
//! in EPSG:3857 web mercator already.
//!
//! The trait-based design allows for further extensibility, so additional operations,
//! tile source formats, etc. will likely be added in the future.

#![deny(warnings)]

// TODO: remove once async fn in traits become stable
use async_trait::async_trait;

use sqlx::PgConnection;

/// This is the main trait exported by this crate. It is presently rather barebones,
/// but is open for future expansion if other formats become relevant.
#[async_trait]
pub trait TileSource: Sized {
    /// Renders the Mapbox vector tile for a slippy map tile in XYZ format.
    async fn render_mvt(
        &self,
        conn: &mut PgConnection,
        zoom: u8,
        x: i32,
        y: i32,
    ) -> Result<Vec<u8>, sqlx::Error>;
}

pub mod tm2;
