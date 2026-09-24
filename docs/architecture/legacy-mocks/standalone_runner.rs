// Self-contained verification suite that compiles directly with standard rustc

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Boolean(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    StringLit(String),
    NumberLit(f64),
    BoolLit(bool),
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    PatternMatch,
    NotPatternMatch,
    And,
    Or,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eof,
}

pub struct Lexer {
    chars: Vec<(usize, char)>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            chars: input.char_indices().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).map(|&(_, c)| c)
    }

    fn advance(&mut self) -> Option<char> {
        if self.pos < self.chars.len() {
            let ch = self.chars[self.pos].1;
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    pub fn next_token(&mut self) -> Result<Token, String> {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }

        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::Eof),
        };

        match c {
            ';' => Ok(Token::And),
            '|' => Ok(Token::Or),
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '[' => Ok(Token::LBracket),
            ']' => Ok(Token::RBracket),
            ',' => Ok(Token::Comma),
            '=' if self.peek() == Some('=') => {
                self.advance();
                Ok(Token::Equal)
            }
            '!' if self.peek() == Some('=') => {
                self.advance();
                Ok(Token::NotEqual)
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::GreaterEqual)
                } else {
                    Ok(Token::Greater)
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::LessEqual)
                } else {
                    Ok(Token::Less)
                }
            }
            '~' if self.peek() == Some('=') => {
                self.advance();
                Ok(Token::PatternMatch)
            }
            '"' | '\'' => {
                let quote = c;
                let mut string_val = String::new();
                let mut closed = false;

                while let Some(ch) = self.advance() {
                    if ch == quote {
                        closed = true;
                        break;
                    } else {
                        string_val.push(ch);
                    }
                }

                if !closed {
                    return Err("Unterminated string".to_string());
                }
                Ok(Token::StringLit(string_val))
            }
            _ if c.is_ascii_digit() || c == '-' => {
                let mut num_str = String::new();
                num_str.push(c);
                let mut has_dot = false;

                while let Some(ch) = self.peek() {
                    if ch.is_ascii_digit() {
                        num_str.push(ch);
                        self.advance();
                    } else if ch == '.' && !has_dot {
                        has_dot = true;
                        num_str.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }

                let num: f64 = num_str.parse().map_err(|_| "Invalid number")?;
                Ok(Token::NumberLit(num))
            }
            _ if c.is_alphabetic() || c == '_' => {
                let mut ident = String::new();
                ident.push(c);

                while let Some(ch) = self.peek() {
                    if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == ':' {
                        ident.push(ch);
                        self.advance();
                    } else {
                        break;
                    }
                }

                match ident.as_str() {
                    "true" => Ok(Token::BoolLit(true)),
                    "false" => Ok(Token::BoolLit(false)),
                    _ => Ok(Token::Ident(ident)),
                }
            }
            other => Err(format!("Unexpected char: {other}")),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            if tok == Token::Eof {
                tokens.push(tok);
                break;
            }
            tokens.push(tok);
        }
        Ok(tokens)
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn parse(input: &str) -> Result<QueryExpr, String> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser { tokens, pos: 0 };
        parser.parse_or()
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            tok
        } else {
            Token::Eof
        }
    }

    fn parse_or(&mut self) -> Result<QueryExpr, String> {
        let mut left = self.parse_and()?;
        while self.peek() == &Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = QueryExpr::Binary {
                op: LogicalOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<QueryExpr, String> {
        let mut left = self.parse_primary()?;
        while self.peek() == &Token::And {
            self.advance();
            let right = self.parse_primary()?;
            left = QueryExpr::Binary {
                op: LogicalOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<QueryExpr, String> {
        match self.peek() {
            Token::LParen => {
                self.advance();
                let expr = self.parse_or()?;
                if self.peek() == &Token::RParen {
                    self.advance();
                    Ok(expr)
                } else {
                    Err("Expected ')'".to_string())
                }
            }
            Token::Ident(_) => self.parse_comparison(),
            other => Err(format!("Unexpected token: {other:?}")),
        }
    }

    fn parse_comparison(&mut self) -> Result<QueryExpr, String> {
        let path = match self.advance() {
            Token::Ident(s) => s,
            other => return Err(format!("Expected ident, found {other:?}")),
        };

        match self.peek() {
            Token::Equal => {
                self.advance();
                if self.peek() == &Token::LBracket {
                    self.advance();
                    let min = self.parse_literal()?;
                    if self.peek() == &Token::Comma {
                        self.advance();
                        let max = self.parse_literal()?;
                        if self.peek() == &Token::RBracket {
                            self.advance();
                            return Ok(QueryExpr::Range { path, min, max });
                        }
                    }
                    return Err("Expected range [min, max]".to_string());
                }
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::Equal,
                    value: val,
                })
            }
            Token::NotEqual => {
                self.advance();
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::NotEqual,
                    value: val,
                })
            }
            Token::Greater => {
                self.advance();
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::GreaterThan,
                    value: val,
                })
            }
            Token::GreaterEqual => {
                self.advance();
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::GreaterThanOrEqual,
                    value: val,
                })
            }
            Token::Less => {
                self.advance();
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::LessThan,
                    value: val,
                })
            }
            Token::LessEqual => {
                self.advance();
                let val = self.parse_literal()?;
                Ok(QueryExpr::Comparison {
                    path,
                    op: CompareOp::LessThanOrEqual,
                    value: val,
                })
            }
            Token::PatternMatch => {
                self.advance();
                match self.advance() {
                    Token::StringLit(pat) | Token::Ident(pat) => Ok(QueryExpr::PatternMatch {
                        path,
                        pattern: pat,
                        negated: false,
                    }),
                    other => Err(format!("Expected pattern, found {other:?}")),
                }
            }
            other => Err(format!("Expected operator, found {other:?}")),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, String> {
        match self.advance() {
            Token::NumberLit(n) => Ok(Literal::Number(n)),
            Token::StringLit(s) => Ok(Literal::String(s)),
            Token::BoolLit(b) => Ok(Literal::Boolean(b)),
            Token::Ident(s) => Ok(Literal::String(s)),
            other => Err(format!("Expected literal, found {other:?}")),
        }
    }
}

