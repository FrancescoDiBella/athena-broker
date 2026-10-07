use chrono::Utc;
use regex::Regex;
use serde_json::Value;

use athena_model::{Entity, Subscription, SubscriptionStatus};
use athena_query::{CompareOp, Literal, LogicalOp, QueryExpr};

pub struct SubscriptionMatcher;

impl SubscriptionMatcher {
    pub fn matches(sub: &Subscription, entity: &Entity, mutated_attrs: &[String]) -> bool {
        PreparedSubscription::new(sub.clone())
            .is_ok_and(|prepared| prepared.matches(entity, mutated_attrs))
    }

    fn evaluate_query_expr(expr: &QueryExpr, entity: &Entity) -> bool {
        match expr {
            QueryExpr::Binary { op, left, right } => {
                let l = Self::evaluate_query_expr(left, entity);
                let r = Self::evaluate_query_expr(right, entity);
                match op {
                    LogicalOp::And => l && r,
                    LogicalOp::Or => l || r,
                }
            }
            QueryExpr::Comparison { path, op, value } => {
                let entity_val = Self::get_attribute_value(entity, path);
                Self::compare_values(&entity_val, *op, value)
            }
            QueryExpr::PatternMatch {
                path,
                pattern,
                negated,
            } => {
                let entity_val = Self::get_attribute_value(entity, path);
                let values = match entity_val {
                    Some(Value::Array(values)) => values,
                    Some(value) => vec![value],
                    None => vec![],
                };
                let Ok(regex) = Regex::new(pattern) else {
                    return false;
                };
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|value| regex.is_match(value) != *negated)
            }
            QueryExpr::Range { path, min, max } => {
                let entity_val = Self::get_attribute_value(entity, path);
                let values = match entity_val {
                    Some(Value::Array(values)) => values,
                    Some(value) => vec![value],
                    None => vec![],
                };
                values.into_iter().any(|value| {
                    Self::compare_values(&Some(value.clone()), CompareOp::GreaterThanOrEqual, min)
                        && Self::compare_values(&Some(value), CompareOp::LessThanOrEqual, max)
                })
            }
            QueryExpr::InList { path, values } => {
                let entity_val = Self::get_attribute_value(entity, path);
                values
                    .iter()
                    .any(|v| Self::compare_values(&entity_val, CompareOp::Equal, v))
            }
        }
    }

    fn get_attribute_value(entity: &Entity, path: &str) -> Option<Value> {
        let parts: Vec<&str> = if entity.attributes.contains_key(path) {
            vec![path]
        } else {
            path.split('.').collect()
        };
        let attr = entity.attributes.get(parts[0])?;
        fn extract(attr: &Value, parts: &[&str]) -> Option<Value> {
            if parts.len() == 1 {
                return attr.get("value").or_else(|| attr.get("object")).cloned();
            }
            if parts.get(1) == Some(&"value") || parts.get(1) == Some(&"object") {
                let mut cur = attr;
                for part in &parts[1..] {
                    cur = cur.get(*part)?;
                }
                return Some(cur.clone());
            }
            let mut cur = attr.get("value")?;
            for part in &parts[1..] {
                cur = cur.get(*part)?;
            }
            Some(cur.clone())
        }
        if let Some(instances) = attr.as_array() {
            Some(Value::Array(
                instances
                    .iter()
                    .filter_map(|v| extract(v, &parts))
                    .collect(),
            ))
        } else {
            extract(attr, &parts)
        }
    }

    fn compare_values(entity_val: &Option<Value>, op: CompareOp, literal: &Literal) -> bool {
        match (entity_val, literal) {
            (Some(Value::Array(values)), _) => values
                .iter()
                .any(|v| Self::compare_values(&Some(v.clone()), op, literal)),
            (Some(Value::Number(n)), Literal::Number(lit_num)) => {
                let val = n.as_f64().unwrap_or(0.0);
                match op {
                    CompareOp::Equal => (val - lit_num).abs() < f64::EPSILON,
                    CompareOp::NotEqual => (val - lit_num).abs() >= f64::EPSILON,
                    CompareOp::GreaterThan => val > *lit_num,
                    CompareOp::GreaterThanOrEqual => val >= *lit_num,
                    CompareOp::LessThan => val < *lit_num,
                    CompareOp::LessThanOrEqual => val <= *lit_num,
                }
            }
            (Some(Value::String(s)), Literal::String(lit_str)) => match op {
                CompareOp::Equal => s == lit_str,
                CompareOp::NotEqual => s != lit_str,
                CompareOp::GreaterThan => s > lit_str,
                CompareOp::GreaterThanOrEqual => s >= lit_str,
                CompareOp::LessThan => s < lit_str,
                CompareOp::LessThanOrEqual => s <= lit_str,
            },
            (Some(Value::Bool(b)), Literal::Boolean(lit_bool)) => match op {
                CompareOp::Equal => b == lit_bool,
                CompareOp::NotEqual => b != lit_bool,
                _ => false,
            },
            _ => false,
        }
    }
}

