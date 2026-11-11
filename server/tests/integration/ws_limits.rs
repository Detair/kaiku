//! Caps on concurrent WebSocket connections per user and per client IP.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use vc_server::api::create_router;
use vc_server::ratelimit::TrustedProxies;
use vc_server::ws::connection_limit::{ConnectionLimits, WsConnectionTracker};

use super::helpers::{
    create_test_user, generate_access_token, next_ws_event, spawn_test_server, ws_connect, TestApp,
    TestServer, WsClient,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

/// Wait for the server's Close frame and return its code and reason.
async fn close_frame(ws: &mut WsClient) -> (u16, String) {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(Some(frame)))) => {
                    return (u16::from(frame.code), frame.reason.to_string());
                }
                Some(Ok(Message::Close(None))) | None => panic!("closed without a close frame"),
                Some(Ok(Message::Text(text))) => {
                    assert!(
                        !text.contains("\"ready\""),
                        "refused socket got ready: {text}"
                    );
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket error: {e}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("no close frame in time")
}

async fn connect_ready(server: &TestServer, token: &str) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    ws
}

async fn assert_alive(ws: &mut WsClient) {
    ws.send(Message::Text(
        serde_json::json!({ "type": "ping" }).to_string().into(),
    ))
    .await
    .expect("send ping");
    next_ws_event(ws, "pong", EVENT_TIMEOUT).await;
}

/// Connect with header auth plus extra headers; returns the HTTP status of a
/// refused handshake, or the socket.
async fn connect_with_headers(
    server: &TestServer,
    token: Option<&str>,
    headers: &[(&'static str, String)],
) -> Result<WsClient, u16> {
    let mut request = format!("ws://{}/ws", server.addr)
        .into_client_request()
        .expect("request");
    if let Some(token) = token {
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            format!("access_token, access_token.{token}")
                .parse()
                .expect("header"),
        );
    }
    for (name, value) in headers {
        request
            .headers_mut()
            .insert(*name, value.parse().expect("header"));
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((ws, _)) => Ok(ws),
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => Err(resp.status().as_u16()),
        Err(e) => panic!("unexpected connect error: {e}"),
    }
}

#[sqlx::test]
async fn eleventh_socket_of_user_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let mut open = Vec::new();
    for _ in 0..10 {
        open.push(connect_ready(&server, &token).await);
    }

    let mut eleventh = ws_connect(&server, &token).await;
    let (code, reason) = close_frame(&mut eleventh).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "TOO_MANY_CONNECTIONS");

    for ws in &mut open {
        assert_alive(ws).await;
    }
    assert_eq!(app.state.ws_connections.user_connections(user), 10);
}

#[sqlx::test]
async fn closing_a_socket_frees_a_slot(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let mut open = Vec::new();
    for _ in 0..10 {
        open.push(connect_ready(&server, &token).await);
    }
    let mut first = open.remove(0);
    first.close(None).await.expect("close");
    drop(first);

    // The slot is released once the socket task has finished its cleanup.
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while app.state.ws_connections.user_connections(user) > 9 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "slot not released after close"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut again = connect_ready(&server, &token).await;
    assert_alive(&mut again).await;
}

#[sqlx::test]
async fn header_auth_over_user_cap_gets_429(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let mut open = Vec::new();
    for _ in 0..10 {
        open.push(connect_ready(&server, &token).await);
    }
    let refused = connect_with_headers(&server, Some(&token), &[]).await;
    assert_eq!(refused.err(), Some(429));
    for ws in &mut open {
        assert_alive(ws).await;
    }
}

/// Behind a trusted proxy the forwarded client address is what counts: one
/// client hits the cap, another client through the same proxy does not.
#[sqlx::test]
async fn per_ip_cap_uses_trusted_proxy_header(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let mut state = app.state.clone();
    state.ws_connections = Arc::new(WsConnectionTracker::new(
        ConnectionLimits {
            per_user: 10,
            per_ip: 3,
        },
        TrustedProxies::parse("127.0.0.1/32").expect("trusted"),
    ));
    let server = spawn_test_server(create_router(state)).await;

    let xff = |ip: &str| vec![("X-Forwarded-For", ip.to_string())];
    let mut open = Vec::new();
    for _ in 0..3 {
        let (user, _) = create_test_user(&pool).await;
        let token = generate_access_token(&app.config, user);
        let mut ws = connect_with_headers(&server, Some(&token), &xff("203.0.113.7"))
            .await
            .expect("under the IP cap");
        next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
        open.push(ws);
    }

    // A fourth socket from the same client is refused before the upgrade,
    // with header auth and without (frame auth).
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);
    let refused = connect_with_headers(&server, Some(&token), &xff("203.0.113.7")).await;
    assert_eq!(refused.err(), Some(429));
    let refused = connect_with_headers(&server, None, &xff("203.0.113.7")).await;
    assert_eq!(refused.err(), Some(429));

    // Another client behind the same proxy still connects.
    let mut other = connect_with_headers(&server, Some(&token), &xff("203.0.113.8"))
        .await
        .expect("other client");
    next_ws_event(&mut other, "ready", EVENT_TIMEOUT).await;

    for ws in &mut open {
        assert_alive(ws).await;
    }
}
