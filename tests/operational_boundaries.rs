// 운영 경계(#138 C-②) 통합 테스트: 네트워크 단절 재시도(응답 유실 복구)와 SQLite 락 경합.
// 기존 커버리지(mcp_client.rs의 재시도 분류 순수함수·404 재연결, runner/exec.rs의 stdin/pipe,
// store/sqlite의 애플리케이션 레벨 직렬화)와 겹치지 않는 두 진짜 갭만 다룬다:
//   1) call_tool의 실제 재시도 루프(5xx 등 일시적 응답 유실)가 real HTTP 위에서 끝까지 동작해
//      회복(성공)하는지, 그리고 예산 소진 시 무한루프 없이 포기하는지.
//   2) 같은 SQLite 파일에 대한 독립된(Rust Mutex로 묶이지 않은) 두 실제 연결이 동시에 쓸 때
//      busy_timeout이 SQLITE_BUSY를 조용히 에러로 흘리지 않고 순번을 넘겨 경합을 흡수하는지.
#![cfg(all(feature = "worker", feature = "sqlite"))]

// ---------------------------------------------------------------------------
// 1) 네트워크 단절 재시도 / 응답 유실: McpHttpClient::call_tool 실제 재시도 루프
// ---------------------------------------------------------------------------
mod flaky_retry {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tunaround::mcp_client::McpHttpClient;

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// 소켓에서 HTTP 요청 한 건을 읽는다(헤더 + Content-Length만큼의 바디). 원시 TCP라
    /// watch_results.rs의 spawn_raw_http_server와 같은 관례를 요청 파싱 방향으로 확장한 것.
    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            match socket.read(&mut chunk).await {
                Ok(0) => break None,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                        break Some(pos);
                    }
                }
                Err(_) => break None,
            }
        };
        let Some(pos) = header_end else {
            return Vec::new();
        };
        let header_str = String::from_utf8_lossy(&buf[..pos]).to_string();
        let mut body = buf[pos + 4..].to_vec();
        let content_length = header_str
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim().eq_ignore_ascii_case("content-length"))
                    .then(|| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        while body.len() < content_length {
            match socket.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => body.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        body
    }

    /// 상태코드 + 헤더 + 바디로 HTTP 응답을 쓰고 연결을 닫는다(매 요청 = 새 TCP 연결 관례,
    /// Connection: close로 reqwest가 죽은 연결을 재사용하지 않게 한다).
    async fn write_http_response(
        socket: &mut tokio::net::TcpStream,
        status: u16,
        reason: &str,
        extra_headers: &[(&str, &str)],
        body: &[u8],
    ) {
        let mut resp = format!("HTTP/1.1 {status} {reason}\r\n");
        for (k, v) in extra_headers {
            resp.push_str(&format!("{k}: {v}\r\n"));
        }
        resp.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ));
        let _ = socket.write_all(resp.as_bytes()).await;
        let _ = socket.write_all(body).await;
        let _ = socket.shutdown().await;
    }

    /// 요청 1건을 처리한다: initialize/notifications/initialized는 항상 성공시켜 핸드셰이크를
    /// 통과시키고, tools/call만 처음 `fail_count`번은 503(응답 유실 시뮬레이션), 그 이후는
    /// 요청 id를 그대로 반영한 정상 JSON-RPC 결과로 응답한다.
    async fn handle_conn(
        mut socket: tokio::net::TcpStream,
        tool_call_count: Arc<AtomicUsize>,
        fail_count: usize,
    ) {
        let body = read_http_request(&mut socket).await;
        let json: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({}));
        let method = json.get("method").and_then(|m| m.as_str()).unwrap_or("");
        match method {
            "initialize" => {
                let body = serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}}).to_string();
                write_http_response(
                    &mut socket,
                    200,
                    "OK",
                    &[
                        ("Content-Type", "application/json"),
                        ("mcp-session-id", "flaky-session"),
                    ],
                    body.as_bytes(),
                )
                .await;
            }
            "notifications/initialized" => {
                write_http_response(
                    &mut socket,
                    200,
                    "OK",
                    &[("Content-Type", "application/json")],
                    b"{}",
                )
                .await;
            }
            "tools/call" => {
                let attempt = tool_call_count.fetch_add(1, Ordering::SeqCst);
                if attempt < fail_count {
                    write_http_response(&mut socket, 503, "Service Unavailable", &[], b"").await;
                } else {
                    let id = json.get("id").cloned().unwrap_or(serde_json::json!(0));
                    let ok_body = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {"content": [{"type": "text", "text": "ok-after-retry"}]}
                    })
                    .to_string();
                    write_http_response(
                        &mut socket,
                        200,
                        "OK",
                        &[("Content-Type", "application/json")],
                        ok_body.as_bytes(),
                    )
                    .await;
                }
            }
            _ => {
                write_http_response(&mut socket, 404, "Not Found", &[], b"").await;
            }
        }
    }

    /// tools/call 시도를 처음 `fail_count`번 503으로 응답하는 가짜 MCP 서버를 띄운다.
    /// 반환된 카운터로 테스트가 실제 시도 횟수를 관찰한다.
    async fn spawn_flaky_mcp_server(fail_count: usize) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        let tool_call_count = Arc::new(AtomicUsize::new(0));
        let counter = tool_call_count.clone();
        tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(handle_conn(socket, counter.clone(), fail_count));
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (format!("http://127.0.0.1:{port}/mcp"), tool_call_count)
    }

    /// 갭 1: 서버가 처음 두 번(응답 유실 시뮬레이션)을 503으로 답해도, McpHttpClient::call_tool의
    /// 실제 재시도 루프(RETRY_BACKOFF_MS=[200,500]ms)가 세 번째 시도에서 성공을 건져 올린다.
    /// 기존 테스트(is_transient_error_covers_5xx_but_not_4xx)는 "5xx는 재시도 대상"이라는 분류만
    /// 순수함수로 확인했을 뿐, 그 분류가 실제 HTTP 왕복에서 재시도를 일으켜 응답 유실을 넘어서는지는
    /// 검증한 적이 없었다.
    #[tokio::test]
    async fn call_tool_retries_transient_failure_and_recovers() {
        let (url, counter) = spawn_flaky_mcp_server(2).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let started = std::time::Instant::now();
        let text = client
            .poll_tasks("nobody")
            .await
            .expect("재시도 끝에 성공해야 함(일시적 응답 유실을 넘어서야 함)");

        assert!(
            text.contains("ok-after-retry"),
            "재시도 성공 응답 불일치: {text}"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            3,
            "정확히 3번(초기 1 + 재시도 2) 시도해야 함"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(650),
            "백오프(200ms+500ms)를 실제로 기다린 뒤 성공해야 함(즉시 성공이면 재시도 루프 우회 의심)"
        );
    }

    /// 갭 1의 반대편: 응답 유실이 영구화(서버가 계속 503)되면 무한 재시도가 아니라 예산
    /// (초기 1 + 재시도 2 = 3회) 소진 후 Err로 끝나야 한다 - 호출부(워커 poll 루프 등)가 실패를
    /// 알아채고 다음 조치를 취할 수 있어야 하며, 조용히 걸려 있으면 안 된다.
    #[tokio::test]
    async fn call_tool_gives_up_after_exhausting_retries() {
        let (url, counter) = spawn_flaky_mcp_server(usize::MAX).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let err = client
            .poll_tasks("nobody")
            .await
            .expect_err("영구적 503은 결국 Err로 끝나야 함(무한 대기 아님)");
        assert!(
            err.contains("503") || err.contains("tools/call"),
            "에러 메시지에 실패 원인이 드러나야 함: {err}"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            3,
            "정확히 3회 시도 후 포기해야 함(그 이상 재시도하면 안 됨)"
        );
    }
}