/// Compile once per outbox batch; the snapshot is refreshed at the next batch,
/// so another broker replica's PATCH takes effect without cache invalidation lag.
pub struct PreparedSubscription {
    pub subscription: Subscription,
    patterns: Vec<Option<Regex>>,
    query: Option<QueryExpr>,
}
impl PreparedSubscription {
    pub fn new(subscription: Subscription) -> Result<Self, String> {
        let patterns = subscription
            .entities
            .iter()
            .map(|target| {
                target
                    .id_pattern
                    .as_ref()
                    .map(|pattern| {
                        regex::RegexBuilder::new(pattern)
                            .size_limit(1024 * 1024)
                            .build()
                            .map_err(|e| e.to_string())
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let query = subscription
            .q
            .as_ref()
            .map(|query| athena_query::Parser::parse_str(query).map_err(|e| e.to_string()))
            .transpose()?;
        Ok(Self {
            subscription,
            patterns,
            query,
        })
    }
    pub fn matches(&self, entity: &Entity, mutated_attrs: &[String]) -> bool {
        let sub = &self.subscription;
        if sub.status != SubscriptionStatus::Active
            || sub.expires_at.is_some_and(|t| t <= Utc::now())
        {
            return false;
        }
        if !sub.entities.is_empty()
            && !sub
                .entities
                .iter()
                .zip(&self.patterns)
                .any(|(target, pattern)| {
                    (target.r#type == entity.type_ || entity.types.contains(&target.r#type))
                        && target.id.as_ref().is_none_or(|id| id == &entity.id)
                        && pattern
                            .as_ref()
                            .is_none_or(|pattern| pattern.is_match(&entity.id))
                })
        {
            return false;
        }
        if sub.watched_attributes.as_ref().is_some_and(|watched| {
            !watched.is_empty() && !mutated_attrs.iter().any(|attr| watched.contains(attr))
        }) {
            return false;
        }
        self.query
            .as_ref()
            .is_none_or(|query| SubscriptionMatcher::evaluate_query_expr(query, entity))
    }
}

/// Type-indexed candidates avoid scanning every subscription for each mutation.
/// Each index is an immutable view of a single database read, not a stale cache.
pub struct SubscriptionIndex {
    entries: Vec<PreparedSubscription>,
    types: std::collections::HashMap<String, Vec<usize>>,
    wildcard: Vec<usize>,
}
impl SubscriptionIndex {
    pub fn new(subscriptions: Vec<Subscription>) -> Self {
        let mut index = Self {
            entries: Vec::new(),
            types: Default::default(),
            wildcard: Vec::new(),
        };
        for sub in subscriptions {
            let id = sub.id.clone();
            match PreparedSubscription::new(sub) {
                Ok(prepared) => {
                    let position = index.entries.len();
                    if prepared.subscription.entities.is_empty() {
                        index.wildcard.push(position);
                    }
                    for target in &prepared.subscription.entities {
                        index
                            .types
                            .entry(target.r#type.clone())
                            .or_default()
                            .push(position);
                    }
                    index.entries.push(prepared);
                }
                Err(error) => {
                    tracing::warn!(subscription_id=%id,%error,"Invalid stored subscription ignored during matching")
                }
            }
        }
        index
    }
    pub fn candidates(&self, entity: &Entity) -> Vec<&PreparedSubscription> {
        let mut positions = self.wildcard.clone();
        for kind in std::iter::once(&entity.type_).chain(entity.types.iter()) {
            if let Some(candidates) = self.types.get(kind) {
                positions.extend(candidates);
            }
        }
        positions.sort_unstable();
        positions.dedup();
        positions
            .into_iter()
            .map(|position| &self.entries[position])
            .collect()
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn candidates_deduplicate_multiple_selectors_and_include_watched_only() {
        let sub:Subscription=serde_json::from_value(json!({"id":"urn:sub:one","type":"Subscription","entities":[{"type":"A"},{"type":"B"},{"type":"A"}],"notification":{"endpoint":{"uri":"https://example.org"}}})).unwrap();
        let watched:Subscription=serde_json::from_value(json!({"id":"urn:sub:watched","type":"Subscription","watchedAttributes":["reading"],"notification":{"endpoint":{"uri":"https://example.org"}}})).unwrap();
        let index = SubscriptionIndex::new(vec![sub, watched]);
        let entity = Entity::from_json(
            json!({"id":"urn:entity:one","type":["A","B"],"reading":{"type":"Property","value":4}}),
        )
        .unwrap();
        let candidates = index.candidates(&entity);
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            candidates
                .iter()
                .filter(|c| c.matches(&entity, &["reading".into()]))
                .count(),
            2
        );
        assert_eq!(
            candidates
                .iter()
                .filter(|c| c.matches(&entity, &["other".into()]))
                .count(),
            1
        );
    }
}
