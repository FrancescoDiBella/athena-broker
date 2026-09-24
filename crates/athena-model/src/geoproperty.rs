use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Geometry {
    Point {
        coordinates: Vec<f64>,
    },
    MultiPoint {
        coordinates: Vec<Vec<f64>>,
    },
    LineString {
        coordinates: Vec<Vec<f64>>,
    },
    MultiLineString {
        coordinates: Vec<Vec<Vec<f64>>>,
    },
    Polygon {
        coordinates: Vec<Vec<Vec<f64>>>,
    },
    MultiPolygon {
        coordinates: Vec<Vec<Vec<Vec<f64>>>>,
    },
}

impl Geometry {
    pub fn validate(&self) -> Result<(), String> {
        fn point(p: &[f64]) -> bool {
            (p.len() == 2 || p.len() == 3)
                && p.iter().all(|n| n.is_finite())
                && (-180.0..=180.0).contains(&p[0])
                && (-90.0..=90.0).contains(&p[1])
        }
        fn line(points: &[Vec<f64>]) -> bool {
            points.len() >= 2 && points.iter().all(|p| point(p))
        }
        fn polygon(rings: &[Vec<Vec<f64>>]) -> bool {
            !rings.is_empty()
                && rings
                    .iter()
                    .all(|r| r.len() >= 4 && line(r) && r.first() == r.last())
        }
        let valid = match self {
            Self::Point { coordinates } => point(coordinates),
            Self::MultiPoint { coordinates } => {
                !coordinates.is_empty() && coordinates.iter().all(|p| point(p))
            }
            Self::LineString { coordinates } => line(coordinates),
            Self::MultiLineString { coordinates } => {
                !coordinates.is_empty() && coordinates.iter().all(|p| line(p))
            }
            Self::Polygon { coordinates } => polygon(coordinates),
            Self::MultiPolygon { coordinates } => {
                !coordinates.is_empty() && coordinates.iter().all(|p| polygon(p))
            }
        };
        if valid {
            Ok(())
        } else {
            Err("Invalid GeoJSON coordinates or unclosed polygon".into())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoProperty {
    #[serde(rename = "type")]
    pub r#type: String, // Always "GeoProperty"
    pub value: Geometry,
    #[serde(skip_serializing_if = "Option::is_none", rename = "observedAt")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "createdAt")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<DateTime<Utc>>,
}

impl GeoProperty {
    pub fn new_point(longitude: f64, latitude: f64) -> Self {
        Self {
            r#type: "GeoProperty".to_string(),
            value: Geometry::Point {
                coordinates: vec![longitude, latitude],
            },
            observed_at: None,
            dataset_id: None,
            created_at: None,
            modified_at: None,
        }
    }

    pub fn new_geometry(geom: Geometry) -> Self {
        Self {
            r#type: "GeoProperty".to_string(),
            value: geom,
            observed_at: None,
            dataset_id: None,
            created_at: None,
            modified_at: None,
        }
    }
}
