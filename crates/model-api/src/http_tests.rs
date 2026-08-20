mod http_tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::RankCandidate;

    struct MockServer {
        url: Url,
        requests: Arc<AtomicUsize>,
        received: Arc<Mutex<Vec<String>>>,
        task: JoinHandle<()>,
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn mock_server(responses: Vec<(u16, String)>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(Mutex::new(Vec::new()));
        let requests_task = requests.clone();
        let received_task = received.clone();
        let task = tokio::spawn(async move {
            for (status, body) in responses {
                let (stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    stream.readable().await.unwrap();
                    let mut chunk = [0_u8; 16 * 1024];
                    match stream.try_read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => {
                            request.extend_from_slice(&chunk[..read]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(error) => panic!("mock read failed: {error}"),
                    }
                }
                requests_task.fetch_add(1, Ordering::SeqCst);
                received_task
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let reason = if status == 200 { "OK" } else { "Error" };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let bytes = response.as_bytes();
                let mut written = 0;
                while written < bytes.len() {
                    stream.writable().await.unwrap();
                    match stream.try_write(&bytes[written..]) {
                        Ok(0) => break,
                        Ok(count) => written += count,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(error) => panic!("mock write failed: {error}"),
                    }
                }
            }
        });
        MockServer {
            url: Url::parse(&format!("http://{address}/v1/")).unwrap(),
            requests,
            received,
            task,
        }
    }

    fn config(url: Url) -> HttpProviderConfig {
        let mut config = HttpProviderConfig::new(
            ModelIdentity {
                provider: "fixture".into(),
                model: "fixture-model".into(),
                version: "1".into(),
            },
            url,
        );
        config.api_key = Some("fixture-key".into());
        config.timeout = Duration::from_secs(2);
        config
    }

    #[tokio::test]
    async fn embedding_retries_transient_failure_and_sends_bearer_auth() {
        let server = mock_server(vec![
            (500, "{}".into()),
            (200, r#"{"data":[{"index":0,"embedding":[0.25,0.75]}]}"#.into()),
        ])
        .await;
        let provider = OpenAiCompatibleProvider::new(config(server.url.clone())).unwrap();
        let output = provider
            .embed(&[EmbeddingInput {
                id: "one".into(),
                text: "bounded fixture".into(),
            }])
            .await
            .unwrap();
        assert_eq!(output[0].vector, vec![0.25, 0.75]);
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
        assert!(server.received.lock().unwrap().iter().all(|request| {
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-key")
        }));
    }

    #[tokio::test]
    async fn rejected_request_is_not_retried() {
        let server = mock_server(vec![(400, "{}".into())]).await;
        let mut settings = config(server.url.clone());
        settings.retries = 3;
        let provider = OpenAiCompatibleProvider::new(settings).unwrap();
        let error = provider
            .embed(&[EmbeddingInput {
                id: "one".into(),
                text: "fixture".into(),
            }])
            .await
            .unwrap_err();
        assert!(matches!(error, ModelError::Request(_)));
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn oversized_response_is_rejected_without_retry() {
        let server = mock_server(vec![(200, format!(r#"{{"padding":"{}"}}"#, "x".repeat(2048)))]).await;
        let mut settings = config(server.url.clone());
        settings.maximum_response_bytes = 1024;
        settings.retries = 3;
        let provider = OpenAiCompatibleProvider::new(settings).unwrap();
        let error = provider
            .embed(&[EmbeddingInput {
                id: "one".into(),
                text: "fixture".into(),
            }])
            .await
            .unwrap_err();
        assert!(matches!(error, ModelError::InvalidOutput(_)));
        assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn summary_accepts_gemma_json_prefix_and_returns_topics() {
        let malformed = "json\n{```json\n{\"summary\":\"A concrete result.\",\"tags\":[{\"label\":\"Rust\",\"confidence\":0.93}]}";
        let body = json!({"choices":[{"message":{"content":malformed}}]}).to_string();
        let server = mock_server(vec![(200, body)]).await;
        let provider = OpenAiCompatibleProvider::new(config(server.url.clone())).unwrap();
        let output = provider
            .summarize(SummaryRequest {
                title: "Rust result".into(),
                source: None,
                author: None,
                canonical_url: None,
                language: Some("en".into()),
                text: "Rust improved this benchmark with a concrete implementation.".into(),
                custom_instruction: None,
            })
            .await
            .unwrap();
        assert_eq!(output.text, "A concrete result.");
        assert_eq!(output.tags, vec![TopicTag { label: "rust".into(), confidence: 0.93 }]);
        assert!(output.include);
    }

    #[tokio::test]
    async fn summary_returns_source_filter_decision() {
        let content = r#"{"include":false,"summary":"Not relevant.","tags":["technology"]}"#;
        let body = json!({"choices":[{"message":{"content":content}}]}).to_string();
        let server = mock_server(vec![(200, body)]).await;
        let provider = OpenAiCompatibleProvider::new(config(server.url.clone())).unwrap();
        let output = provider
            .summarize(SummaryRequest {
                title: "Product launch".into(),
                source: None,
                author: None,
                canonical_url: None,
                language: Some("en".into()),
                text: "A product launched today.".into(),
                custom_instruction: Some("Remove product launches from the feed".into()),
            })
            .await
            .unwrap();
        assert!(!output.include);
    }

    #[tokio::test]
    async fn chat_ranker_keeps_only_supplied_story_ids() {
        let content = r#"{"requestId":"fixture","ranked":[{"storyId":"unknown","score":1.0},{"storyId":"known","score":0.8}]}"#;
        let body = json!({"choices":[{"message":{"content":content}}]}).to_string();
        let server = mock_server(vec![(200, body)]).await;
        let provider = OpenAiCompatibleRecommendationProvider::new(config(server.url.clone())).unwrap();
        let output = provider
            .rank(RankRequest {
                user_key: "opaque".into(),
                stream_slug: "home".into(),
                ranking_instruction: Some("Prefer implementation details".into()),
                candidates: vec![RankCandidate {
                    story_id: "known".into(),
                    title: "Known".into(),
                    summary: "Useful summary".into(),
                    topics: vec!["rust".into()],
                    publisher: None,
                    freshness: 1.0,
                    coverage: 1,
                    local_score: 0.5,
                }],
                result_count: 10,
                ui_mode: "modern".into(),
            })
            .await
            .unwrap();
        assert_eq!(output.ranked.len(), 1);
        assert_eq!(output.ranked[0].story_id, "known");
    }
}
