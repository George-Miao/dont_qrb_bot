use std::time::Duration;

use compio::{
    io::{AsyncRead, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use super::*;

#[compio::test]
async fn stalled_poll_releases_request() {
    check_recovery(false).await;
}

#[compio::test]
async fn stalled_sticker_body_releases_request() {
    check_recovery(true).await;
}

#[compio::test]
async fn rejected_poll_reports_telegram_error() {
    for status in ["409 Conflict", "200 OK"] {
        let body = r#"{"ok":false,"error_code":409,"description":"Conflict: terminated by other getUpdates request; make sure that only one bot instance is running"}"#;
        let error = rejected_poll(status, body).await;
        let Error::Api(error) = error else {
            panic!("Expected a Telegram error, got {error:?}");
        };
        assert_eq!(error.error_code, 409);
        assert_eq!(
            error.description,
            "Conflict: terminated by other getUpdates request; make sure that only one bot instance is running"
        );
    }
}

#[compio::test]
async fn rate_limit_retains_retry_delay() {
    let body = r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 17","parameters":{"retry_after":17}}"#;
    let error = rejected_poll("429 Too Many Requests", body).await;
    let Error::Api(error) = error else {
        panic!("Expected a Telegram error, got {error:?}");
    };
    assert_eq!(error.error_code, 429);
    assert_eq!(error.parameters.unwrap().retry_after, Some(17));
}

#[compio::test]
async fn missing_success_result_remains_a_decode_error() {
    let error = rejected_poll("200 OK", r#"{"ok":true}"#).await;
    assert!(
        matches!(error, Error::Client(cyper::Error::Json(_))),
        "{error:?}"
    );
}

async fn rejected_poll(status: &'static str, body: &'static str) -> Error {
    timeout(Duration::from_secs(2), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bot = Bot {
            api_url: format!("http://{}/bot-test", listener.local_addr().unwrap()),
            client: Client::new(),
            timeout: Duration::from_secs(1),
        };
        let server = compio::runtime::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream, "getUpdates").await;
            stream
                .write_all(format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ))
                .await
                .0
                .unwrap();
        });

        let error = bot
            .get_updates(&GetUpdatesParams::builder().build())
            .await
            .expect_err("The response must be rejected");
        server.await.unwrap();
        error
    })
    .await
    .unwrap()
}

async fn check_recovery(partial_body: bool) {
    timeout(Duration::from_secs(2), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bot = Bot {
            api_url: format!("http://{}/bot-test", listener.local_addr().unwrap()),
            client: Client::new(),
            timeout: Duration::from_millis(100),
        };
        let server = compio::runtime::spawn(async move {
            let (mut stalled, _) = listener.accept().await.unwrap();
            let method = if partial_body {
                "sendSticker"
            } else {
                "getUpdates"
            };
            read_request(&mut stalled, method).await;
            if partial_body {
                stalled
                    .write_all(
                        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"ok\":true,",
                    )
                    .await
                    .0
                    .unwrap();
            }

            // Keep the stalled connection open until the next poll succeeds.
            let (mut next, _) = listener.accept().await.unwrap();
            read_request(&mut next, "getUpdates").await;
            next.write_all(
                "HTTP/1.1 200 OK\r\nContent-Length: 23\r\nConnection: close\r\n\r\n{\"ok\":true,\"result\":[]}",
            )
            .await
            .0
            .unwrap();
            drop(stalled);
        });

        let result = if partial_body {
            let params = SendStickerParams::builder()
                .chat_id(1i64)
                .sticker(FileUpload::String("test-sticker".into()))
                .build();
            bot.send_sticker(&params).await.map(|_| ())
        } else {
            bot.get_updates(&GetUpdatesParams::builder().timeout(30u32).build())
                .await
                .map(|_| ())
        };
        assert!(matches!(result, Err(Error::Client(cyper::Error::Timeout))), "{result:?}");

        let updates = bot
            .get_updates(&GetUpdatesParams::builder().build())
            .await
            .expect("Polling must recover after a stalled request");
        assert!(updates.result.is_empty());
        server.await.unwrap();
    })
    .await
    .expect("The bot stayed alive but could not resume polling");
}

async fn read_request(stream: &mut TcpStream, method: &str) {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let (len, bytes) = stream.read(Vec::with_capacity(1024)).await.unwrap();
        assert_ne!(
            len, 0,
            "The client closed the request before sending headers"
        );
        request.extend_from_slice(&bytes);
    }
    assert!(request.starts_with(format!("POST /bot-test/{method} HTTP/1.1\r\n").as_bytes()));
}
