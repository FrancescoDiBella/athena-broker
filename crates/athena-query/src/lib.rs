pub mod ast;
pub mod geo_parser;
pub mod lexer;
pub mod parser;
pub mod sql_compiler;

pub use ast::{CompareOp, GeoQuery, GeoRel, Literal, LogicalOp, QueryExpr};
pub use geo_parser::{GeoParserError, GeoQueryParser};
pub use lexer::{Lexer, LexerError, Token};
pub use parser::{Parser, ParserError};
pub use sql_compiler::{CompiledQuery, SqlCompiler, SqlParam};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_retain_every_element_and_complexity_is_bounded() {
        assert_eq!(
            Parser::parse_str("reading==1,2,3").unwrap(),
            QueryExpr::InList {
                path: "reading".into(),
                values: vec![
                    Literal::Number(1.),
                    Literal::Number(2.),
                    Literal::Number(3.)
                ]
            }
        );
        assert!(
            Parser::parse_str(&format!("{}reading==1{}", "(".repeat(33), ")".repeat(33))).is_err()
        );
        assert!(Parser::parse_str(&"x".repeat(16385)).is_err());
    }
    #[test]
    fn test_parse_simple_comparison() {
        let q = "speed>50";
        let expr = Parser::parse_str(q).unwrap();
        assert_eq!(
            expr,
            QueryExpr::Comparison {
                path: "speed".to_string(),
                op: CompareOp::GreaterThan,
                value: Literal::Number(50.0),
            }
        );
    }

    #[test]
    fn test_parse_and_or_precedence() {
        // "a==1;b==2|c==3" should parse as ((a==1 AND b==2) OR c==3)
        let q = "a==1;b==2|c==3";
        let expr = Parser::parse_str(q).unwrap();
        match expr {
            QueryExpr::Binary { op, left, right } => {
                assert_eq!(op, LogicalOp::Or);
                assert!(matches!(
                    *left,
                    QueryExpr::Binary {
                        op: LogicalOp::And,
                        ..
                    }
                ));
                assert!(matches!(*right, QueryExpr::Comparison { .. }));
            }
            _ => panic!("Expected Binary expression"),
        }
    }

    #[test]
    fn test_parse_parentheses() {
        let q = "(speed>50|speed<10);brand=='BMW'";
        let expr = Parser::parse_str(q).unwrap();
        match expr {
            QueryExpr::Binary { op, left, right } => {
                assert_eq!(op, LogicalOp::And);
                assert!(matches!(
                    *left,
                    QueryExpr::Binary {
                        op: LogicalOp::Or,
                        ..
                    }
                ));
                assert!(matches!(*right, QueryExpr::Comparison { .. }));
            }
            _ => panic!("Expected Binary expression"),
        }
    }

    #[test]
    fn test_parse_range() {
        let q = "speed==20..80";
        let expr = Parser::parse_str(q).unwrap();
        assert_eq!(
            expr,
            QueryExpr::Range {
                path: "speed".to_string(),
                min: Literal::Number(20.0),
                max: Literal::Number(80.0),
            }
        );
    }

    #[test]
    fn test_parse_pattern_match() {
        let q = "brand~='^Tesla.*'";
        let expr = Parser::parse_str(q).unwrap();
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
        let q = "speed>50;brand=='Mercedes'";
        let expr = Parser::parse_str(q).unwrap();
        let compiled = SqlCompiler::compile_q(&expr, 0);

        assert!(!compiled.where_clause.contains("Mercedes"));
        assert!(!compiled.where_clause.contains("speed"));
        assert_eq!(compiled.params.len(), 4);
        assert_eq!(compiled.params[1], SqlParam::Number(50.0));
        assert_eq!(compiled.params[3], SqlParam::String("Mercedes".to_string()));
    }

    #[test]
    fn test_geo_parser_and_compiler() {
        let geo = GeoQueryParser::parse(
            Some("near;maxDistance==1000"),
            Some("Point"),
            Some("[13.4050, 52.5200]"),
            Some("location"),
        )
        .unwrap()
        .unwrap();

        let compiled = SqlCompiler::compile_geo(&geo, 1);
        assert!(compiled.where_clause.contains("ST_DWithin"));
        assert_eq!(compiled.params.last(), Some(&SqlParam::Number(1000.0)));
        assert!(compiled
            .params
            .contains(&SqlParam::String("location".into())));
    }
}
