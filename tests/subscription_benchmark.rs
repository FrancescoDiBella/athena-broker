use athena_model::{Entity, Subscription};
use athena_subscription::matcher::SubscriptionIndex;
use serde_json::json;
use std::{hint::black_box, time::Instant};

#[test]
#[ignore = "explicit release-mode matching component benchmark"]
fn subscription_candidate_matching() {
    let subscriptions: Vec<Subscription> = (0..1000)
        .map(|i| {
            serde_json::from_value(json!({
                "id":format!("urn:ngsi-ld:Subscription:{i}"),"type":"Subscription",
                "entities":[{"type":format!("Sensor{}",i%100),"idPattern":"^urn:ngsi-ld:perf:"}],
                "watchedAttributes":["temperature"],"q":"temperature>20;temperature<50",
                "notification":{"endpoint":{"uri":"https://example.org/notify"}}
            }))
            .unwrap()
        })
        .collect();
    let started = Instant::now();
    let index = SubscriptionIndex::new(subscriptions);
    let build_ms = started.elapsed().as_secs_f64() * 1000.;
    let entity=Entity::from_json(json!({"id":"urn:ngsi-ld:perf:one","type":"Sensor7","temperature":{"type":"Property","value":25}})).unwrap();
    let attrs = vec!["temperature".into()];
    let candidates = index.candidates(&entity);
    assert_eq!(candidates.len(), 10);
    let started = Instant::now();
    let mut matches = 0usize;
    for _ in 0..10000 {
        for candidate in index.candidates(black_box(&entity)) {
            if candidate.matches(black_box(&entity), &attrs) {
                matches += 1;
            }
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    assert_eq!(matches, 100000);
    println!(
        "{}",
        json!({"component":"subscription_matching","subscriptions":1000,"entity_types":100,"events":10000,"candidates_per_event":10,"matches":matches,"index_build_ms":build_ms,"events_per_second":10000./elapsed,"includes_database_or_http":false})
    );
}