pub struct SqlCompiler;

impl SqlCompiler {
    pub fn compile(expr: &QueryExpr, start_idx: usize) -> (String, Vec<Literal>) {
        let mut params = Vec::new();
        let sql = Self::compile_expr(expr, start_idx, &mut params);
        (sql, params)
    }

    fn compile_expr(expr: &QueryExpr, offset: usize, params: &mut Vec<Literal>) -> String {
        match expr {
            QueryExpr::Binary { op, left, right } => {
                let l = Self::compile_expr(left, offset, params);
                let r = Self::compile_expr(right, offset, params);
                let op_str = match op {
                    LogicalOp::And => "AND",
                    LogicalOp::Or => "OR",
                };
                format!("({l} {op_str} {r})")
            }
            QueryExpr::Comparison { path, op, value } => {
                let idx = offset + params.len() + 1;
                params.push(value.clone());
                let op_str = match op {
                    CompareOp::Equal => "=",
                    CompareOp::NotEqual => "<>",
                    CompareOp::GreaterThan => ">",
                    CompareOp::GreaterThanOrEqual => ">=",
                    CompareOp::LessThan => "<",
                    CompareOp::LessThanOrEqual => "<=",
                };
                let cast = match value {
                    Literal::Number(_) => "::numeric",
                    Literal::Boolean(_) => "::boolean",
                    Literal::String(_) => "",
                };
                format!("(attrs->'{path}'->>'value'){cast} {op_str} ${idx}")
            }
            QueryExpr::PatternMatch { path, pattern, negated } => {
                let idx = offset + params.len() + 1;
                params.push(Literal::String(pattern.clone()));
                let op_str = if *negated { "!~" } else { "~" };
                format!("(attrs->'{path}'->>'value') {op_str} ${idx}")
            }
            QueryExpr::Range { path, min, max } => {
                let idx1 = offset + params.len() + 1;
                params.push(min.clone());
                let idx2 = offset + params.len() + 1;
                params.push(max.clone());
                format!("((attrs->'{path}'->>'value')::numeric >= ${idx1} AND (attrs->'{path}'->>'value')::numeric <= ${idx2})")
            }
        }
    }
}

// -----------------------------------------------------------------------------
// GEO PARSER & POSTGIS COMPILER
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct GeoQuery {
    pub georel: GeoRel,
    pub geometry: String,
    pub coordinates: String,
    pub geoproperty: String,
}

pub struct GeoQueryParser;

