//! A gateway that takes POSTs, answers 202, and hands each request's headers and body to the test.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub struct Received {
    pub head: String,
    pub body: serde_json::Value,
}

pub async fn gateway() -> (String, mpsc::UnboundedReceiver<Received>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/events", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    let Some(split) = text.find("\r\n\r\n") else { continue };
                    let head = text[..split].to_string();
                    let length: usize = head
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                        .unwrap_or(0);
                    if buf.len() < split + 4 + length {
                        continue;
                    }
                    let body = serde_json::from_slice(&buf[split + 4..split + 4 + length]).unwrap();
                    let _ = socket
                        .write_all(b"HTTP/1.1 202 Accepted\r\ncontent-length: 14\r\ncontent-type: application/json\r\n\r\n{\"accepted\":1}")
                        .await;
                    let _ = tx.send(Received { head, body });
                    buf.clear();
                }
            });
        }
    });
    (url, rx)
}

/// Every event received within `wait`.
pub async fn events(rx: &mut mpsc::UnboundedReceiver<Received>, wait: std::time::Duration) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut heads = Vec::new();
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + wait;
    while let Ok(Some(r)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        heads.push(r.head);
        events.extend(r.body.as_array().unwrap().iter().cloned());
    }
    (heads, events)
}
