mod common;

use std::time::Duration;

use clicklog::{Config, Event, Severity};

#[tokio::test]
async fn events_reach_the_gateway_in_a_batch_with_the_key() {
    assert!(!clicklog::enabled());
    Event::new("before", "init", Severity::Info).emit(); // off: dropped, no panic

    let (url, mut rx) = common::gateway().await;
    let mut config = Config::new(&url, "ik_test");
    config.server = "test-server".into();
    config.flush_interval = Duration::from_millis(100);
    assert!(clicklog::init(config.clone()));
    assert!(!clicklog::init(config), "a second init changes nothing");

    for i in 0..3 {
        Event::new("job", "done", Severity::Info).entity("job", i).emit();
    }
    Event::new("job", "done", Severity::Warn).server("elsewhere").emit();

    let (heads, events) = common::events(&mut rx, Duration::from_secs(2)).await;
    assert_eq!(heads.len(), 1, "one batch");
    assert!(heads[0].to_ascii_lowercase().contains("authorization: bearer ik_test"));
    assert!(heads[0].starts_with("POST /v1/events "));
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["server"], "test-server");
    assert_eq!(events[2]["entity_id"], "2");
    assert_eq!(events[3]["server"], "elsewhere");
    assert!(events.iter().all(|e| e.get("before").is_none() && e["category"] == "job"));
}
