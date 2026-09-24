use athena_query::{Parser, SqlCompiler};
use std::{hint::black_box, time::Instant};

fn main() {
    let query = "speed>80;brand=='Mercedes';(temperature>25|humidity<40)";
    let iterations = 100_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let expr = Parser::parse_str(black_box(query)).unwrap();
        black_box(SqlCompiler::compile_q(&expr, 0));
    }
    println!(
        "production parser + compiler: {:.0} operations/s",
        iterations as f64 / start.elapsed().as_secs_f64()
    );
}
