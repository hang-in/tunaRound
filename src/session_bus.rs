// Redis 기반 세션 버스 프리미티브(멀티세션 런타임 조율). memory.db 대체 아님, hot 미러.

use futures_util::StreamExt;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::AsyncCommands;
use tokio::sync::mpsc;

const DEFAULT_COMMAND_MAXLEN: usize = 10_000;
const DEFAULT_EVENT_MAXLEN: usize = 2_000;

/// 최소 버스 경계. REPL/런타임이 세션 commands/events를 흘려보낼 때 쓴다.
///
/// 구현은 in-process, Redis 기반, 또는 테스트 더블일 수 있다. payload는 호출자가
/// 소유한 JSON이며, 세션 로직은 이 trait 뒤로 격리돼 Redis 세부를 몰라도 된다.
pub trait SessionBus {
    fn submit_command_json(&self, session_id: &str, payload: &str);
    fn publish_event_json(&self, session_id: &str, payload: &str);
    /// hot_snapshot 키에 세션 전체 상태를 fire-and-forget으로 저장한다.
    fn snapshot_json(&self, session_id: &str, payload: &str);
}

/// Redis keys for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisSessionKeys {
    pub owner: String,
    pub commands: String,
    pub command_cursor: String,
    pub events: String,
    pub event_channel: String,
    pub presence: String,
    pub hot_snapshot: String,
}

impl RedisSessionKeys {
    pub fn new(session_id: &str) -> Self {
        let base = format!("session:{session_id}");
        Self {
            owner: format!("{base}:owner"),
            commands: format!("{base}:cmd"),
            command_cursor: format!("{base}:cmd:cursor"),
            events: format!("{base}:events"),
            event_channel: format!("{base}:events:pubsub"),
            presence: format!("{base}:presence"),
            hot_snapshot: format!("{base}:hot_snapshot"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisStreamMessage {
    pub id: String,
    pub payload: String,
}

/// Thin Redis client for session bus operations.
#[derive(Clone)]
pub struct RedisBus {
    client: redis::Client,
    command_maxlen: usize,
    event_maxlen: usize,
}

impl RedisBus {
    pub fn open(url: &str) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(url)?,
            command_maxlen: DEFAULT_COMMAND_MAXLEN,
            event_maxlen: DEFAULT_EVENT_MAXLEN,
        })
    }

    pub fn open_from_env() -> Option<Self> {
        let url = std::env::var("TUNAROUND_REDIS_URL").ok()?;
        if url.trim().is_empty() {
            return None;
        }
        match Self::open(url.trim()) {
            Ok(bus) => Some(bus),
            Err(e) => {
                eprintln!("[tunaRound] Redis bus disabled: {e}");
                None
            }
        }
    }

    pub fn with_limits(mut self, command_maxlen: usize, event_maxlen: usize) -> Self {
        self.command_maxlen = command_maxlen;
        self.event_maxlen = event_maxlen;
        self
    }

    pub fn keys(session_id: &str) -> RedisSessionKeys {
        RedisSessionKeys::new(session_id)
    }

    pub async fn submit_command(
        &self,
        session_id: &str,
        payload: &str,
    ) -> redis::RedisResult<String> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        redis::cmd("XADD")
            .arg(keys.commands)
            .arg("MAXLEN")
            .arg("~")
            .arg(self.command_maxlen)
            .arg("*")
            .arg("payload")
            .arg(payload)
            .query_async(&mut con)
            .await
    }

