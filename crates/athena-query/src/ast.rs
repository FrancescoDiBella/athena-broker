use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Number(f64),
    String(String),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareOp {
    Equal,              // ==
    NotEqual,           // !=
    GreaterThan,        // >
    GreaterThanOrEqual, // >=
    LessThan,           // <
    LessThanOrEqual,    // <=
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalOp {
    And, // ;
    Or,  // |
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryExpr {
    Binary {
        op: LogicalOp,
        left: Box<QueryExpr>,
        right: Box<QueryExpr>,
    },
    Comparison {
        path: String,
        op: CompareOp,
        value: Literal,
    },
    PatternMatch {
        path: String,
        pattern: String,
        negated: bool,
    },
    Range {
        path: String,
        min: Literal,
        max: Literal,
    },
    InList {
        path: String,
        values: Vec<Literal>,
    },
}

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoQuery {
    pub georel: GeoRel,
    pub geometry: String,    // "Point", "Polygon", etc.
    pub coordinates: String, // Raw GeoJSON coordinates JSON
    pub geoproperty: String, // Defaults to "location"
}
