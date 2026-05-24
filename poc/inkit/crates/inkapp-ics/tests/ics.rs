use inkapp_core::connector::Connector;
use inkapp_ics::IcsConnector;
use std::sync::Arc;

const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\nSUMMARY:Standup\r\nDTSTART:20260525T090000Z\r\nDTEND:20260525T091500Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[tokio::test]
async fn parses_and_caches_events() {
    let c = IcsConnector::from_ics(SAMPLE);
    let evs = c.events();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].uid, "e1");
    assert_eq!(evs[0].summary, "Standup");
    assert_eq!(evs[0].start, "20260525T090000Z");
    assert!(!evs[0].cancelled);
}

#[tokio::test]
async fn refresh_repopulates_cache() {
    let c = Arc::new(IcsConnector::from_ics(SAMPLE));
    let (a, b) = tokio::join!(c.refresh(), c.refresh());
    a.unwrap();
    b.unwrap();
    assert_eq!(c.events().len(), 1);
}

#[tokio::test]
async fn flush_is_noop() {
    let c = IcsConnector::from_ics(SAMPLE);
    c.flush().await;
    assert_eq!(c.events().len(), 1);
}

#[test]
fn fixture_feed_parses() {
    let c = IcsConnector::from_fixture();
    assert!(c.events().len() >= 2, "fixture feed has at least two events");
}
