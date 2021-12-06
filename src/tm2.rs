//! Data models and trait implementations for TileMill 2 yaml sources.
//!
//! Further reading: https://tilemill-project.github.io/tilemill/docs/manual/adding-layers/

use crate::TileSource;

use serde::Deserialize;

// TODO: remove once async fn in traits become stable
use async_trait::async_trait;

use sqlx::{query, PgConnection, Row};

const TILE_EXTENT: u16 = 4096;
const TILE_SIZE: u16 = 512;

/// The TileMill (.tm2source) data source model.
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

/// A single layer of a TM2Source
#[derive(Clone, Deserialize, Debug)]
pub struct DataLayer {
    pub id: String,
    pub properties: DataLayerProperties,
    #[serde(rename = "Datasource")]
    pub source: LayerSource,
    // TODO: srs
}

/// A `DataLayer`'s source details
#[derive(Clone, Deserialize, Debug)]
pub struct LayerSource {
    pub table: String,
    pub key_field: String,
    // TODO: Database connection parameters
}

/// Additional properties of a `DataLayer`
#[derive(Clone, Deserialize, Debug)]
pub struct DataLayerProperties {
    #[serde(rename = "buffer-size")]
    pub buffer_size: i64,
}

impl DataLayerProperties {
    fn buffer_size_as_tile_pct(&self) -> f32 {
        self.buffer_size as f32 / TILE_SIZE as f32
    }
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

    fn prepared_statement_sql(&self) -> String {
        let layers = self
            .layers
            .iter()
            .map(|layer| {
                let geom = format!(
                    "ST_AsMVTGeom(geometry,!bbox_nobuffer!,{},{},{}) as geom",
                    TILE_EXTENT,
                    (layer.properties.buffer_size_as_tile_pct() * TILE_EXTENT as f32) as i32,
                    true
                );

                let query = layer
                    .source
                    .table
                    .replace("geometry", &geom)
                    .replace("!bbox_nobuffer!", "ST_TileEnvelope($1, $2, $3)")
                    .replace("z(!scale_denominator!)", "$1")
                    .replace("!pixel_width!", "$4")
                    .replace(
                        "!bbox!",
                        &format!(
                            "ST_TileEnvelope($1, $2, $3, margin => {})",
                            layer.properties.buffer_size_as_tile_pct()
                        ),
                    );

                let key_field = if !layer.source.key_field.is_empty() {
                    format!(", '{}'", layer.source.key_field)
                } else {
                    String::new()
                };

                let column = format!(
                    "ST_AsMVT(t.*, '{}', {}, 'geom'{})",
                    layer.id, TILE_EXTENT, key_field
                );

                format!("SELECT {} AS mvt FROM ({}) as t", column, query)
            })
            .collect::<Vec<_>>();

        format!(
            "SELECT STRING_AGG(a.mvt, NULL) FROM ({}) a",
            layers.join(" UNION ALL ")
        )
    }
}

#[async_trait]
impl TileSource for TM2Source {
    async fn render_mvt(
        &self,
        conn: &mut PgConnection,
        zoom: u8,
        x: i32,
        y: i32,
    ) -> Result<Vec<u8>, sqlx::Error> {
        query(&self.prepared_statement_sql())
            .bind(zoom as i32)
            .bind(x)
            .bind(y)
            .bind(self.pixel_scale)
            .fetch_one(conn)
            .await?
            .try_get(0)
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
                        key_field: String::from(""),
                    },
                },
                DataLayer {
                    id: String::from("land"),
                    properties: DataLayerProperties { buffer_size: 4 },
                    source: LayerSource {
                        table: String::from("(SELECT geometry, osm_id FROM layer_land(!bbox!))"),
                        key_field: String::from("osm_id"),
                    },
                },
                DataLayer {
                    id: String::from("poi"),
                    properties: DataLayerProperties { buffer_size: 32 },
                    source: LayerSource {
                        table: String::from("(SELECT geometry FROM layer_poi(!bbox!))"),
                        key_field: String::from(""),
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
        assert!(sql.contains("layer_water"));
        assert!(sql.contains("layer_land"));
        assert!(sql.contains("layer_poi"));
        assert_eq!(sql.matches("ST_AsMVT(").collect::<Vec<_>>().len(), 3);
        assert_eq!(sql.matches("ST_AsMVTGeom(").collect::<Vec<_>>().len(), 3);
        assert_eq!(sql.matches("osm_id").collect::<Vec<_>>().len(), 2);
    }
}
