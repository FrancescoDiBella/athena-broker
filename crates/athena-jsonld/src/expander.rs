use crate::resolver::ResolvedContext;
use athena_model::Entity;
use std::collections::BTreeMap;

pub fn expand_entity(entity: &Entity, context: &ResolvedContext) -> Entity {
    let mut expanded = entity.clone();
    expanded.type_ = context.expand(&entity.type_);
    expanded.types = entity.types.iter().map(|t| context.expand(t)).collect();

    let mut new_attrs = BTreeMap::new();
    for (k, v) in &entity.attributes {
        let expanded_key = context.expand(k);
        new_attrs.insert(expanded_key, v.clone());
    }
    expanded.attributes = new_attrs;
    expanded
}

pub fn compact_entity(entity: &Entity, context: &ResolvedContext) -> Entity {
    let mut compacted = entity.clone();
    compacted.type_ = context.compact(&entity.type_);
    compacted.types = entity.types.iter().map(|t| context.compact(t)).collect();

    let mut new_attrs = BTreeMap::new();
    for (k, v) in &entity.attributes {
        let compacted_key = context.compact(k);
        new_attrs.insert(compacted_key, v.clone());
    }
    compacted.attributes = new_attrs;
    compacted
}
