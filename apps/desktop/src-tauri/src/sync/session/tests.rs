use super::*;
use crate::sync::saved::InMemory;

fn state() -> SyncState {
    let path = std::env::temp_dir().join(format!("sync-{}.db", uuid::Uuid::new_v4()));
    SyncState::new(Arc::new(InMemory::default()), Ok(path))
}

#[tokio::test]
async fn nothing_kept_is_signed_out() {
    assert_eq!(state().status().await.unwrap(), Status::default());
}

/// Signed out, the loop's thirty-second check costs nothing: no request, a quiet error.
#[tokio::test]
async fn signed_out_asks_nobody() {
    let state = state();
    let pulled = state.pull_page("c").await.err().map(|error| error.code);
    let pushed = state.push("c", vec![]).await.err().map(|error| error.code);
    assert_eq!(pulled, Some("error.syncNotSignedIn"));
    assert_eq!(pushed, Some("error.syncNotSignedIn"));
}

/// A token nobody handed out — or one from before a sign-in — records nothing.
#[tokio::test]
async fn a_token_nobody_handed_out_records_nothing() {
    let error = state()
        .commit_pull("c", "nope", Vec::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "error.syncPageStale");
}

#[tokio::test]
async fn a_code_with_no_registration_waiting_is_refused() {
    let error = state().verify("AAAA-AAAA", "desktop").await.unwrap_err();
    assert_eq!(error.code, "error.syncNothingToVerify");
}

#[tokio::test]
async fn keeping_with_no_ticket_held_is_refused() {
    let error = state()
        .reset_keeping("AAAA", "new".into(), "desktop")
        .await
        .unwrap_err();
    assert_eq!(error.code, "error.syncNothingToReset");
}

#[tokio::test]
async fn starting_over_with_nothing_prepared_is_refused() {
    let error = state()
        .start_over("AAAA-AAAA", "desktop")
        .await
        .unwrap_err();
    assert_eq!(error.code, "error.syncNothingToReset");
}

/// Signed out, there is no server to ask about, and nobody is asked.
#[tokio::test]
async fn signed_out_has_no_closing_date() {
    assert_eq!(state().closing_on().await.unwrap(), None);
}

/// Counts what is kept, so a test can say a refresh did not happen: a refresh is what keeps.
#[derive(Default)]
struct Counting {
    inner: InMemory,
    kept: std::sync::atomic::AtomicUsize,
}

impl Keeping for Counting {
    fn load(&self) -> Result<Option<Saved>, AppError> {
        self.inner.load()
    }
    fn keep(&self, saved: &Saved) -> Result<(), AppError> {
        self.kept.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.keep(saved)
    }
    fn forget(&self) -> Result<(), AppError> {
        self.inner.forget()
    }
}

/// A server that answers every request with the same capabilities, and records each request's
/// first line.
async fn capabilities_server(closing_on: i64) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = asked.clone();
    let body = serde_json::json!({
        "protocolVersions": ["1"],
        "maxRecordBytes": 1,
        "maxBatchOperations": 1,
        "maxBatchBytes": 1,
        "maxPageRecords": 1,
        "accountQuotaBytes": 1,
        "tombstoneRetentionDays": 1,
        "closingOn": closing_on,
        "features": [],
    })
    .to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut head = vec![0u8; 4096];
            let read = socket.read(&mut head).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&head[..read]).into_owned();
            let line = head.lines().next().unwrap_or_default();
            let line = line.rsplit_once(' ').map_or(line, |(line, _)| line);
            recorded.lock().unwrap().push(line.to_owned());
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                     connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(answer.as_bytes()).await;
        }
    });
    (base, asked)
}

/// **Reading the closing date at launch keeps nothing.** It used to open a session, which
/// refreshes, which rotates the refresh token and writes the vault on every launch of a
/// signed-in machine — with every sync row off. The date comes from `/v1/capabilities`, which
/// needs no session.
#[tokio::test]
async fn the_closing_date_at_launch_refreshes_nothing() {
    let (base, asked) = capabilities_server(1_900_000_000).await;
    let keeping = Arc::new(Counting::default());
    let saved = Saved::from_json(&format!(
        r#"{{"server":"{base}","access":null,"email":"e","deviceId":"d","refreshToken":"r","masterKey":"m"}}"#
    ))
    .unwrap();
    keeping.inner.keep(&saved).unwrap();
    let path = std::env::temp_dir().join(format!("sync-{}.db", uuid::Uuid::new_v4()));
    let state = SyncState::new(keeping.clone(), Ok(path));

    assert_eq!(state.closing_on().await.unwrap(), Some(1_900_000_000));
    assert_eq!(
        keeping.kept.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing was kept, so no refresh token was spent"
    );
    assert_eq!(*asked.lock().unwrap(), ["GET /v1/capabilities"]);
}