impl GeoQueryParser {
    pub fn parse(
        georel: Option<&str>,
        geometry: Option<&str>,
        coordinates: Option<&str>,
        geoproperty: Option<&str>,
    ) -> Result<Option<GeoQuery>, String> {
        if georel.is_none() && geometry.is_none() && coordinates.is_none() {
            return Ok(None);
        }

        let georel_str = georel.ok_or("Missing georel")?;
        let geom_str = geometry.ok_or("Missing geometry")?;
        let coords_str = coordinates.ok_or("Missing coordinates")?;

        let rel = match georel_str.split(';').next().unwrap_or("") {
            "within" => GeoRel::Within,
            "contains" => GeoRel::Contains,
            "intersects" => GeoRel::Intersects,
            "disjoint" => GeoRel::Disjoint,
            "equals" => GeoRel::Equals,
            "near" => {
                let mut max_d = None;
                for part in georel_str.split(';').skip(1) {
                    if let Some(val) = part.strip_prefix("maxDistance==") {
                        max_d = Some(val.parse::<f64>().map_err(|_| "Invalid maxDistance")?);
                    }
                }
                GeoRel::Near {
                    max_distance: max_d,
                    min_distance: None,
                }
            }
            other => return Err(format!("Unknown georel: {other}")),
        };

        Ok(Some(GeoQuery {
            georel: rel,
            geometry: geom_str.to_string(),
            coordinates: coords_str.to_string(),
            geoproperty: geoproperty.unwrap_or("location").to_string(),
        }))
    }

    pub fn compile_postgis(geo: &GeoQuery, start_param: usize) -> (String, Vec<Literal>) {
        let mut params = Vec::new();
        params.push(Literal::String(format!(
            r#"{{"type":"{}","coordinates":{}}}"#,
            geo.geometry, geo.coordinates
        )));

        let target_geom = format!("ST_SetSRID(ST_GeomFromGeoJSON(${start_param}), 4326)");
        let col = &geo.geoproperty;

        let sql = match &geo.georel {
            GeoRel::Near { max_distance, .. } => {
                let dist_param = start_param + 1;
                params.push(Literal::Number(max_distance.unwrap_or(1000.0)));
                format!("ST_DWithin({col}::geography, {target_geom}::geography, ${dist_param})")
            }
            GeoRel::Within => format!("ST_Within({col}, {target_geom})"),
            GeoRel::Contains => format!("ST_Contains({col}, {target_geom})"),
            GeoRel::Intersects => format!("ST_Intersects({col}, {target_geom})"),
            GeoRel::Disjoint => format!("ST_Disjoint({col}, {target_geom})"),
            GeoRel::Equals => format!("ST_Equals({col}, {target_geom})"),
        };

        (sql, params)
    }
}

// -----------------------------------------------------------------------------
// CORE CONTEXT DEFINITION
// -----------------------------------------------------------------------------
pub fn is_etsi_core_context(uri: &str) -> bool {
    uri == "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        || uri == "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld"
        || uri == "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"
}

// -----------------------------------------------------------------------------
// TESTS
// -----------------------------------------------------------------------------
#[test]
fn test_query_lexer_and_precedence() {
    let q = "speed>80;brand=='Mercedes'|temp<=15";
    let expr = Parser::parse(q).expect("Parsing failed");
    match expr {
        QueryExpr::Binary { op, left, right } => {
            assert_eq!(op, LogicalOp::Or);
            assert!(matches!(*left, QueryExpr::Binary { op: LogicalOp::And, .. }));
            assert!(matches!(*right, QueryExpr::Comparison { .. }));
        }
        _ => panic!("Expected OR binary expression"),
    }
}

#[test]
fn test_query_parentheses_grouping() {
    let q = "(speed>80|speed<20);brand=='BMW'";
    let expr = Parser::parse(q).expect("Parsing failed");
    match expr {
        QueryExpr::Binary { op, left, right } => {
            assert_eq!(op, LogicalOp::And);
            assert!(matches!(*left, QueryExpr::Binary { op: LogicalOp::Or, .. }));
            assert!(matches!(*right, QueryExpr::Comparison { .. }));
        }
        _ => panic!("Expected AND binary expression"),
    }
}

#[test]
fn test_query_range() {
    let q = "speed==[30,100]";
    let expr = Parser::parse(q).expect("Range parse failed");
    assert_eq!(
        expr,
        QueryExpr::Range {
            path: "speed".to_string(),
            min: Literal::Number(30.0),
            max: Literal::Number(100.0),
        }
    );
}

