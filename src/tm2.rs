/// TileMill Layer Source YAML Format
///
/// Further reading: https://tilemill-project.github.io/tilemill/docs/manual/adding-layers/
use crate::{get_epsg_3857_tile_bounds, TileSource};

use std::collections::HashMap;

use serde::Deserialize;

// TODO: remove once async fn in traits become stable
use async_trait::async_trait;

use sqlx::{cursor::Cursor, query, PgPool, Row};

/// A TileMill (.tm2source) data structure.
///
/// Note: The current data structure is not entirely complete. See the
/// crate README for limitations.
#[derive(Clone, Deserialize, Debug)]
pub struct TM2Source {
    pub name: String,
    pub pixel_scale: i64,
    #[serde(rename = "Layer")]
    pub layers: Vec<DataLayer>,
    pub attribution: String,
    #[serde(rename = "minzoom")]
    pub min_zoom: i64,
    #[serde(rename = "maxzoom")]
    pub max_zoom: i64,
    pub center: [f64; 3],
    pub bounds: [f64; 4],
}

#[derive(Clone, Deserialize, Debug)]
pub struct DataLayer {
    pub id: String,
    pub properties: DataLayerProperties,
    #[serde(rename = "Datasource")]
    pub source: LayerSource,
    // TODO: srs
}

#[derive(Clone, Deserialize, Debug)]
pub struct LayerSource {
    pub table: String,
    // TODO: Database connection parameters
}

#[derive(Clone, Deserialize, Debug)]
pub struct DataLayerProperties {
    #[serde(rename = "buffer-size")]
    pub buffer_size: i64,
}

impl TM2Source {
    /// Constructs a new TM2Source using a TM2 format YAML string
    pub fn from(data: &str) -> Result<TM2Source, failure::Error> {
        let mut result: TM2Source = serde_yaml::from_str(data)?;

        for layer in result.layers.iter_mut() {
            layer.source.table = layer
                .source
                .table
                .trim() // Remove whitespace
                .chars() // Convert to a view of characters
                .skip(1) // Drop the first one
                .take(layer.source.table.len() - 7) // Grab all but the last 7 (known spec)
                .collect();
        }

        Ok(result)
    }

    // This should be used when building up and executing the prepared statement,
    // as it guarantees consistent ordering of the buffers
    fn buffer_sizes(&self) -> Vec<i64> {
        let mut result: Vec<i64> = self
            .layers
            .iter()
            .map(|x| x.properties.buffer_size)
            .collect();
        result.sort_unstable();
        result.dedup();

        result
    }

    fn prepared_statement_sql(&self) -> String {
        // Build a mapping of buffer values to parameter indexes. These indexes will
        // be interpolated into the query later. The 7+ is necessary because these buffer sizes
        // get added to the end of the query as we build it.
        let buffer_param_indices: HashMap<i64, i64> = self
            .buffer_sizes()
            .iter()
            .enumerate()
            .map(|(i, x)| (x.to_owned(), 7 + (i * 4) as i64))
            .collect();

        let queries: Vec<String> = self.layers.iter().map(|layer| {
            let buffer_param_index = buffer_param_indices[&layer.properties.buffer_size];

            let clip = layer.properties.buffer_size < 8;  // TODO: Not necessarily scientific...
            let geom = format!("ST_AsMVTGeom(geometry,!bbox_nobuffer!,4096,{},{})", layer.properties.buffer_size, clip);
            let layer_query = layer.source.table.replace("geometry", &format!("{} as mvtgeometry", geom));

            // TODO: look into wrapping everything in a single ST_AsMVT for better parallelism
            let base_query = format!("SELECT ST_ASMVT(tile, '{}', 4096, 'mvtgeometry') FROM ({} WHERE {} IS NOT NULL) AS tile", layer.id, layer_query, geom);
            base_query
                .replace("!bbox_nobuffer!", "ST_MakeBox2D(ST_Point($1, $2), ST_Point($3, $4))")
                .replace("z(!scale_denominator!)", "$5")
                .replace("!pixel_width!", "$6")
                .replace("!bbox!", &format!("ST_MakeBox2D(ST_Point(${}, ${}), ST_Point(${}, ${}))", buffer_param_index, buffer_param_index + 1, buffer_param_index + 2, buffer_param_index + 3))
        }).collect();

        queries.join(" UNION ALL ")
    }
}

