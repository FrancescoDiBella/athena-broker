use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GeoRel {
    Near {
        max_distance: Option<f64>,
        min_distance: Option<f64>,
    },
    Within,
    Contains,
    Intersects,
    Disjoint,
    Equals,
    Overlaps,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoQuery {
    pub georel: GeoRel,
    pub geometry: String,    // "Point", "Polygon", etc.
    pub coordinates: String, // Raw GeoJSON coordinates JSON
    pub geoproperty: String, // Defaults to "location"
}
