//! Manual live ICS fetch. Run with: cargo test -p inkapp-ics --test live -- --ignored
use inkapp_core::connector::Connector;
use inkapp_ics::{IcsConfig, IcsConnector};

#[tokio::test]
#[ignore = "hits the network; set ICS_TEST_URL"]
async fn fetches_real_feed() {
    let url = std::env::var("ICS_TEST_URL").expect("set ICS_TEST_URL");
    let c = IcsConnector::from_config(&IcsConfig { url });
    c.refresh().await.expect("refresh");
    assert!(!c.events().is_empty());
}
