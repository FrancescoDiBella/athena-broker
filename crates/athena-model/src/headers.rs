pub const JSON_LD_CONTEXT_REL: &str = "http://www.w3.org/ns/json-ld#context";
pub const APPLICATION_JSON: &str = "application/json";
pub const APPLICATION_LD_JSON: &str = "application/ld+json";
pub const APPLICATION_GEO_JSON: &str = "application/geo+json";
pub const ETSI_CORE_CONTEXT_URL: &str =
    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHeader {
    pub uri: String,
    pub rel: String,
    pub mime_type: Option<String>,
}

impl LinkHeader {
    /// Split combined Link fields without treating commas in targets or quoted
    /// parameters as separators.
    pub fn parse_list(header: &str) -> Vec<Self> {
        let mut start = 0;
        let mut quoted = false;
        let mut escaped = false;
        let mut target = false;
        let mut links = Vec::new();
        for (index, c) in header.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' if quoted => escaped = true,
                '"' if !target => quoted = !quoted,
                '<' if !quoted => target = true,
                '>' if !quoted => target = false,
                ',' if !quoted && !target => {
                    if let Some(link) = Self::parse(&header[start..index]) {
                        links.push(link);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        if let Some(link) = Self::parse(&header[start..]) {
            links.push(link);
        }
        links
    }

    pub fn new_context(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            rel: JSON_LD_CONTEXT_REL.to_string(),
            mime_type: Some(APPLICATION_LD_JSON.to_string()),
        }
    }

    pub fn to_header_value(&self) -> String {
        let mut s = format!("<{}>; rel=\"{}\"", self.uri, self.rel);
        if let Some(mime) = &self.mime_type {
            s.push_str(&format!("; type=\"{mime}\""));
        }
        s
    }

    pub fn parse(header: &str) -> Option<Self> {
        let trimmed = header.trim();
        let start = trimmed.find('<')?;
        let end = trimmed.find('>')?;
        if end <= start {
            return None;
        }
        let uri = trimmed[start + 1..end].to_string();
        let params = &trimmed[end + 1..];

        let mut rel = None;
        let mut mime_type = None;

        for part in params.split(';') {
            let part = part.trim();
            if part.starts_with("rel=") {
                let val = part[4..].trim_matches(|c| c == '"' || c == '\'');
                rel = Some(val.to_string());
            } else if part.starts_with("type=") {
                let val = part[5..].trim_matches(|c| c == '"' || c == '\'');
                mime_type = Some(val.to_string());
            }
        }

        let rel = rel?;
        Some(Self {
            uri,
            rel,
            mime_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_survives_combined_navigation_links() {
        let links = LinkHeader::parse_list(&format!(
            "</entities?id=a,b&offset=1>; rel=\"next\"; title=\"page, two\",<https://example.org/context>; rel=\"{JSON_LD_CONTEXT_REL}\"; type=\"application/ld+json\""
        ));
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].uri, "/entities?id=a,b&offset=1");
        assert_eq!(
            links[1],
            LinkHeader::new_context("https://example.org/context")
        );
    }
}
