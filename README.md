# Tile Sorcerer

[![](https://img.shields.io/crates/v/tile_sorcerer.svg)](https://crates.io/crates/tile_sorcerer) [![](https://docs.rs/tile_sorcerer/badge.svg)](https://docs.rs/tile_sorcerer)

Tools for modeling and querying vector tile sources.

## Features

Given a PostGIS database and a TileMill source (such as OpenMapTiles data),
this crate will help you leverage PostGIS to render Mapbox Vector Tiles.

## Known Limitations

The current focus is on high-performance rendering from a single PostGIS database.
Other formats are not presently supported, but can be added in the future.
As such, the database connection info present in layers is presently ignored, and
it is up to the calling application to set up a connection pool pointed at the right
database. Projection info is also currently ignored, and your database is assumed to be
in EPSG:3857 web mercator already.

The trait-based design allows for further extensibility, so additional operations,
tile source formats, etc. will likely be added in the future.
