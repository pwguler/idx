//! IDX disclosure endpoints used by the pipeline: official announcements by day
//! and the paged news feed.

use std::time::Duration;

use serde_json::json;
use stockbit_cli::error::Error;
use stockbit_cli::idx::{IdxClient, announcement, news};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> IdxClient {
    IdxClient::with_base(&server.uri(), None)
        .expect("idx client builds")
        .with_retry_base(Duration::from_millis(1))
}

#[tokio::test]
async fn announcements_query_one_day_with_compact_dates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/ListedCompany/GetAnnouncement"))
        .and(query_param("kodeEmiten", ""))
        .and(query_param("emitenType", "s"))
        .and(query_param("indexFrom", "0"))
        .and(query_param("pageSize", "9999"))
        .and(query_param("dateFrom", "20261110"))
        .and(query_param("dateTo", "20261110"))
        .and(query_param("lang", "id"))
        .and(query_param("keyword", ""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"Replies": []})))
        .expect(1)
        .mount(&server)
        .await;
    let v = announcement::fetch(&client(&server), "2026-11-10")
        .await
        .unwrap();
    assert_eq!(v, json!({"Replies": []}));
}

#[tokio::test]
async fn announcements_reject_a_bad_date_before_network() {
    let server = MockServer::start().await;
    let err = announcement::fetch(&client(&server), "2026-02-30")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidDate { .. }), "{err:?}");
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn news_fetches_the_requested_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/primary/NewsAnnouncement/GetAllAnnouncement"))
        .and(query_param("keywords", ""))
        .and(query_param("pageNumber", "3"))
        .and(query_param("pageSize", "200"))
        .and(query_param("lang", "id"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"Items": [], "ItemCount": 0})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let v = news::fetch_page(&client(&server), 3, 200).await.unwrap();
    assert_eq!(v, json!({"Items": [], "ItemCount": 0}));
}
