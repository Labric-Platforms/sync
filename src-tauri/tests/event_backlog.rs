//! Tests for the event backlog that lets the frontend recover `file_change`
//! and `file_upload_status` events emitted before the webview registered its
//! listeners (Tauri drops emits that have no listener, so without the backlog
//! the initial scan of a watch resumed at startup was invisible in the UI).
//!
//! Lives in an integration test target for the same reason as
//! watch_supersede.rs: on Windows the test binary needs the Common Controls
//! v6 manifest that build.rs injects via `rustc-link-arg-tests`.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;
use uuid::Uuid;

use labric_sync_lib::{
    emit_file_upload_status, scan_folder_contents, start_watching_impl, FileChangeEvent,
    FileChangeLog, UploadConfig, UploadConfigState, UploadQueue, UploadStatusLog, WatchGeneration,
    WatchStartOutcome, WatchedFolderState, WatcherState, MAX_FILE_CHANGE_LOG,
    MAX_UPLOAD_STATUS_LOG,
};

/// Mock app with both backlogs managed, the way run() manages them
fn mock_app() -> tauri::App<MockRuntime> {
    let file_change_log: FileChangeLog = Arc::new(Mutex::new(VecDeque::new()));
    let upload_status_log: UploadStatusLog = Arc::new(Mutex::new(Vec::new()));
    mock_builder()
        // start_watching_impl persists the watched folder via the store plugin
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(file_change_log)
        .manage(upload_status_log)
        .build(mock_context(noop_assets()))
        .expect("failed to build mock app")
}

/// Temp directory that cleans up on drop, without adding a tempfile dep
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("labric-sync-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn path_str(&self) -> String {
        self.0.to_string_lossy().to_string()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// root/a.txt, root/sub/c.txt
fn fixture_folder() -> TempDir {
    let dir = TempDir::new();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("c.txt"), "c").unwrap();
    dir
}

fn logged_names(app: &tauri::App<MockRuntime>) -> Vec<String> {
    let log = app.state::<FileChangeLog>();
    let mut names: Vec<String> = log
        .lock()
        .iter()
        .map(|e| {
            Path::new(&e.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn scan_records_every_entry_in_the_backlog() {
    let app = mock_app();
    let dir = fixture_folder();

    scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .expect("scan failed")
        .expect("scan unexpectedly superseded");

    // Directories are logged too -- the UI shows them in the change list
    assert_eq!(logged_names(&app), ["a.txt", "c.txt", "sub"]);
    let log = app.state::<FileChangeLog>();
    assert!(log.lock().iter().all(|e| e.event_type == "initial"));
}

#[test]
fn backlog_keeps_one_entry_per_path_across_rescans() {
    let app = mock_app();
    let dir = fixture_folder();

    for _ in 0..2 {
        scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
            .expect("scan failed")
            .expect("scan unexpectedly superseded");
    }

    assert_eq!(logged_names(&app), ["a.txt", "c.txt", "sub"]);
}

#[test]
fn backlog_is_capped_and_evicts_oldest() {
    let app = mock_app();
    let dir = TempDir::new();
    // File names sort by creation order so the eviction order is predictable
    for i in 0..MAX_FILE_CHANGE_LOG + 10 {
        fs::write(dir.path().join(format!("f{i:04}.txt")), "x").unwrap();
    }

    scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .expect("scan failed")
        .expect("scan unexpectedly superseded");

    let log = app.state::<FileChangeLog>();
    let log = log.lock();
    assert_eq!(log.len(), MAX_FILE_CHANGE_LOG);
    // The oldest entries were evicted from the front
    assert!(!log.iter().any(|e| e.path.ends_with("f0000.txt")));
    assert!(log.iter().any(|e| e.path.ends_with(&format!(
        "f{:04}.txt",
        MAX_FILE_CHANGE_LOG + 9
    ))));
}

#[test]
fn start_watching_clears_backlog_from_previous_folder() {
    let app = mock_app();
    let dir = fixture_folder();

    {
        let log = app.state::<FileChangeLog>();
        log.lock().push_back(FileChangeEvent {
            path: "stale-from-old-folder".to_string(),
            event_type: "created".to_string(),
            timestamp: 0,
        });
    }

    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(None));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(1));
    let queue: UploadQueue = Arc::new(Mutex::new(VecDeque::new()));
    let config: UploadConfigState = Arc::new(Mutex::new(UploadConfig::default()));

    let outcome = start_watching_impl(
        dir.path_str(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        1,
    )
    .expect("start failed");

    assert!(matches!(outcome, WatchStartOutcome::Started));
    let names = logged_names(&app);
    assert!(!names.contains(&"stale-from-old-folder".to_string()));
    assert_eq!(names, ["a.txt", "c.txt", "sub"]);
}

#[test]
fn status_log_keeps_latest_status_per_path() {
    let app = mock_app();

    emit_file_upload_status("a.txt", "queued", None, app.handle());
    emit_file_upload_status("b.txt", "queued", None, app.handle());
    emit_file_upload_status("a.txt", "uploaded", None, app.handle());

    let log = app.state::<UploadStatusLog>();
    let log = log.lock();
    assert_eq!(log.len(), 2);
    // Updating a path keeps its original position (mirrors the frontend Map)
    assert_eq!(log[0].relative_path, "a.txt");
    assert_eq!(log[0].status, "uploaded");
    assert_eq!(log[1].relative_path, "b.txt");
    assert_eq!(log[1].status, "queued");
}

#[test]
fn status_log_is_capped_and_evicts_oldest() {
    let app = mock_app();

    for i in 0..MAX_UPLOAD_STATUS_LOG + 5 {
        emit_file_upload_status(&format!("f{i}.txt"), "queued", None, app.handle());
    }

    let log = app.state::<UploadStatusLog>();
    let log = log.lock();
    assert_eq!(log.len(), MAX_UPLOAD_STATUS_LOG);
    assert_eq!(log[0].relative_path, "f5.txt");
    assert_eq!(
        log[log.len() - 1].relative_path,
        format!("f{}.txt", MAX_UPLOAD_STATUS_LOG + 4)
    );
}

#[test]
fn recording_is_skipped_gracefully_when_logs_are_not_managed() {
    // A mock app without the managed logs (like the watch_supersede tests)
    let app = mock_builder()
        .plugin(tauri_plugin_store::Builder::default().build())
        .build(mock_context(noop_assets()))
        .expect("failed to build mock app");
    let dir = fixture_folder();

    // Must not panic, and the scan must still succeed
    let files = scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .expect("scan failed")
        .expect("scan unexpectedly superseded");
    assert_eq!(files.len(), 2);
    emit_file_upload_status("a.txt", "queued", None, app.handle());
}