#[async_trait]
impl TileSource for TM2Source {
    async fn render_mvt(
        &self,
        pool: &PgPool,
        zoom: u8,
        x: i32,
        y: i32,
    ) -> Result<Vec<u8>, sqlx::Error> {
        let z: i32 = zoom.into();
        let tile_bounds = get_epsg_3857_tile_bounds(self.pixel_scale, zoom, x, y, 0);
        let buffer_sizes = self.buffer_sizes();
        let buffered_tile_bounds = buffer_sizes.iter().map(|buffer_size| {
            get_epsg_3857_tile_bounds(self.pixel_scale, zoom, x, y, buffer_size.to_owned())
        });

        let prepare_sql = self.prepared_statement_sql();

        let mut conn = pool.acquire().await?;
        let init_query = query(&prepare_sql)
            .bind(tile_bounds.west)
            .bind(tile_bounds.south)
            .bind(tile_bounds.east)
            .bind(tile_bounds.north)
            .bind(z)
            .bind(self.pixel_scale);
        let query = buffered_tile_bounds.fold(init_query, |acc, bbox| {
            acc.bind(bbox.west)
                .bind(bbox.south)
                .bind(bbox.east)
                .bind(bbox.north)
        });

        let mut raw_tile: Vec<u8> = Vec::new();
        let mut stream = query.fetch(&mut conn);
        while let Some(row) = stream.next().await? {
            let layer: Vec<u8> = row.get(0);
            raw_tile.extend_from_slice(&layer);
        }

        Ok(raw_tile)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read;

    use super::*;

    #[test]
    fn test_parse_tm2source() {
        let mut file =
            File::open("test_data/tm2layers.yml").expect("Unable to open the test yml file.");
        let mut data = String::new();
        file.read_to_string(&mut data)
            .expect("Unable to read the file");

        let source: Result<TM2Source, _> = TM2Source::from(data.as_str());
        match source {
            Ok(result) => {
                // Check the basic properties
                assert_eq!("OpenMapTiles", result.name);
                assert_eq!(256, result.pixel_scale);

                // Make sure we get the right amount of data back
                assert_ne!(0, result.layers.len());
            }
            Err(e) => panic!("{}", e),
        }
    }

    #[test]
    fn test_generate_prepared_statement_sql() {
        // Simple tests with some contrived layers to make sure the substitution works
        let source = TM2Source {
            name: String::from("Test Style"),
            pixel_scale: 256,
            layers: vec![
                DataLayer {
                    id: String::from("water"),
                    properties: DataLayerProperties { buffer_size: 4 },
                    source: LayerSource {
                        table: String::from("(SELECT geometry FROM layer_water(!bbox!))"),
                    },
                },
                DataLayer {
                    id: String::from("land"),
                    properties: DataLayerProperties { buffer_size: 4 },
                    source: LayerSource {
                        table: String::from("(SELECT geometry FROM layer_land(!bbox!))"),
                    },
                },
                DataLayer {
                    id: String::from("poi"),
                    properties: DataLayerProperties { buffer_size: 32 },
                    source: LayerSource {
                        table: String::from("(SELECT geometry FROM layer_poi(!bbox!))"),
                    },
                },
            ],
            attribution: String::from("OpenStreetMap"),
            min_zoom: 0,
            max_zoom: 14,
            center: [0.0, 0.0, 4.0],
            bounds: [-180.0, -85.0511, 180.0, 85.0511],
        };

        let sql = source.prepared_statement_sql();

        // Make sure it's not empty
        assert_ne!(0, sql.len());

        // Check that the layers show up
        assert_eq!(sql.contains("SELECT ST_ASMVT(tile, \'water\'"), true);
        assert_eq!(sql.contains("SELECT ST_ASMVT(tile, \'land\'"), true);
        assert_eq!(sql.contains("SELECT ST_ASMVT(tile, \'poi\'"), true);
    }
}
