//! Tests for the watch start/stop supersede logic: a start that is overtaken
//! by a newer start/stop request must abort without queueing uploads or
//! disturbing state owned by the newer request.
//!
//! These live in an integration test target (not `#[cfg(test)]` in lib.rs)
//! because on Windows the test binary needs the Common Controls v6 manifest
//! that build.rs injects via `rustc-link-arg-tests`, which cargo only applies
//! to integration test targets.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use uuid::Uuid;

use labric_sync_lib::{
    enqueue_scanned_files, scan_folder_contents, start_watching_impl, UploadConfig,
    UploadConfigState, UploadQueue, WatchGeneration, WatchStartOutcome, WatchedFolderState,
    WatcherState,
};

fn mock_app() -> tauri::App<MockRuntime> {
    mock_builder()
        // start_watching_impl persists the watched folder via the store
        // plugin; without it registered, app_handle.store() panics
        .plugin(tauri_plugin_store::Builder::default().build())
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

/// root/a.txt, root/b.tmp (matches the default "*.tmp" ignore pattern),
/// root/sub/c.txt
fn fixture_folder() -> TempDir {
    let dir = TempDir::new();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    fs::write(dir.path().join("b.tmp"), "b").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("c.txt"), "c").unwrap();
    dir
}

fn empty_queue() -> UploadQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}

fn default_config() -> UploadConfigState {
    Arc::new(Mutex::new(UploadConfig::default()))
}

/// Returns true (superseded) starting from the nth call
fn superseded_after(n: usize) -> impl Fn() -> bool {
    let calls = AtomicUsize::new(0);
    move || calls.fetch_add(1, Ordering::SeqCst) + 1 >= n
}

fn queued_paths(queue: &UploadQueue) -> Vec<String> {
    queue.lock().iter().map(|item| item.path.clone()).collect()
}

#[test]
fn scan_collects_files_recursively_without_queueing() {
    let app = mock_app();
    let dir = fixture_folder();

    let files = scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .expect("scan failed")
        .expect("scan unexpectedly superseded");

    let mut names: Vec<String> = files
        .iter()
        .map(|f| {
            Path::new(f)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    names.sort();
    // The scan collects everything, including files the upload config would
    // ignore -- filtering happens at enqueue time
    assert_eq!(names, ["a.txt", "b.tmp", "c.txt"]);
}

#[test]
fn scan_aborts_when_superseded_immediately() {
    let app = mock_app();
    let dir = fixture_folder();

    let result =
        scan_folder_contents(&dir.path_str(), app.handle(), &|| true).expect("scan failed");

    assert!(result.is_none(), "superseded scan must return None");
}

#[test]
fn scan_aborts_when_superseded_mid_walk() {
    let app = mock_app();
    let dir = fixture_folder();

    let result = scan_folder_contents(&dir.path_str(), app.handle(), &superseded_after(2))
        .expect("scan failed");

    assert!(result.is_none(), "scan superseded mid-walk must return None");
}

#[test]
fn enqueue_queues_files_and_respects_ignore_patterns() {
    let app = mock_app();
    let dir = fixture_folder();
    let queue = empty_queue();
    let config = default_config();

    let files = scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .unwrap()
        .unwrap();
    enqueue_scanned_files(files, &dir.path_str(), &queue, &config, app.handle(), &|| {
        false
    });

    let paths = queued_paths(&queue);
    assert_eq!(paths.len(), 2, "expected a.txt and c.txt, got {paths:?}");
    assert!(paths.iter().any(|p| p.ends_with("a.txt")));
    assert!(paths.iter().any(|p| p.ends_with("c.txt")));
    assert!(
        !paths.iter().any(|p| p.ends_with("b.tmp")),
        "*.tmp files must be filtered out at enqueue time"
    );
}

#[test]
fn enqueue_aborts_when_superseded_midway() {
    let app = mock_app();
    let dir = TempDir::new();
    for i in 0..5 {
        fs::write(dir.path().join(format!("f{i}.txt")), "x").unwrap();
    }
    let queue = empty_queue();
    let config = default_config();

    let files = scan_folder_contents(&dir.path_str(), app.handle(), &|| false)
        .unwrap()
        .unwrap();
    assert_eq!(files.len(), 5);

    // Superseded from the third check onward: two files get queued, the
    // remaining three must not
    enqueue_scanned_files(
        files,
        &dir.path_str(),
        &queue,
        &config,
        app.handle(),
        &superseded_after(3),
    );

    assert_eq!(queue.lock().len(), 2);
}

#[test]
fn start_watching_impl_starts_queues_and_records_state() {
    let app = mock_app();
    let dir = fixture_folder();
    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(None));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(0));
    let queue = empty_queue();
    let config = default_config();

    let expected = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let outcome = start_watching_impl(
        dir.path_str(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        expected,
    )
    .expect("start failed");

    assert!(matches!(outcome, WatchStartOutcome::Started));
    assert!(watcher_state.lock().is_some(), "watcher must be installed");
    assert_eq!(*watched_folder.lock(), Some(dir.path_str()));
    assert_eq!(
        queue.lock().len(),
        2,
        "a.txt and c.txt queued, b.tmp ignored"
    );
}

#[test]
fn stale_start_is_superseded_before_any_side_effects() {
    let app = mock_app();
    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(None));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(2));
    let queue = empty_queue();
    let config = default_config();

    // The folder deliberately does not exist: if the stale start ever reached
    // the scan, this would return Err instead of Superseded
    let outcome = start_watching_impl(
        "Z:\\does\\not\\exist".to_string(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        1, // stale: current generation is 2
    )
    .expect("stale start must not error");

    assert!(matches!(outcome, WatchStartOutcome::Superseded));
    assert!(watcher_state.lock().is_none());
    assert!(watched_folder.lock().is_none());
    assert!(
        queue.lock().is_empty(),
        "superseded start must queue nothing"
    );
}

