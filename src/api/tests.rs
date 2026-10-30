use super::*;
use std::io::{Read as _, Write as _};

// One-request loopback server; all credentials in these tests are synthetic.
fn mock_server(
    status: u16,
    body: &'static str,
) -> (Client, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "request ended before body was complete");
            request.extend_from_slice(&buf[..n]);
            if let Some(end) =
                request.windows(4).position(|w| w == b"\r\n\r\n")
            {
                let headers = String::from_utf8_lossy(&request[..end]);
                let len = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + len {
                    break;
                }
            }
        }
        write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        String::from_utf8(request).unwrap()
    });
    (Client::new(&url, &url, &url, None), handle)
}

const ROTATED: &str =
    r#"{"access_token":"new-access","refresh_token":"new-refresh"}"#;
const ACCESS_ONLY: &str = r#"{"access_token":"new-access"}"#;
const REJECTED: &str = r#"{"error":"invalid_grant"}"#;

fn check_tokens(tokens: RefreshedTokens, rotated: bool) {
    let mut db = crate::db::Db::new();
    db.access_token = Some("old-access".into());
    db.refresh_token = Some("old-refresh".into());
    db.update_tokens(tokens);
    // Verify the database representation used on disk retains the new token.
    let stored = serde_json::to_string(&db).unwrap();
    let loaded: crate::db::Db = serde_json::from_str(&stored).unwrap();
    assert_eq!(loaded.access_token.as_deref(), Some("new-access"));
    assert_eq!(
        loaded.refresh_token.as_deref(),
        Some(if rotated {
            "new-refresh"
        } else {
            "old-refresh"
        })
    );
}

#[test]
fn blocking_refresh_retains_rotated_token() {
    let (client, server) = mock_server(200, ROTATED);
    check_tokens(client.exchange_refresh_token("old-refresh").unwrap(), true);
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /connect/token "));
    assert!(request.contains("refresh_token=old-refresh"));
}

#[tokio::test]
async fn async_refresh_retains_rotated_token() {
    let (client, server) = mock_server(200, ROTATED);
    check_tokens(
        client
            .exchange_refresh_token_async("old-refresh")
            .await
            .unwrap(),
        true,
    );
    server.join().unwrap();
}

#[test]
fn blocking_refresh_preserves_token_when_server_omits_replacement() {
    let (client, server) = mock_server(200, ACCESS_ONLY);
    check_tokens(
        client.exchange_refresh_token("old-refresh").unwrap(),
        false,
    );
    server.join().unwrap();
}

#[tokio::test]
async fn async_refresh_preserves_token_when_server_omits_replacement() {
    let (client, server) = mock_server(200, ACCESS_ONLY);
    check_tokens(
        client
            .exchange_refresh_token_async("old-refresh")
            .await
            .unwrap(),
        false,
    );
    server.join().unwrap();
}

#[test]
fn blocking_refresh_reports_rejected_session() {
    let (client, server) = mock_server(400, REJECTED);
    assert!(matches!(
        client.exchange_refresh_token("expired"),
        Err(Error::RefreshTokenRejected)
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn async_refresh_reports_rejected_session() {
    let (client, server) = mock_server(400, REJECTED);
    assert!(matches!(
        client.exchange_refresh_token_async("expired").await,
        Err(Error::RefreshTokenRejected)
    ));
    server.join().unwrap();
}

#[test]
fn blocking_refresh_preserves_server_error_status() {
    let (client, server) = mock_server(503, "unavailable");
    assert!(matches!(
        client.exchange_refresh_token("old-refresh"),
        Err(Error::RequestFailed { status: 503 })
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn async_refresh_preserves_server_error_status() {
    let (client, server) = mock_server(503, "unavailable");
    assert!(matches!(
        client.exchange_refresh_token_async("old-refresh").await,
        Err(Error::RequestFailed { status: 503 })
    ));
    server.join().unwrap();
}

#[test]
fn refresh_unauthorized_is_session_rejection() {
    assert!(matches!(
        classify_refresh_error(reqwest::StatusCode::UNAUTHORIZED, None),
        Error::RefreshTokenRejected
    ));
    assert!(matches!(
        classify_refresh_error(reqwest::StatusCode::BAD_REQUEST, None),
        Error::RequestFailed { status: 400 }
    ));
}

#[test]
fn create_sends_encrypted_for() {
    let (client, server) = mock_server(200, "{}");
    client
        .add(
            "access",
            "test-user-id",
            "encrypted-name",
            &crate::db::EntryData::SecureNote,
            None,
            None,
        )
        .unwrap();
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /ciphers "));
    let body: serde_json::Value =
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1)
            .unwrap();
    assert_eq!(body["encryptedFor"], "test-user-id");
}

#[test]
fn edit_sends_encrypted_for() {
    let (client, server) = mock_server(200, "{}");
    client
        .edit(
            "access",
            "test-user-id",
            "entry-id",
            None,
            "encrypted-name",
            &crate::db::EntryData::SecureNote,
            &[],
            None,
            None,
            &[],
        )
        .unwrap();
    let request = server.join().unwrap();
    assert!(request.starts_with("PUT /ciphers/entry-id "));
    let body: serde_json::Value =
        serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1)
            .unwrap();
    assert_eq!(body["encryptedFor"], "test-user-id");
}

#[test]
fn old_database_without_user_id_still_loads() {
    let mut old = serde_json::to_value(crate::db::Db::new()).unwrap();
    old.as_object_mut().unwrap().remove("user_id");
    let db: crate::db::Db = serde_json::from_value(old).unwrap();
    assert!(db.user_id.is_none());
}