#[test]
fn test_query_pattern_match() {
    let q = "brand~='^Tesla.*'";
    let expr = Parser::parse(q).expect("Pattern parse failed");
    assert_eq!(
        expr,
        QueryExpr::PatternMatch {
            path: "brand".to_string(),
            pattern: "^Tesla.*".to_string(),
            negated: false,
        }
    );
}

#[test]
fn test_sql_compilation() {
    let q = "speed>50;brand=='Mercedes';active==true";
    let expr = Parser::parse(q).expect("Parse failed");
    let (sql, params) = SqlCompiler::compile(&expr, 0);

    assert_eq!(
        sql,
        "(((attrs->'speed'->>'value')::numeric > $1 AND (attrs->'brand'->>'value') = $2) AND (attrs->'active'->>'value')::boolean = $3)"
    );
    assert_eq!(params.len(), 3);
    assert_eq!(params[0], Literal::Number(50.0));
    assert_eq!(params[1], Literal::String("Mercedes".to_string()));
    assert_eq!(params[2], Literal::Boolean(true));
}

#[test]
fn test_geo_parser_near() {
    let geo = GeoQueryParser::parse(
        Some("near;maxDistance==1500"),
        Some("Point"),
        Some("[13.4050, 52.5200]"),
        Some("location"),
    )
    .expect("Parse failed")
    .expect("Expected GeoQuery");

    let (sql, params) = GeoQueryParser::compile_postgis(&geo, 1);
    assert_eq!(sql, "ST_DWithin(location::geography, ST_SetSRID(ST_GeomFromGeoJSON($1), 4326)::geography, $2)");
    assert_eq!(params.len(), 2);
    assert_eq!(params[1], Literal::Number(1500.0));
}

#[test]
fn test_geo_parser_within() {
    let geo = GeoQueryParser::parse(
        Some("within"),
        Some("Polygon"),
        Some("[[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]"),
        Some("location"),
    )
    .expect("Parse failed")
    .expect("Expected GeoQuery");

    let (sql, params) = GeoQueryParser::compile_postgis(&geo, 1);
    assert_eq!(sql, "ST_Within(location, ST_SetSRID(ST_GeomFromGeoJSON($1), 4326))");
    assert_eq!(params.len(), 1);
}

// -----------------------------------------------------------------------------
// SUBSCRIPTION MATCHER
// -----------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct MockEntity {
    pub id: String,
    pub type_: String,
    pub speed: f64,
}

#[derive(Debug, Clone)]
pub struct MockSubscription {
    pub id: String,
    pub target_type: String,
    pub id_pattern: Option<String>,
    pub watched_attributes: Option<Vec<String>>,
    pub min_speed: Option<f64>,
}

impl MockSubscription {
    pub fn matches(&self, entity: &MockEntity, mutated_attrs: &[String]) -> bool {
        if self.target_type != entity.type_ {
            return false;
        }

        if let Some(pattern) = &self.id_pattern {
            if !entity.id.contains(pattern) {
                return false;
            }
        }

        if let Some(watched) = &self.watched_attributes {
            if !mutated_attrs.iter().any(|a| watched.contains(a)) {
                return false;
            }
        }

        if let Some(min_s) = self.min_speed {
            if entity.speed <= min_s {
                return false;
            }
        }

        true
    }
}

#[test]
fn test_subscription_matching_logic() {
    let sub = MockSubscription {
        id: "urn:ngsi-ld:Subscription:SpeedAlert".to_string(),
        target_type: "Vehicle".to_string(),
        id_pattern: Some("Vehicle:A1".to_string()),
        watched_attributes: Some(vec!["speed".to_string()]),
        min_speed: Some(80.0),
    };

    let speeding_vehicle = MockEntity {
        id: "urn:ngsi-ld:Vehicle:A102".to_string(),
        type_: "Vehicle".to_string(),
        speed: 95.0,
    };

    // Case 1: speed mutated, vehicle matches pattern, speed > 80 -> should match
    assert!(sub.matches(&speeding_vehicle, &["speed".to_string()]));

    // Case 2: only temperature mutated -> should NOT match
    assert!(!sub.matches(&speeding_vehicle, &["temperature".to_string()]));

    // Case 3: speed mutated, but speed is 70 <= 80 -> should NOT match
    let slow_vehicle = MockEntity {
        id: "urn:ngsi-ld:Vehicle:A102".to_string(),
        type_: "Vehicle".to_string(),
        speed: 70.0,
    };
    assert!(!sub.matches(&slow_vehicle, &["speed".to_string()]));

    // Case 4: ID doesn't match pattern -> should NOT match
    let other_vehicle = MockEntity {
        id: "urn:ngsi-ld:Vehicle:B900".to_string(),
        type_: "Vehicle".to_string(),
        speed: 120.0,
    };
    assert!(!sub.matches(&other_vehicle, &["speed".to_string()]));
}

