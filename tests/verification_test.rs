use athena_jsonld::core_context::*;
use athena_query::*;

#[test]
fn test_core_context_definitions() {
    assert!(is_etsi_core_context(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
    ));
    assert!(is_etsi_core_context(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld"
    ));
    assert!(is_etsi_core_context(
        "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"
    ));

    let mappings = get_etsi_core_mappings();
    assert_eq!(
        mappings.get("Property").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/Property")
    );
    assert_eq!(
        mappings.get("Relationship").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/Relationship")
    );
    assert_eq!(
        mappings.get("GeoProperty").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/GeoProperty")
    );
    assert_eq!(
        mappings.get("value").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/hasValue")
    );
    assert_eq!(
        mappings.get("object").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/hasObject")
    );
    assert_eq!(
        mappings.get("observedAt").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/observedAt")
    );
    assert_eq!(
        mappings.get("createdAt").map(String::as_str),
        Some("https://uri.etsi.org/ngsi-ld/createdAt")
    );
}

#[test]
fn test_lexer_tokens() {
    let input = r#"speed>80;brand=="Tesla"|(temp>=20;temp<=30);model~='^Model.*'"#;
    let mut lexer = Lexer::new(input);
    let tokens = lexer.tokenize().expect("Tokenization failed");

    assert_eq!(tokens[0], Token::Ident("speed".to_string()));
    assert_eq!(tokens[1], Token::Greater);
    assert_eq!(tokens[2], Token::NumberLit(80.0));
    assert_eq!(tokens[3], Token::And);
    assert_eq!(tokens[4], Token::Ident("brand".to_string()));
    assert_eq!(tokens[5], Token::Equal);
    assert_eq!(tokens[6], Token::StringLit("Tesla".to_string()));
    assert_eq!(tokens[7], Token::Or);
    assert_eq!(tokens[8], Token::LParen);
}

#[test]
fn test_parser_precedence_and_grouping() {
    // a==1;b==2|c==3 must parse as ((a==1 AND b==2) OR c==3)
    let q = "a==1;b==2|c==3";
    let expr = Parser::parse_str(q).expect("Parsing failed");

    match expr {
        QueryExpr::Binary { op, left, right } => {
            assert_eq!(op, LogicalOp::Or);
            match *left {
                QueryExpr::Binary { op: l_op, .. } => assert_eq!(l_op, LogicalOp::And),
                _ => panic!("Expected AND on left"),
            }
            match *right {
                QueryExpr::Comparison { path, op: r_op, .. } => {
                    assert_eq!(path, "c");
                    assert_eq!(r_op, CompareOp::Equal);
                }
                _ => panic!("Expected Comparison on right"),
            }
        }
        _ => panic!("Expected Binary OR root"),
    }
}

#[test]
fn test_parser_parentheses_overrides_precedence() {
    let q = "a==1;(b==2|c==3)";
    let expr = Parser::parse_str(q).expect("Parsing failed");

    match expr {
        QueryExpr::Binary { op, left: _, right } => {
            assert_eq!(op, LogicalOp::And);
            match *right {
                QueryExpr::Binary { op: r_op, .. } => assert_eq!(r_op, LogicalOp::Or),
                _ => panic!("Expected OR on right"),
            }
        }
        _ => panic!("Expected Binary AND root"),
    }
}

#[test]
fn test_parser_ranges_and_patterns() {
    let q_range = "speed==50..120";
    let expr_range = Parser::parse_str(q_range).expect("Parsing range failed");
    assert_eq!(
        expr_range,
        QueryExpr::Range {
            path: "speed".to_string(),
            min: Literal::Number(50.0),
            max: Literal::Number(120.0),
        }
    );

    let q_pattern = "brand~='^BMW.*'";
    let expr_pattern = Parser::parse_str(q_pattern).expect("Parsing pattern failed");
    assert_eq!(
        expr_pattern,
        QueryExpr::PatternMatch {
            path: "brand".to_string(),
            pattern: "^BMW.*".to_string(),
            negated: false,
        }
    );
}

#[test]
fn test_sql_compilation_parametrized() {
    let q = "speed>80;brand=='Mercedes';active==true";
    let expr = Parser::parse_str(q).expect("Parsing failed");
    let compiled = SqlCompiler::compile_q(&expr, 0);

    assert!(!compiled.where_clause.contains("Mercedes"));
    assert!(!compiled.where_clause.contains("speed"));
    assert_eq!(compiled.params.len(), 6);
    assert_eq!(
        compiled.params[0],
        SqlParam::StringList(vec!["speed".into(), "value".into()])
    );
    assert_eq!(compiled.params[1], SqlParam::Number(80.0));
    assert_eq!(compiled.params[3], SqlParam::String("Mercedes".into()));
    assert_eq!(compiled.params[5], SqlParam::Boolean(true));
}

#[test]
fn test_geo_parser_and_postgis_compilation() {
    let geo = GeoQueryParser::parse(
        Some("near;maxDistance==2500"),
        Some("Point"),
        Some("[13.4050, 52.5200]"),
        Some("location"),
    )
    .expect("Geo parse failed")
    .expect("Expected Some(GeoQuery)");

    let compiled = SqlCompiler::compile_geo(&geo, 1);
    assert!(compiled.where_clause.contains("ST_DWithin"));
    assert!(compiled.where_clause.contains("::geography"));
    assert!(!compiled.where_clause.contains("2500"));
    assert_eq!(compiled.params.last(), Some(&SqlParam::Number(2500.0)));

    // Test polygon containment
    let geo_within = GeoQueryParser::parse(
        Some("within"),
        Some("Polygon"),
        Some("[[[13.0,52.0],[14.0,52.0],[14.0,53.0],[13.0,53.0],[13.0,52.0]]]"),
        Some("location"),
    )
    .expect("Geo within failed")
    .expect("Expected Some");

    let compiled_within = SqlCompiler::compile_geo(&geo_within, 1);
    assert!(compiled_within.where_clause.contains("ST_Within"));
    assert!(!compiled_within.where_clause.contains("13.0"));
}
