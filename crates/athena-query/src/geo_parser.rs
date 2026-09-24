use thiserror::Error;

use crate::ast::{GeoQuery, GeoRel};

#[derive(Debug, Error, PartialEq)]
pub enum GeoParserError {
    #[error("Missing required geo parameter: {0}")]
    MissingParam(&'static str),

    #[error("Invalid georel: {0}")]
    InvalidGeorel(String),

    #[error("Invalid geometry: {0}")]
    InvalidGeometry(String),

    #[error("Invalid coordinates JSON")]
    InvalidCoordinates,
}

pub struct GeoQueryParser;

impl GeoQueryParser {
    pub fn parse(
        georel: Option<&str>,
        geometry: Option<&str>,
        coordinates: Option<&str>,
        geoproperty: Option<&str>,
    ) -> Result<Option<GeoQuery>, GeoParserError> {
        if georel.is_none() && geometry.is_none() && coordinates.is_none() {
            return Ok(None);
        }

        let georel_str = georel.ok_or(GeoParserError::MissingParam("georel"))?;
        let geom_str = geometry.ok_or(GeoParserError::MissingParam("geometry"))?;
        let coords_str = coordinates.ok_or(GeoParserError::MissingParam("coordinates"))?;

        let rel = Self::parse_georel(georel_str)?;
        let valid_geom = match geom_str.to_lowercase().as_str() {
            "point" => "Point",
            "multipoint" => "MultiPoint",
            "linestring" => "LineString",
            "multilinestring" => "MultiLineString",
            "polygon" => "Polygon",
            "multipolygon" => "MultiPolygon",
            _ => return Err(GeoParserError::InvalidGeometry(geom_str.to_string())),
        };

        let coords: serde_json::Value =
            serde_json::from_str(coords_str).map_err(|_| GeoParserError::InvalidCoordinates)?;
        let geometry: athena_model::Geometry =
            serde_json::from_value(serde_json::json!({"type": valid_geom, "coordinates": coords}))
                .map_err(|_| GeoParserError::InvalidCoordinates)?;
        geometry
            .validate()
            .map_err(|_| GeoParserError::InvalidCoordinates)?;

        let prop = geoproperty.unwrap_or("location").to_string();
        if prop.is_empty() || prop.len() > 2048 || prop.chars().any(char::is_control) {
            return Err(GeoParserError::InvalidCoordinates);
        }

        Ok(Some(GeoQuery {
            georel: rel,
            geometry: valid_geom.to_string(),
            coordinates: coords_str.to_string(),
            geoproperty: prop,
        }))
    }

    fn parse_georel(georel: &str) -> Result<GeoRel, GeoParserError> {
        let parts: Vec<&str> = georel.split(';').map(str::trim).collect();
        let primary = parts[0].to_lowercase();

        match primary.as_str() {
            "within" => Ok(GeoRel::Within),
            "contains" => Ok(GeoRel::Contains),
            "intersects" => Ok(GeoRel::Intersects),
            "disjoint" => Ok(GeoRel::Disjoint),
            "equals" => Ok(GeoRel::Equals),
            "near" => {
                let mut max_distance = None;
                let mut min_distance = None;

                for modifier in &parts[1..] {
                    if let Some(val_str) = modifier.strip_prefix("maxDistance==") {
                        let dist: f64 = val_str
                            .parse()
                            .map_err(|_| GeoParserError::InvalidGeorel(georel.to_string()))?;
                        if !dist.is_finite() || dist < 0.0 || max_distance.is_some() {
                            return Err(GeoParserError::InvalidGeorel(georel.into()));
                        }
                        max_distance = Some(dist);
                    } else if let Some(val_str) = modifier.strip_prefix("minDistance==") {
                        let dist: f64 = val_str
                            .parse()
                            .map_err(|_| GeoParserError::InvalidGeorel(georel.to_string()))?;
                        if !dist.is_finite() || dist < 0.0 || min_distance.is_some() {
                            return Err(GeoParserError::InvalidGeorel(georel.into()));
                        }
                        min_distance = Some(dist);
                    } else {
                        return Err(GeoParserError::InvalidGeorel(georel.into()));
                    }
                }

                if max_distance.is_none() && min_distance.is_none()
                    || matches!((min_distance, max_distance), (Some(min), Some(max)) if min > max)
                {
                    return Err(GeoParserError::InvalidGeorel(georel.into()));
                }
                Ok(GeoRel::Near {
                    max_distance,
                    min_distance,
                })
            }
            _ => Err(GeoParserError::InvalidGeorel(georel.to_string())),
        }
    }
}