#[test]
fn test_core_context_detection() {
    assert!(is_etsi_core_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"));
    assert!(is_etsi_core_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld"));
    assert!(is_etsi_core_context("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"));
    assert!(!is_etsi_core_context("https://schema.org/"));
}

// -----------------------------------------------------------------------------
// SSRF PROTECTION TESTS
// -----------------------------------------------------------------------------
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn is_private_or_reserved_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_loopback()
                || ipv4.is_private()
                || ipv4.is_link_local()
                || ipv4.is_broadcast()
                || ipv4.is_multicast()
                || ipv4.is_unspecified()
                || *ipv4 == Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_multicast()
                || ipv6.is_unspecified()
        }
    }
}

#[test]
fn test_ssrf_protection_rules() {
    // Loopback addresses must be blocked
    assert!(is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(is_private_or_reserved_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));

    // RFC 1918 Private ranges must be blocked
    assert!(is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 1, 5))));
    assert!(is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))));

    // Cloud metadata service (169.254.169.254) must be blocked
    assert!(is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));

    // Public Internet IP must be allowed
    assert!(!is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    assert!(!is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    assert!(!is_private_or_reserved_ip(&IpAddr::V4(Ipv4Addr::new(142, 250, 180, 206))));
}

// -----------------------------------------------------------------------------
// CSR MATCHING & FEDERATION TESTS
// -----------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct MockCsr {
    pub endpoint: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub property_names: Option<Vec<String>>,
}

impl MockCsr {
    pub fn matches(&self, entity_type: Option<&str>, entity_id: Option<&str>, attrs: Option<&[String]>) -> bool {
        if let Some(t) = entity_type {
            if self.target_type != t {
                return false;
            }
        }
        if let Some(id) = entity_id {
            if let Some(tid) = &self.target_id {
                if tid != id {
                    return false;
                }
            }
        }
        if let Some(queried_attrs) = attrs {
            if let Some(props) = &self.property_names {
                if !queried_attrs.iter().any(|a| props.contains(a)) {
                    return false;
                }
            }
        }
        true
    }
}

#[test]
fn test_csr_matching_for_federation() {
    let csr = MockCsr {
        endpoint: "https://remote-city.broker.org".to_string(),
        target_type: "Streetlight".to_string(),
        target_id: Some("urn:ngsi-ld:Streetlight:001".to_string()),
        property_names: Some(vec!["powerConsumption".to_string(), "status".to_string()]),
    };

    // Query for Streetlight with powerConsumption -> MATCH
    assert!(csr.matches(Some("Streetlight"), Some("urn:ngsi-ld:Streetlight:001"), Some(&["powerConsumption".to_string()])));

    // Query for Vehicle -> NO MATCH
    assert!(!csr.matches(Some("Vehicle"), None, None));

    // Query for unhandled attribute "temperature" -> NO MATCH
    assert!(!csr.matches(Some("Streetlight"), Some("urn:ngsi-ld:Streetlight:001"), Some(&["temperature".to_string()])));
}

#[test]
fn test_federation_entity_merging() {
    use std::collections::HashMap;

    let mut local_entity_attrs = HashMap::new();
    local_entity_attrs.insert("speed".to_string(), 85.0);

    let mut remote_entity_attrs = HashMap::new();
    remote_entity_attrs.insert("speed".to_string(), 90.0); // should not overwrite local
    remote_entity_attrs.insert("fuelLevel".to_string(), 42.0); // new attribute from remote

    // Merge attributes from remote into local entity
    for (k, v) in remote_entity_attrs {
        local_entity_attrs.entry(k).or_insert(v);
    }

    assert_eq!(local_entity_attrs.get("speed"), Some(&85.0)); // Preserved local value
    assert_eq!(local_entity_attrs.get("fuelLevel"), Some(&42.0)); // Augmented remote attribute
}

// -----------------------------------------------------------------------------
// SQL PAGINATION, CONCATENATION & TEMPORAL QUERY BUILDER SIMULATORS
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq)]
pub struct MockSqlParam {
    pub name: String,
    pub index: usize,
}