    pub async fn publish_event(
        &self,
        session_id: &str,
        payload: &str,
    ) -> redis::RedisResult<String> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        let stream_id: String = redis::cmd("XADD")
            .arg(&keys.events)
            .arg("MAXLEN")
            .arg("~")
            .arg(self.event_maxlen)
            .arg("*")
            .arg("payload")
            .arg(payload)
            .query_async(&mut con)
            .await?;
        let _: usize = con.publish(keys.event_channel, payload).await?;
        Ok(stream_id)
    }

    pub async fn read_commands(
        &self,
        session_id: &str,
        last_id: &str,
        block_ms: usize,
        count: usize,
    ) -> redis::RedisResult<Vec<RedisStreamMessage>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        let opts = StreamReadOptions::default().block(block_ms).count(count);
        let reply: Option<StreamReadReply> = con
            .xread_options(&[keys.commands], &[last_id], &opts)
            .await?;
        let mut messages = Vec::new();
        if let Some(reply) = reply {
            for key in reply.keys {
                for id in key.ids {
                    if let Some(payload) = id.get::<String>("payload") {
                        messages.push(RedisStreamMessage { id: id.id, payload });
                    }
                }
            }
        }
        Ok(messages)
    }

    pub async fn command_cursor(
        &self,
        session_id: &str,
    ) -> redis::RedisResult<Option<String>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.get(keys.command_cursor).await
    }

    pub async fn mark_command_consumed(
        &self,
        session_id: &str,
        stream_id: &str,
    ) -> redis::RedisResult<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.set(keys.command_cursor, stream_id).await
    }

    /// hot_snapshot 키에 세션 상태(JSON)를 저장한다.
    pub async fn set_snapshot(&self, session_id: &str, payload: &str) -> redis::RedisResult<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.set(keys.hot_snapshot, payload).await
    }

    /// hot_snapshot 키에서 세션 상태(JSON)를 읽는다.
    pub async fn get_snapshot(&self, session_id: &str) -> redis::RedisResult<Option<String>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.get(keys.hot_snapshot).await
    }

    pub async fn subscribe_events(
        &self,
        session_id: &str,
        frame_tx: tokio::sync::broadcast::Sender<String>,
    ) -> redis::RedisResult<()> {
        let keys = Self::keys(session_id);
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(keys.event_channel).await?;
        let mut messages = pubsub.on_message();
        while let Some(message) = messages.next().await {
            let payload: String = message.get_payload()?;
            let _ = frame_tx.send(payload);
        }
        Ok(())
    }

    pub async fn try_acquire_owner(
        &self,
        session_id: &str,
        worker_id: &str,
        ttl_secs: u64,
    ) -> redis::RedisResult<bool> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        let result: Option<String> = redis::cmd("SET")
            .arg(keys.owner)
            .arg(worker_id)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut con)
            .await?;
        Ok(result.is_some())
    }

    pub async fn refresh_owner(
        &self,
        session_id: &str,
        worker_id: &str,
        ttl_secs: u64,
    ) -> redis::RedisResult<bool> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        let current: Option<String> = con.get(&keys.owner).await?;
        if current.as_deref() != Some(worker_id) {
            return Ok(false);
        }
        let refreshed: bool = con.expire(keys.owner, ttl_secs as i64).await?;
        Ok(refreshed)
    }
}

enum RedisBusMessage {
    Command { session_id: String, payload: String },
    Event { session_id: String, payload: String },
    Snapshot { session_id: String, payload: String },
}

/// 유계 채널 용량. Redis writer가 밀리면 이 이상 쌓이지 않고 drop(무한 증가=OOM 방지).
/// 스냅샷/이벤트 발행률은 낮아 정상 상황에서 도달하지 않는다.
const BUS_CHANNEL_CAP: usize = 1024;

/// 동기 호출자(REPL 스레드)가 쓰는 fire-and-forget async writer. tokio 태스크가 실제 Redis 쓰기를 수행한다.
#[derive(Clone)]
pub struct RedisBusHandle {
    tx: mpsc::Sender<RedisBusMessage>,
}

impl RedisBusHandle {
    pub fn spawn(bus: RedisBus) -> Self {
        let (tx, mut rx) = mpsc::channel::<RedisBusMessage>(BUS_CHANNEL_CAP);
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let result = match msg {
                    RedisBusMessage::Command { session_id, payload } => {
                        bus.submit_command(&session_id, &payload).await.map(|_| ())
                    }
                    RedisBusMessage::Event { session_id, payload } => {
                        bus.publish_event(&session_id, &payload).await.map(|_| ())
                    }
                    RedisBusMessage::Snapshot { session_id, payload } => {
                        bus.set_snapshot(&session_id, &payload).await
                    }
                };
                if let Err(e) = result {
                    eprintln!("[tunaRound] redis bus write failed: {e}");
                }
            }
        });
        Self { tx }
    }

    pub fn spawn_from_env() -> Option<Self> {
        RedisBus::open_from_env().map(Self::spawn)
    }
}

