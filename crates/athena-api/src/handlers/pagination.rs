use axum::http::{header, HeaderMap, HeaderValue};

/// Preserve all filters when constructing navigation links. The context Link is
/// appended separately by middleware and must not replace these relations.
pub fn links(
    headers: &mut HeaderMap,
    path: &str,
    query: Option<&str>,
    limit: i64,
    offset: i64,
    count: i64,
) {
    if limit <= 0 {
        return;
    }
    let pairs: Vec<_> = url::form_urlencoded::parse(query.unwrap_or("").as_bytes())
        .into_owned()
        .filter(|(key, _)| key != "limit" && key != "offset")
        .collect();
    for (relation, target) in [
        (
            "next",
            offset.checked_add(limit).filter(|next| *next < count),
        ),
        ("prev", (offset > 0).then(|| (offset - limit).max(0))),
    ] {
        if let Some(target) = target {
            let mut params = pairs.clone();
            params.extend([
                ("limit".into(), limit.to_string()),
                ("offset".into(), target.to_string()),
            ]);
            let query = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(params)
                .finish();
            if let Ok(value) =
                HeaderValue::from_str(&format!("<{path}?{query}>; rel=\"{relation}\""))
            {
                headers.append(header::LINK, value);
            }
        }
    }
}