// ---------------------------------------------------------------------------
// 2) SQLite 락 경합: Rust Mutex로 묶이지 않은 독립 파일 연결 두 개의 동시 쓰기
// ---------------------------------------------------------------------------
mod sqlite_lock_contention {
    use tunaround::store::sqlite::SqliteStore;

    /// 기존 mcp/indexing.rs의 concurrent_index_same_task_no_duplicate_turns는 두 스레드가 항상
    /// 같은 a2a_store `Arc<Mutex<..>>`의 락을 먼저 잡은 뒤에만 SQLite에 접근하므로, 두 스레드가
    /// SQLite 파일을 향해 실제로 동시에(Rust 레벨 상호배제 없이) 쓰는 상황은 한 번도 재현되지
    /// 않았다. 여기서는 SqliteStore::open을 같은 파일에 두 번 호출해(각자 별개 rusqlite
    /// Connection, 공유 Mutex 없음) 진짜 SQLite 레벨 쓰기 경합을 만든다. open()이 세팅하는
    /// `busy_timeout=5000`이 없거나 0이면 경합 중 일부 execute()가 즉시 "database is locked"로
    /// 실패한다 - 이 테스트는 그 pragma가 실제로 경합을 흡수해 에러 없이 넘어가는지 고정한다.
    #[test]
    fn concurrent_file_connections_survive_write_contention_via_busy_timeout() {
        let path = std::env::temp_dir().join(format!(
            "tuna_lock_contend_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos()
        ));
        let path_str = path.to_str().unwrap().to_string();
        let sidecars = |p: &str| -> Vec<String> {
            ["", "-wal", "-shm"]
                .iter()
                .map(|s| format!("{p}{s}"))
                .collect()
        };
        for f in sidecars(&path_str) {
            let _ = std::fs::remove_file(f);
        }

        let store_a = SqliteStore::open(&path_str).expect("연결 A open 성공해야 함");
        let store_b = SqliteStore::open(&path_str).expect("연결 B open 성공해야 함");

        const WRITES_PER_THREAD: usize = 150;
        // 두 스레드가 동시에 쓰기를 시작하도록 맞춰 경합 창을 최대화한다(그 뒤로도 각자 빠르게
        // 연달아 써서 경합 기회가 루프 내내 이어진다).
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let barrier_a = barrier.clone();
        let handle_a = std::thread::spawn(move || {
            barrier_a.wait();
            for i in 0..WRITES_PER_THREAD {
                store_a
                    .set_config(&format!("boundary_a_{i}"), "v")
                    .expect("연결 A 쓰기가 락 경합으로 실패하면 안 됨(busy_timeout이 흡수해야 함)");
            }
            store_a
        });
        let barrier_b = barrier.clone();
        let handle_b = std::thread::spawn(move || {
            barrier_b.wait();
            for i in 0..WRITES_PER_THREAD {
                store_b
                    .set_config(&format!("boundary_b_{i}"), "v")
                    .expect("연결 B 쓰기가 락 경합으로 실패하면 안 됨(busy_timeout이 흡수해야 함)");
            }
            store_b
        });

        let store_a = handle_a.join().expect("스레드 A 패닉 없어야 함");
        let store_b = handle_b.join().expect("스레드 B 패닉 없어야 함");

        // 경합 중 유실 없이 양쪽 다 실제로 반영됐는지 대표 키로 확인(첫/끝 각 스레드).
        assert_eq!(
            store_a.get_config("boundary_a_0").unwrap().as_deref(),
            Some("v")
        );
        assert_eq!(
            store_a
                .get_config(&format!("boundary_a_{}", WRITES_PER_THREAD - 1))
                .unwrap()
                .as_deref(),
            Some("v")
        );
        assert_eq!(
            store_b.get_config("boundary_b_0").unwrap().as_deref(),
            Some("v")
        );
        assert_eq!(
            store_b
                .get_config(&format!("boundary_b_{}", WRITES_PER_THREAD - 1))
                .unwrap()
                .as_deref(),
            Some("v")
        );

        drop(store_a);
        drop(store_b);
        for f in sidecars(&path_str) {
            let _ = std::fs::remove_file(f);
        }
    }
}
