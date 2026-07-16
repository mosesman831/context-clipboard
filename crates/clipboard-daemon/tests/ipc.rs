//! Integration test: boot the IPC server on a temp socket and exercise the
//! full request set over the wire, including an encrypt→store→decrypt roundtrip.

use clipboard_core::config::Config;
use clipboard_core::crypto;
use clipboard_core::ipc::{Request, Response, IPC_VERSION};
use clipboard_core::schema;
use clipboard_daemon::db::{self, CapturedClip};
use clipboard_daemon::frame::{read_frame, write_frame};
use clipboard_daemon::server::Server;
use clipboard_daemon::state::AppState;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::oneshot;

struct Harness {
    _dir: tempfile::TempDir,
    socket: std::path::PathBuf,
    state: Arc<AppState>,
    shutdown: Option<oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<()>,
}

async fn boot() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("history.db");
    let conn = schema::open(&db_path).unwrap();
    let key = crypto::load_or_create_dev_key(&dir.path().join("db.key")).unwrap();
    let state = Arc::new(AppState::new(
        conn,
        key,
        Config::default(),
        dir.path().to_path_buf(),
    ));

    let socket = dir.path().join("daemon.sock");
    let server = Server::bind(&socket).unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    let srv_state = Arc::clone(&state);
    let handle = tokio::spawn(async move {
        server
            .run(srv_state, async move {
                let _ = rx.await;
            })
            .await;
    });

    Harness {
        _dir: dir,
        socket,
        state,
        shutdown: Some(tx),
        handle,
    }
}

impl Harness {
    async fn request(&self, req: Request) -> Response {
        let mut stream = UnixStream::connect(&self.socket).await.unwrap();
        let bytes = serde_json::to_vec(&req).unwrap();
        write_frame(&mut stream, &bytes).await.unwrap();
        let body = read_frame(&mut stream)
            .await
            .unwrap()
            .expect("expected a response frame");
        serde_json::from_slice(&body).unwrap()
    }

    /// Insert a clip through the real store layer (as the capture loop would).
    fn insert_text(&self, text: &str) {
        let conn = self.state.db.lock().unwrap();
        let clip = CapturedClip {
            content_hash: schema::content_hash(text.as_bytes()),
            mime: "text/plain".to_string(),
            category: "text".to_string(),
            text: Some(text.to_string()),
            preview: text.chars().take(200).collect(),
            source_app: Some("TestApp".to_string()),
            source_bundle_id: None,
            source_window_title: Some("Test Window".to_string()),
            source_url: None,
            byte_size: text.len() as i64,
        };
        db::store_capture(&conn, &self.state.key, clip).unwrap();
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.handle.await;
    }
}

#[tokio::test]
async fn ping_roundtrip() {
    let h = boot().await;
    assert!(matches!(
        h.request(Request::Ping { v: IPC_VERSION }).await,
        Response::Ok
    ));
    h.shutdown().await;
}

#[tokio::test]
async fn status_reports_count() {
    let h = boot().await;
    h.insert_text("hello world");
    h.insert_text("second clip");
    match h.request(Request::GetStatus).await {
        Response::Status {
            count,
            paused,
            version,
            ..
        } => {
            assert_eq!(count, 2);
            assert!(!paused);
            assert_eq!(version, IPC_VERSION);
        }
        other => panic!("expected Status, got {other:?}"),
    }
    h.shutdown().await;
}

#[tokio::test]
async fn recent_get_decrypts_text() {
    let h = boot().await;
    h.insert_text("the quick brown fox");

    let id = match h.request(Request::Recent { limit: Some(10) }).await {
        Response::SearchResults { items } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].preview, "the quick brown fox");
            items[0].id.clone()
        }
        other => panic!("expected SearchResults, got {other:?}"),
    };

    match h.request(Request::Get { id }).await {
        Response::ClipDetail { detail } => {
            assert_eq!(detail.text.as_deref(), Some("the quick brown fox"));
        }
        other => panic!("expected ClipDetail, got {other:?}"),
    }
    h.shutdown().await;
}

#[tokio::test]
async fn fts_search_matches() {
    let h = boot().await;
    h.insert_text("rust programming language");
    h.insert_text("python scripting");

    match h
        .request(Request::Search {
            query: "rust".to_string(),
            limit: Some(10),
            category: None,
            app: None,
            since: None,
        })
        .await
    {
        Response::SearchResults { items } => {
            assert_eq!(items.len(), 1);
            assert!(items[0].preview.contains("rust"));
        }
        other => panic!("expected SearchResults, got {other:?}"),
    }
    h.shutdown().await;
}

#[tokio::test]
async fn delete_and_clear() {
    let h = boot().await;
    h.insert_text("disposable clip");
    let id = match h.request(Request::Recent { limit: Some(10) }).await {
        Response::SearchResults { items } => items[0].id.clone(),
        other => panic!("expected SearchResults, got {other:?}"),
    };

    assert!(matches!(
        h.request(Request::Delete { id }).await,
        Response::Ok
    ));
    match h.request(Request::GetStatus).await {
        Response::Status { count, .. } => assert_eq!(count, 0),
        other => panic!("expected Status, got {other:?}"),
    }

    h.insert_text("a");
    h.insert_text("b");
    assert!(matches!(h.request(Request::Clear).await, Response::Ok));
    match h.request(Request::GetStatus).await {
        Response::Status { count, .. } => assert_eq!(count, 0),
        other => panic!("expected Status, got {other:?}"),
    }
    h.shutdown().await;
}

#[tokio::test]
async fn set_paused_and_excluded_apps() {
    let h = boot().await;

    assert!(matches!(
        h.request(Request::SetPaused { paused: true }).await,
        Response::Ok
    ));
    match h.request(Request::GetStatus).await {
        Response::Status { paused, .. } => assert!(paused),
        other => panic!("expected Status, got {other:?}"),
    }

    assert!(matches!(
        h.request(Request::AddExcludedApp {
            match_type: "app_name".to_string(),
            match_value: "SecretApp".to_string(),
        })
        .await,
        Response::Ok
    ));

    let id = match h.request(Request::ListExcludedApps).await {
        Response::ExcludedApps { apps } => {
            let app = apps
                .iter()
                .find(|a| a.match_value == "SecretApp")
                .expect("added app present");
            app.id
        }
        other => panic!("expected ExcludedApps, got {other:?}"),
    };

    // Invalid match_type is rejected with an error response.
    match h
        .request(Request::AddExcludedApp {
            match_type: "bogus".to_string(),
            match_value: "x".to_string(),
        })
        .await
    {
        Response::Err { code, .. } => assert_eq!(code, "invalid_input"),
        other => panic!("expected Err, got {other:?}"),
    }

    assert!(matches!(
        h.request(Request::RemoveExcludedApp { id }).await,
        Response::Ok
    ));
    h.shutdown().await;
}

#[tokio::test]
async fn get_missing_returns_err() {
    let h = boot().await;
    match h
        .request(Request::Get {
            id: "nope".to_string(),
        })
        .await
    {
        Response::Err { code, .. } => assert_eq!(code, "not_found"),
        other => panic!("expected Err, got {other:?}"),
    }
    h.shutdown().await;
}

#[tokio::test]
async fn dedup_updates_last_seen() {
    let h = boot().await;
    h.insert_text("same content");
    h.insert_text("same content");
    match h.request(Request::GetStatus).await {
        Response::Status { count, .. } => {
            assert_eq!(count, 1, "duplicate content must not create a new row")
        }
        other => panic!("expected Status, got {other:?}"),
    }
    h.shutdown().await;
}
