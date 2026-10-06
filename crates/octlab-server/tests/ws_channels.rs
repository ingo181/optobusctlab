//! Spec 0005, `/ws`-Vertrag: Snapshot beim Verbinden, danach nur Änderungen
//! (AK7); Format aus Spec 0002 plus `stale`, `value: null` bei "nie
//! geantwortet" (AK8); unaufgeforderte Zeilen und Quittungen erscheinen
//! nicht (AK9); ein Setzen per POST zeigt sich nur als Kanaländerung des
//! Rücklesewerts (AK6).
//!
//! Echte Zeit und echte Sockets (WebSocket braucht ein echtes Upgrade), mit
//! kurzem Poll-Intervall. Der WebSocket-Client ist bewusst von Hand
//! geschrieben (Handshake + unmaskierte Server-Frames lesen) - für diesen
//! Test keine neue Dependency wert.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use octlab_lab::PollConfig;
use octlab_protocol::{ChannelKey, ModuleAddress, SubChannel};
use octlab_server::build_app_with_connection;
use octlab_transport::{SimBus, SimulatedConnection};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tower::util::ServiceExt;

/// Gelesene Frames ohne Wartezeit danach = "Stille" (Push nur bei Änderung).
const QUIET: Duration = Duration::from_millis(400);

fn key(address: u8, subchannel: u8) -> ChannelKey {
    ChannelKey {
        address: ModuleAddress(address),
        subchannel: SubChannel(subchannel),
    }
}

struct WsClient {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl WsClient {
    async fn connect(addr: std::net::SocketAddr) -> Self {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(
                b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n\
                  Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                  Sec-WebSocket-Version: 13\r\n\r\n",
            )
            .await
            .unwrap();
        let mut client = Self {
            stream,
            buffer: Vec::new(),
        };
        let header_end = loop {
            if let Some(pos) = client.buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            client.fill().await;
        };
        let header = String::from_utf8_lossy(&client.buffer[..header_end]).to_string();
        assert!(header.starts_with("HTTP/1.1 101"), "kein Upgrade: {header}");
        client.buffer.drain(..header_end);
        client
    }

    async fn fill(&mut self) {
        let mut chunk = [0u8; 4096];
        let n = self.stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "Server hat die Verbindung geschlossen");
        self.buffer.extend_from_slice(&chunk[..n]);
    }

    /// Nächster Text-Frame als JSON, oder `None`, wenn innerhalb `wait`
    /// nichts kommt.
    async fn next(&mut self, wait: Duration) -> Option<Value> {
        tokio::time::timeout(wait, async {
            loop {
                if self.buffer.len() >= 2 {
                    let mut len = usize::from(self.buffer[1] & 0x7f);
                    let mut offset = 2;
                    if len == 126 && self.buffer.len() >= 4 {
                        len = usize::from(u16::from_be_bytes([self.buffer[2], self.buffer[3]]));
                        offset = 4;
                    }
                    if len < 126 && self.buffer.len() >= offset + len {
                        let payload: Vec<u8> =
                            self.buffer.drain(..offset + len).skip(offset).collect();
                        return serde_json::from_slice(&payload).unwrap();
                    }
                }
                self.fill().await;
            }
        })
        .await
        .ok()
    }

    async fn expect_quiet(&mut self, what: &str) {
        if let Some(frame) = self.next(QUIET).await {
            panic!("{what}: unerwartete Nachricht {frame}");
        }
    }
}

fn dto(address: u8, subchannel: u8, value: Value, stale: bool) -> Value {
    json!({
        "address": address,
        "subchannel": subchannel,
        "value": value,
        "status_text": null,
        "stale": stale
    })
}

#[tokio::test]
async fn ws_liefert_snapshot_dann_nur_aenderungen_und_keine_unaufgeforderten_zeilen() {
    let bus = SimBus::new();
    bus.set_value(1, 0, "0.0022");
    bus.set_value(4, 0, "1000.0");
    bus.mute(2); // 2:10 antwortet nie
    let connection = SimulatedConnection::new("sim").with_bus(bus.clone());
    let config = PollConfig {
        channels: vec![key(1, 0), key(2, 10), key(4, 0)],
        interval: Duration::from_millis(100),
    };
    let app = build_app_with_connection(
        Box::new(connection),
        PathBuf::from("/nonexistent-frontend-dist"),
        config,
    )
    .await
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await });

    // Erster Zyklus: 2:10 läuft nach 500 ms ins Timeout.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let mut ws = WsClient::connect(addr).await;

    // AK7: Snapshot in Reihenfolge der Kanalliste; AK8: 0002-Format + stale,
    // value null für "nie geantwortet".
    let mut snapshot = Vec::new();
    for _ in 0..3 {
        snapshot.push(ws.next(Duration::from_secs(2)).await.expect("Snapshot"));
    }
    assert_eq!(
        snapshot,
        vec![
            dto(1, 0, json!(0.0022), false),
            dto(2, 10, Value::Null, true),
            dto(4, 0, json!(1000.0), false),
        ]
    );

    // AK7: Unveränderte Werte werden nicht erneut gepusht.
    ws.expect_quiet("konstante Werte").await;

    // AK7: Änderung wird gepusht.
    bus.set_value(1, 0, "0.5");
    assert_eq!(
        ws.next(Duration::from_secs(2)).await,
        Some(dto(1, 0, json!(0.5), false))
    );
    ws.expect_quiet("nach der Änderung").await;

    // AK9: Unaufgeforderter Wert und Statuszeile erscheinen nicht.
    bus.inject("#4:0=440.0");
    bus.inject("#2:255=65");
    ws.expect_quiet("unaufgeforderte Zeilen").await;

    // AK6/AK9: Setzen per POST - auf /ws erscheint nur die Änderung des
    // Kanals durch den Rücklesewert, keine Quittung (Subkanal 255).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/channel/4/0")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"value":2500}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        ws.next(Duration::from_secs(2)).await,
        Some(dto(4, 0, json!(2500.0), false))
    );
    ws.expect_quiet("nach dem Setzen").await;
}
