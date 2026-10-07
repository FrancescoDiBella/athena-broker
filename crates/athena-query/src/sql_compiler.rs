use crate::ast::{CompareOp, GeoQuery, GeoRel, Literal, LogicalOp, QueryExpr};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SqlParam {
    String(String),
    Number(f64),
    Integer(i64),
    Boolean(bool),
    StringList(Vec<String>),
    NumberList(Vec<f64>),
}

#[derive(Debug, Clone, Default)]
pub struct CompiledQuery {
    pub where_clause: String,
    pub params: Vec<SqlParam>,
}

pub struct SqlCompiler;

impl SqlCompiler {
    /// start_idx is the number of parameters already bound by the caller.
    pub fn compile_q(expr: &QueryExpr, start_idx: usize) -> CompiledQuery {
        let mut params = Vec::new();
        let where_clause = compile_expr(expr, start_idx, &mut params);
        CompiledQuery {
            where_clause,
            params,
        }
    }

    /// start_idx is the first available (one-based) spatial parameter.
    pub fn compile_geo(geo: &GeoQuery, start_idx: usize) -> CompiledQuery {
        let mut params = vec![SqlParam::String(format!(
            r#"{{"type":"{}","coordinates":{}}}"#,
            geo.geometry, geo.coordinates
        ))];
        let target = format!("ST_SetSRID(ST_GeomFromGeoJSON(${start_idx}), 4326)");
        let idx = start_idx + params.len();
        params.push(SqlParam::String(geo.geoproperty.clone()));
        let root = format!("attrs->${idx}::text");
        let root = if let Some(legacy) = geo
            .geoproperty
            .strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
        {
            let legacy_idx = start_idx + params.len();
            params.push(SqlParam::String(legacy.into()));
            format!("COALESCE({root}, attrs->${legacy_idx}::text)")
        } else {
            root
        };
        let column = "(CASE WHEN ga.value->>'type'='GeoProperty' THEN ST_SetSRID(ST_GeomFromGeoJSON((ga.value->'value')::text),4326) END)";
        let where_clause = match &geo.georel {
            GeoRel::Near {
                max_distance,
                min_distance,
            } => {
                let mut clauses = Vec::new();
                for (distance, negated) in [(max_distance, false), (min_distance, true)] {
                    if let Some(distance) = distance {
                        let idx = start_idx + params.len();
                        params.push(SqlParam::Number(*distance));
                        clauses.push(format!(
                            "{}ST_DWithin({column}::geography, {target}::geography, ${idx})",
                            if negated { "NOT " } else { "" }
                        ));
                    }
                }
                if clauses.is_empty() {
                    "FALSE".into()
                } else {
                    format!("({})", clauses.join(" AND "))
                }
            }
            GeoRel::Within => format!("ST_Within({column}, {target})"),
            GeoRel::Contains => format!("ST_Contains({column}, {target})"),
            GeoRel::Intersects => format!("ST_Intersects({column}, {target})"),
            GeoRel::Disjoint => format!("ST_Disjoint({column}, {target})"),
            GeoRel::Equals => format!("ST_Equals({column}, {target})"),
            GeoRel::Overlaps => format!("ST_Overlaps({column}, {target})"),
        };
        let where_clause = format!("EXISTS(SELECT 1 FROM jsonb_array_elements(CASE WHEN jsonb_typeof({root})='array' THEN {root} ELSE jsonb_build_array({root}) END) ga(value) WHERE {where_clause})");
        CompiledQuery {
            where_clause,
            params,
        }
    }
}

fn bind(value: SqlParam, offset: usize, params: &mut Vec<SqlParam>) -> String {
    params.push(value);
    format!("${}", offset + params.len())
}

fn literal(value: &Literal) -> SqlParam {
    match value {
        Literal::Number(n) => SqlParam::Number(*n),
        Literal::String(s) => SqlParam::String(s.clone()),
        Literal::Boolean(b) => SqlParam::Boolean(*b),
    }
}

