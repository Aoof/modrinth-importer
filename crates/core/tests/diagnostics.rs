use mrimport_core::{curseforge::CurseForge, debuglog, Error};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "$2a$10$SECRETSECRETSECRETSECRETSECRETSECRETSECRETSECRETSECRETxy";

#[tokio::test]
async fn forbidden_gives_hint_and_a_safe_log() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("server", "cloudflare")
                .insert_header("cf-ray", "abc123-AMS")
                .set_body_string("<html>Forbidden</html>"),
        )
        .mount(&server)
        .await;

    let err = CurseForge::with_base(server.uri(), KEY)
        .check_key()
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, Error::Api { status: 403, .. }));
    assert!(
        msg.contains("hint:") && msg.contains("single quotes"),
        "{msg}"
    );

    let log = debuglog::snapshot();
    assert!(log.contains("GET /games -> 403"), "{log}");
    assert!(log.contains("cf-ray=abc123-AMS") && log.contains("server=cloudflare"));
    assert!(log.contains("Forbidden"));
    assert!(
        !log.contains("SECRETSECRET"),
        "the API key leaked into the log"
    );
}

#[tokio::test]
async fn key_is_sent_trimmed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games"))
        .and(header("x-api-key", "abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
        .mount(&server)
        .await;
    CurseForge::with_base(server.uri(), "  abc\n")
        .check_key()
        .await
        .expect("trimmed key should be accepted");
}
