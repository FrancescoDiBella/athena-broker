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

pub use athena_model::geoquery::{GeoQuery, GeoRel};
