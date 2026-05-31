use rm_cloud::fake::FakeCloud;
use rm_cloud::{refresh_user_token, register_device, Config};

#[tokio::test]
async fn pairing_and_user_token() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let http = reqwest::Client::new();

    let device = register_device(&http, &config, "ABCD1234").await.unwrap();
    assert_eq!(device, "device-token-for-ABCD1234");

    let user = refresh_user_token(&http, &config, &device).await.unwrap();
    assert_eq!(user, "user-token");
}

#[tokio::test]
async fn empty_device_token_is_missing_credential() {
    let cloud = FakeCloud::spawn().await;
    let config = Config::single_host(&cloud.base);
    let http = reqwest::Client::new();
    let err = refresh_user_token(&http, &config, "").await.unwrap_err();
    assert!(matches!(err, rm_cloud::Error::MissingCredential(_)));
}