pub struct MockEntityQueryBuilder {
    pub id: Option<String>,
    pub id_pattern: Option<String>,
    pub type_: Option<String>,
    pub q_params: Vec<String>,
    pub geo_params: Vec<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl MockEntityQueryBuilder {
    pub fn build_sql(&self) -> (String, Vec<MockSqlParam>) {
        let mut sql = String::from("SELECT id, type, attrs FROM entities WHERE 1=1");
        let mut params = Vec::new();

        if let Some(id) = &self.id {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND id = ${idx}"));
            params.push(MockSqlParam { name: format!("id:{id}"), index: idx });
        }

        if let Some(pattern) = &self.id_pattern {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND id ~ ${idx}"));
            params.push(MockSqlParam { name: format!("pattern:{pattern}"), index: idx });
        }

        if let Some(t) = &self.type_ {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND (type = ${idx} OR ${idx} = ANY(types))"));
            params.push(MockSqlParam { name: format!("type:{t}"), index: idx });
        }

        for q_p in &self.q_params {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND (attrs->'{q_p}'->>'value')::numeric > ${idx}"));
            params.push(MockSqlParam { name: format!("q:{q_p}"), index: idx });
        }

        for geo_p in &self.geo_params {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND ST_DWithin(location, ${idx})"));
            params.push(MockSqlParam { name: format!("geo:{geo_p}"), index: idx });
        }

        sql.push_str(" ORDER BY modified_at DESC");

        // Edge case: pagination clamping
        let clamped_limit = self.limit.unwrap_or(20).clamp(1, 1000);
        let valid_offset = self.offset.unwrap_or(0);

        let limit_idx = params.len() + 1;
        let offset_idx = params.len() + 2;
        sql.push_str(&format!(" LIMIT ${limit_idx} OFFSET ${offset_idx}"));
        params.push(MockSqlParam { name: format!("limit:{clamped_limit}"), index: limit_idx });
        params.push(MockSqlParam { name: format!("offset:{valid_offset}"), index: offset_idx });

        (sql, params)
    }
}

pub struct MockTemporalQueryBuilder {
    pub entity_id: String,
    pub timerel: String, // "before", "after", "between"
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: String, // "observedAt", "createdAt", "modifiedAt"
    pub attrs: Vec<String>,
    pub aggr_method: Option<String>,
    pub last_n: Option<usize>,
}

impl MockTemporalQueryBuilder {
    pub fn build_sql(&self) -> Result<(String, Vec<MockSqlParam>), String> {
        if self.timerel == "between" {
            if let Some(end) = &self.end_time_at {
                if self.time_at > *end {
                    return Err("timeAt cannot be after endTimeAt in timerel=between query".to_string());
                }
            } else {
                return Err("endTimeAt required for timerel=between".to_string());
            }
        }

        let time_col = match self.timeproperty.as_str() {
            "createdAt" => "created_at",
            "modifiedAt" => "modified_at",
            _ => "observed_at",
        };

        let mut params = Vec::new();
        params.push(MockSqlParam { name: format!("entity:{}", self.entity_id), index: 1 });
        params.push(MockSqlParam { name: format!("time_at:{}", self.time_at), index: 2 });

        let time_clause = match self.timerel.as_str() {
            "before" => format!("{time_col} < $2"),
            "after" => format!("{time_col} > $2"),
            "between" => {
                params.push(MockSqlParam { name: format!("end_time:{}", self.end_time_at.as_ref().unwrap()), index: 3 });
                format!("{time_col} >= $2 AND {time_col} <= $3")
            }
            other => return Err(format!("Unsupported timerel {other}")),
        };

        let mut attrs_filter = String::new();
        if !self.attrs.is_empty() {
            let idx = params.len() + 1;
            attrs_filter = format!(" AND attribute_id = ANY(${idx})");
            params.push(MockSqlParam { name: format!("attrs:{:?}", self.attrs), index: idx });
        }

        if let Some(aggr) = &self.aggr_method {
            let aggr_fn = match aggr.as_str() {
                "avg" => "AVG(value_numeric)",
                "min" => "MIN(value_numeric)",
                "max" => "MAX(value_numeric)",
                "sum" => "SUM(value_numeric)",
                "totalCount" => "COUNT(*)",
                _ => return Err(format!("Unsupported aggr {aggr}")),
            };

            let sql = format!(
                "SELECT attribute_id, {aggr_fn} as aggr_val, MIN({time_col}) as start_time, MAX({time_col}) as end_time FROM entity_temporal WHERE entity_id = $1 AND {time_clause}{attrs_filter} GROUP BY attribute_id"
            );
            return Ok((sql, params));
        }

        let sql = if let Some(last_n) = self.last_n {
            let last_n_idx = params.len() + 1;
            params.push(MockSqlParam { name: format!("lastN:{last_n}"), index: last_n_idx });
            format!(
                "SELECT entity_id, attribute_id, observed_at, value_json FROM (SELECT entity_id, attribute_id, observed_at, value_json FROM entity_temporal WHERE entity_id = $1 AND {time_clause}{attrs_filter} ORDER BY {time_col} DESC LIMIT ${last_n_idx}) sub ORDER BY {time_col} ASC"
            )
        } else {
            format!(
                "SELECT entity_id, attribute_id, observed_at, value_json FROM entity_temporal WHERE entity_id = $1 AND {time_clause}{attrs_filter} ORDER BY {time_col} ASC"
            )
        };

        Ok((sql, params))
    }
}

// -----------------------------------------------------------------------------
// TESTS: SQL PAGINATION, LIMIT & CONCATENATION EDGE CASES
// -----------------------------------------------------------------------------
#[test]
fn test_sql_pagination_limit_clamping_and_offset() {
    // Case 1: Limit 0 should be clamped to 1
    let q1 = MockEntityQueryBuilder {
        id: None,
        id_pattern: None,
        type_: Some("Vehicle".to_string()),
        q_params: vec![],
        geo_params: vec![],
        limit: Some(0),
        offset: Some(0),
    };
    let (sql1, params1) = q1.build_sql();
    assert!(sql1.contains("LIMIT $2 OFFSET $3"));
    assert_eq!(params1[1].name, "limit:1");
    assert_eq!(params1[2].name, "offset:0");

    // Case 2: Limit 5000 should be clamped to 1000
    let q2 = MockEntityQueryBuilder {
        id: None,
        id_pattern: None,
        type_: None,
        q_params: vec![],
        geo_params: vec![],
        limit: Some(5000),
        offset: Some(150),
    };
    let (sql2, params2) = q2.build_sql();
    assert!(sql2.contains("LIMIT $1 OFFSET $2"));
    assert_eq!(params2[0].name, "limit:1000");
    assert_eq!(params2[1].name, "offset:150");
}

#[test]
fn test_sql_condition_concatenation_and_parameter_indexing() {
    // Test concatenation of ALL conditions: id + pattern + type + q + geo + pagination
    let builder = MockEntityQueryBuilder {
        id: Some("urn:ngsi-ld:Vehicle:A102".to_string()),
        id_pattern: Some("Vehicle:.*".to_string()),
        type_: Some("Vehicle".to_string()),
        q_params: vec!["speed".to_string(), "fuel".to_string()],
        geo_params: vec!["rome_point".to_string()],
        limit: Some(25),
        offset: Some(50),
    };

    let (sql, params) = builder.build_sql();

    // Verify SQL condition concatenation
    assert!(sql.contains("WHERE 1=1"));
    assert!(sql.contains("AND id = $1"));
    assert!(sql.contains("AND id ~ $2"));
    assert!(sql.contains("AND (type = $3 OR $3 = ANY(types))"));
    assert!(sql.contains("(attrs->'speed'->>'value')::numeric > $4"));
    assert!(sql.contains("(attrs->'fuel'->>'value')::numeric > $5"));
    assert!(sql.contains("ST_DWithin(location, $6)"));
    assert!(sql.contains("LIMIT $7 OFFSET $8"));

    // Verify all parameter indices are strictly continuous 1..8
    assert_eq!(params.len(), 8);
    for (i, param) in params.iter().enumerate() {
        assert_eq!(param.index, i + 1, "Parameter index mismatch at position {i}");
    }
}

#[test]
fn test_sql_complex_boolean_q_concatenation() {
    // Nested parentheses: (speed > 80 AND brand == 'BMW') OR (speed > 100 AND brand == 'Audi')
    let q = "(speed>80;brand=='BMW')|(speed>100;brand=='Audi')";
    let expr = Parser::parse(q).expect("Parse complex q failed");
    let (sql, params) = SqlCompiler::compile(&expr, 0);

    assert_eq!(
        sql,
        "(((attrs->'speed'->>'value')::numeric > $1 AND (attrs->'brand'->>'value') = $2) OR ((attrs->'speed'->>'value')::numeric > $3 AND (attrs->'brand'->>'value') = $4))"
    );
    assert_eq!(params.len(), 4);
    assert_eq!(params[0], Literal::Number(80.0));
    assert_eq!(params[1], Literal::String("BMW".to_string()));
    assert_eq!(params[2], Literal::Number(100.0));
    assert_eq!(params[3], Literal::String("Audi".to_string()));
}

#[test]
fn test_sql_temporal_between_and_timeproperty_concatenation() {
    // Case 1: timerel=between with timeproperty=createdAt and attribute filter
    let builder = MockTemporalQueryBuilder {
        entity_id: "urn:ngsi-ld:Sensor:Device01".to_string(),
        timerel: "between".to_string(),
        time_at: "2026-09-20T00:00:00Z".to_string(),
        end_time_at: Some("2026-09-22T00:00:00Z".to_string()),
        timeproperty: "createdAt".to_string(),
        attrs: vec!["co2".to_string(), "humidity".to_string()],
        aggr_method: None,
        last_n: None,
    };

    let (sql, params) = builder.build_sql().expect("Build between query failed");
    assert!(sql.contains("created_at >= $2 AND created_at <= $3"));
    assert!(sql.contains("AND attribute_id = ANY($4)"));
    assert!(sql.contains("ORDER BY created_at ASC"));

    assert_eq!(params.len(), 4);
    assert_eq!(params[0].index, 1);
    assert_eq!(params[1].index, 2);
    assert_eq!(params[2].index, 3);
    assert_eq!(params[3].index, 4);
}

#[test]
fn test_sql_temporal_last_n_subquery_generation() {
    // Case 2: lastN=5 should generate a subquery with ORDER BY DESC LIMIT, and outer ASC
    let builder = MockTemporalQueryBuilder {
        entity_id: "urn:ngsi-ld:Sensor:Device01".to_string(),
        timerel: "after".to_string(),
        time_at: "2026-09-20T00:00:00Z".to_string(),
        end_time_at: None,
        timeproperty: "observedAt".to_string(),
        attrs: vec!["co2".to_string()],
        aggr_method: None,
        last_n: Some(5),
    };

    let (sql, params) = builder.build_sql().expect("Build lastN query failed");
    assert!(sql.contains("FROM (SELECT entity_id"));
    assert!(sql.contains("WHERE entity_id = $1 AND observed_at > $2 AND attribute_id = ANY($3)"));
    assert!(sql.contains("ORDER BY observed_at DESC LIMIT $4"));
    assert!(sql.ends_with(") sub ORDER BY observed_at ASC"));

    assert_eq!(params.len(), 4);
    assert_eq!(params[3].name, "lastN:5");
}

#[test]
fn test_sql_temporal_invalid_interval_validation() {
    // Case 3: timeAt > endTimeAt should return an Err validation failure
    let builder = MockTemporalQueryBuilder {
        entity_id: "urn:ngsi-ld:Sensor:Device01".to_string(),
        timerel: "between".to_string(),
        time_at: "2026-09-25T00:00:00Z".to_string(),
        end_time_at: Some("2026-09-20T00:00:00Z".to_string()), // inverted!
        timeproperty: "observedAt".to_string(),
        attrs: vec![],
        aggr_method: None,
        last_n: None,
    };

    let result = builder.build_sql();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("timeAt cannot be after endTimeAt"));
}

#[test]
fn test_sql_temporal_aggregation_query_generation() {
    // Case 4: aggrMethod=avg with modifiedAt and time window
    let builder = MockTemporalQueryBuilder {
        entity_id: "urn:ngsi-ld:Sensor:Device01".to_string(),
        timerel: "after".to_string(),
        time_at: "2026-09-21T00:00:00Z".to_string(),
        end_time_at: None,
        timeproperty: "modifiedAt".to_string(),
        attrs: vec!["co2".to_string()],
        aggr_method: Some("avg".to_string()),
        last_n: None,
    };

    let (sql, params) = builder.build_sql().expect("Build aggr query failed");
    assert!(sql.contains("AVG(value_numeric) as aggr_val"));
    assert!(sql.contains("MIN(modified_at) as start_time"));
    assert!(sql.contains("MAX(modified_at) as end_time"));
    assert!(sql.contains("GROUP BY attribute_id"));
    assert_eq!(params.len(), 3);
}