fn json_path(path: &str, offset: usize, params: &mut Vec<SqlParam>) -> (String, String) {
    let mut parts: Vec<String> = if path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("urn:")
    {
        vec![path.to_owned()]
    } else {
        path.split('.').map(String::from).collect()
    };
    if parts.len() == 1 {
        parts.push("value".into());
    } else if parts[1] != "value" && parts[1] != "object" {
        parts.insert(1, "value".into());
    }
    let relationship_fallback = parts.len() == 2 && parts[1] == "value";
    let legacy = parts[0]
        .strip_prefix("https://uri.etsi.org/ngsi-ld/default-context/")
        .is_some();
    let parameter = bind(SqlParam::StringList(parts), offset, params);
    let mut root = format!("attrs -> ({parameter}::text[])[1]");
    if legacy {
        root = format!("COALESCE({root}, attrs -> substring(({parameter}::text[])[1] from 46))");
    }
    let source=format!("jsonb_array_elements(CASE WHEN jsonb_typeof({root})='array' THEN {root} ELSE jsonb_build_array({root}) END) AS qa(value)");
    let value = if relationship_fallback {
        "COALESCE(qa.value->'value',qa.value->'object')".to_owned()
    } else {
        format!("qa.value #> ({parameter}::text[])[2:]")
    };
    (value, source)
}

fn typed_value(json: &str, value: &Literal) -> String {
    let (kind, cast) = match value {
        Literal::Number(_) => ("number", "double precision"),
        Literal::Boolean(_) => ("boolean", "boolean"),
        Literal::String(_) => ("string", "text"),
    };
    // CASE prevents PostgreSQL from evaluating a cast on heterogeneous values.
    format!("(CASE WHEN jsonb_typeof({json}) = '{kind}' THEN ({json} #>> '{{}}')::{cast} END)")
}

fn compile_expr(expr: &QueryExpr, offset: usize, params: &mut Vec<SqlParam>) -> String {
    match expr {
        QueryExpr::Binary { op, left, right } => {
            let left = compile_expr(left, offset, params);
            let right = compile_expr(right, offset, params);
            format!(
                "({left} {} {right})",
                if *op == LogicalOp::And { "AND" } else { "OR" }
            )
        }
        QueryExpr::Comparison { path, op, value } => {
            let (json, source) = json_path(path, offset, params);
            let lhs = typed_value(&json, value);
            let rhs = bind(literal(value), offset, params);
            let op = match op {
                CompareOp::Equal => "=",
                CompareOp::NotEqual => "<>",
                CompareOp::GreaterThan => ">",
                CompareOp::GreaterThanOrEqual => ">=",
                CompareOp::LessThan => "<",
                CompareOp::LessThanOrEqual => "<=",
            };
            format!("EXISTS(SELECT 1 FROM {source} WHERE {lhs} {op} {rhs})")
        }
        QueryExpr::PatternMatch {
            path,
            pattern,
            negated,
        } => {
            let (json, source) = json_path(path, offset, params);
            let lhs = typed_value(&json, &Literal::String(String::new()));
            let rhs = bind(SqlParam::String(pattern.clone()), offset, params);
            format!(
                "EXISTS(SELECT 1 FROM {source} WHERE {lhs} {} {rhs})",
                if *negated { "!~" } else { "~" }
            )
        }
        QueryExpr::Range { path, min, max } => {
            let (json, source) = json_path(path, offset, params);
            let lhs_min = typed_value(&json, min);
            let lhs_max = typed_value(&json, max);
            let min = bind(literal(min), offset, params);
            let max = bind(literal(max), offset, params);
            format!(
                "EXISTS(SELECT 1 FROM {source} WHERE {lhs_min} >= {min} AND {lhs_max} <= {max})"
            )
        }
        QueryExpr::InList { path, values } => {
            let (json, source) = json_path(path, offset, params);
            let clauses: Vec<_> = values
                .iter()
                .map(|value| {
                    let lhs = typed_value(&json, value);
                    let rhs = bind(literal(value), offset, params);
                    format!("{lhs} = {rhs}")
                })
                .collect();
            if clauses.is_empty() {
                "FALSE".into()
            } else {
                format!(
                    "EXISTS(SELECT 1 FROM {source} WHERE ({}))",
                    clauses.join(" OR ")
                )
            }
        }
    }
}