impl RedisBusHandle {
    /// 유계 채널에 fire-and-forget 비블로킹 발행. 가득 차면(writer 지연) drop+경고, 채널이 닫혔으면 무시.
    /// REPL 스레드가 Redis 지연에 블로킹되지 않도록 try_send만 쓴다.
    fn enqueue(&self, msg: RedisBusMessage) {
        use mpsc::error::TrySendError;
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                eprintln!("[tunaRound] redis bus 채널 포화(용량 {BUS_CHANNEL_CAP}) - 메시지 drop");
            }
            Err(TrySendError::Closed(_)) => {} // writer 태스크 종료(정상 shutdown 등).
        }
    }
}

impl SessionBus for RedisBusHandle {
    fn submit_command_json(&self, session_id: &str, payload: &str) {
        self.enqueue(RedisBusMessage::Command {
            session_id: session_id.to_string(),
            payload: payload.to_string(),
        });
    }

    fn publish_event_json(&self, session_id: &str, payload: &str) {
        self.enqueue(RedisBusMessage::Event {
            session_id: session_id.to_string(),
            payload: payload.to_string(),
        });
    }

    fn snapshot_json(&self, session_id: &str, payload: &str) {
        self.enqueue(RedisBusMessage::Snapshot {
            session_id: session_id.to_string(),
            payload: payload.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keys_are_stable() {
        let keys = RedisSessionKeys::new("debate-alpha");
        assert_eq!(keys.owner, "session:debate-alpha:owner");
        assert_eq!(keys.commands, "session:debate-alpha:cmd");
        assert_eq!(keys.command_cursor, "session:debate-alpha:cmd:cursor");
        assert_eq!(keys.events, "session:debate-alpha:events");
        assert_eq!(keys.event_channel, "session:debate-alpha:events:pubsub");
        assert_eq!(keys.presence, "session:debate-alpha:presence");
        assert_eq!(keys.hot_snapshot, "session:debate-alpha:hot_snapshot");
    }

    #[test]
    fn empty_env_disables_handle() {
        // 단일 스레드 테스트라 안전한 unsafe 사용.
        unsafe {
            std::env::set_var("TUNAROUND_REDIS_URL", "");
        }
        assert!(RedisBusHandle::spawn_from_env().is_none());
        unsafe {
            std::env::remove_var("TUNAROUND_REDIS_URL");
        }
    }

    // 라이브 Redis 필요: TUNAROUND_REDIS_URL 설정 후 `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn command_roundtrip_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-roundtrip";
        let id = bus.submit_command(sid, "{\"cmd\":\"hi\"}").await.expect("submit");
        assert!(!id.is_empty());
        let msgs = bus.read_commands(sid, "0", 100, 10).await.expect("read");
        assert!(msgs.iter().any(|m| m.payload.contains("hi")));
    }

    #[tokio::test]
    #[ignore]
    async fn snapshot_set_get_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-snapshot";
        bus.set_snapshot(sid, "{\"messages\":[],\"head\":null}").await.expect("set");
        let got = bus.get_snapshot(sid).await.expect("get");
        assert_eq!(got.as_deref(), Some("{\"messages\":[],\"head\":null}"));
    }

    #[tokio::test]
    #[ignore]
    async fn owner_acquire_then_refresh_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-owner";
        let _ = bus.try_acquire_owner(sid, "w1", 30).await.expect("acquire");
        // 같은 worker는 refresh 성공
        assert!(bus.refresh_owner(sid, "w1", 30).await.expect("refresh"));
        // 다른 worker는 refresh 실패
        assert!(!bus.refresh_owner(sid, "w2", 30).await.expect("refresh2"));
    }
}
