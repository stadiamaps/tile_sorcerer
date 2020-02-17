//! # Tile Sorcerer
//!
//! This crate provides tools for modeling and querying vector tile sources.

#![deny(warnings)]

use slippy_map_tilenames::tile2lonlat;
use std::f64::consts::PI;

// TODO: remove once async fn in traits become stable
use async_trait::async_trait;

use sqlx::PgPool;

/// This is the main trait exported by this crate. It is presently rather barebones,
/// but is open for future expansion if other formats become relevant.
#[async_trait]
pub trait TileSource: Sized {
    /// Renders the Mapbox vector tile for a slippy map tile in XYZ format.
    async fn render_mvt(&self, pool: &PgPool, zoom: u8, x: i32, y: i32) -> Result<Vec<u8>, sqlx::Error>;
}

pub mod tm2;

static MAP_WIDTH_IN_METRES: f64 = 40_075_016.685_578_49;
static EPSG_3857_BOUNDS: f64 = 6_378_137_f64 * PI;

struct EPSG3857BBox {
    north: f64,
    west: f64,
    south: f64,
    east: f64,
}

/// Input: longitude and latitude degrees in EPSG:4326
/// Output: (x, y) EPSG:3857
fn epsg_4326_to_epsg_3857(lng: f64, lat: f64) -> (f64, f64) {
    // TODO: turn this into a const fn once the math functions are stabilized
    let y = ((90f64 + lat) * PI / 360f64).tan().ln() / (PI / 180f64);
    (
        lng * EPSG_3857_BOUNDS / 180f64,
        y * EPSG_3857_BOUNDS / 180f64,
    )
}

// TODO: Look into using ST_TileEnvelope and ST_Buffer to replace this
#[allow(clippy::many_single_char_names)]
fn get_epsg_3857_tile_bounds(
    tile_px_width: i64,
    zoom: u8,
    x: i32,
    y: i32,
    buffer: i64,
) -> EPSG3857BBox {
    let (w, n) = tile2lonlat(x as u32, y as u32, zoom);
    let (e, s) = tile2lonlat((x + 1) as u32, (y + 1) as u32, zoom);

    let (nw_3857_x, nw_3857_y) = epsg_4326_to_epsg_3857(w, n);
    let (se_3857_x, se_3857_y) = epsg_4326_to_epsg_3857(e, s);

    let map_width_in_px = (tile_px_width as f64) * 2f64.powf(zoom as f64);
    let buffer_px_percentage = buffer as f64 / map_width_in_px;
    let buffer_amt = buffer_px_percentage * MAP_WIDTH_IN_METRES; // Because EPSG:3857 uses meters as its unit

    EPSG3857BBox {
        north: nw_3857_y + buffer_amt,
        west: nw_3857_x - buffer_amt,
        south: se_3857_y - buffer_amt,
        east: se_3857_x + buffer_amt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;

    #[test]
    fn test_get_tile_bounds() {
        // Zoom 0; planet bounds
        let computed_bounds = get_epsg_3857_tile_bounds(256, 0, 0, 0, 0);
        let correct_bounds = EPSG3857BBox {
            north: 20037508.342789244,
            west: -20037508.342789244,
            south: -20037508.342789255,
            east: 20037508.342789244,
        };
        assert_approx_eq!(correct_bounds.north, computed_bounds.north);

        // Der Schweiz @ z6
        let computed_bounds = get_epsg_3857_tile_bounds(256, 6, 33, 22, 0);
        let correct_bounds = EPSG3857BBox {
            north: 6261721.35712164,
            west: 626172.1357121646,
            south: 5635549.221409473,
            east: 1252344.2714243291,
        };
        assert_approx_eq!(correct_bounds.north, computed_bounds.north);

        // Northern Africa @ z3 with a 1 tile buffer
        let computed_bounds = get_epsg_3857_tile_bounds(256, 3, 4, 3, 256);
        let correct_bounds = EPSG3857BBox {
            north: 10018754.17139462,
            west: -5009377.085697311,
            south: -5009377.085697312,
            east: 10018754.171394622,
        };
        assert_approx_eq!(correct_bounds.north, computed_bounds.north);

        // Der Schweiz @ z6 with a 1 tile buffer
        let computed_bounds = get_epsg_3857_tile_bounds(256, 6, 33, 22, 256);
        let correct_bounds = EPSG3857BBox {
            north: 6887893.492833803,
            west: 0.00000000069849193096,
            south: 5009377.085697309,
            east: 1878516.407136493,
        };
        assert_approx_eq!(correct_bounds.north, computed_bounds.north);
    }
}