#[test]
fn stale_start_does_not_disturb_current_watcher() {
    let app = mock_app();
    let current_dir = fixture_folder();
    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(Some(current_dir.path_str())));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(2));
    let queue = empty_queue();
    let config = default_config();

    // Install a watcher owned by the current (generation 2) watch
    let mut watcher = notify::recommended_watcher(|_| {}).unwrap();
    watcher
        .watch(current_dir.path(), RecursiveMode::Recursive)
        .unwrap();
    *watcher_state.lock() = Some(watcher);

    let outcome = start_watching_impl(
        "Z:\\does\\not\\exist".to_string(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        1, // stale request racing against the installed generation-2 watch
    )
    .expect("stale start must not error");

    assert!(matches!(outcome, WatchStartOutcome::Superseded));
    assert!(
        watcher_state.lock().is_some(),
        "stale start must not tear down the newer watch's watcher"
    );
    assert_eq!(*watched_folder.lock(), Some(current_dir.path_str()));
    assert!(queue.lock().is_empty());
}

#[test]
fn start_superseded_by_stop_leaves_queue_empty_and_no_watcher() {
    let app = mock_app();
    let dir = fixture_folder();
    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(None));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(0));
    let queue = empty_queue();
    let config = default_config();

    let expected = generation.fetch_add(1, Ordering::SeqCst) + 1;
    // Simulate stop_watching winning the race: its generation bump lands
    // before this start makes any progress
    generation.fetch_add(1, Ordering::SeqCst);

    let outcome = start_watching_impl(
        dir.path_str(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        expected,
    )
    .expect("superseded start must not error");

    assert!(matches!(outcome, WatchStartOutcome::Superseded));
    assert!(watcher_state.lock().is_none());
    assert!(watched_folder.lock().is_none());
    assert!(
        queue.lock().is_empty(),
        "a start superseded by stop must not queue any uploads"
    );
}

/// End-to-end race: a stop arrives while the start's initial scan is running.
/// The scan observes the generation bump mid-walk, aborts, and leaves no
/// uploads or watcher behind.
#[test]
fn stop_arriving_mid_scan_aborts_start_without_queueing() {
    let app = mock_app();
    let dir = fixture_folder();
    let watcher_state: WatcherState = Arc::new(Mutex::new(None));
    let watched_folder: WatchedFolderState = Arc::new(Mutex::new(None));
    let generation: WatchGeneration = Arc::new(AtomicU64::new(0));
    let queue = empty_queue();
    let config = default_config();

    let expected = generation.fetch_add(1, Ordering::SeqCst) + 1;

    // Bump the generation from another thread shortly after the start begins,
    // like stop_watching would. Whether the bump lands before, during, or
    // after the scan, the outcome must be consistent: either the start
    // committed fully before the stop (watcher installed, files queued) or it
    // was superseded (nothing queued, no watcher).
    let generation_for_stop = generation.clone();
    let stopper = std::thread::spawn(move || {
        generation_for_stop.fetch_add(1, Ordering::SeqCst);
    });

    let outcome = start_watching_impl(
        dir.path_str(),
        app.handle(),
        &watcher_state,
        &queue,
        &config,
        &watched_folder,
        &generation,
        expected,
    )
    .expect("start must not error");
    stopper.join().unwrap();

    match outcome {
        WatchStartOutcome::Superseded => {
            assert!(
                queue.lock().is_empty(),
                "superseded start must not queue uploads"
            );
            assert!(watcher_state.lock().is_none());
            assert!(watched_folder.lock().is_none());
        }
        WatchStartOutcome::Started => {
            // The start won the race and committed before the bump was
            // observable at any check point; committed state must be complete
            assert!(watcher_state.lock().is_some());
            assert_eq!(*watched_folder.lock(), Some(dir.path_str()));
        }
    }
}
