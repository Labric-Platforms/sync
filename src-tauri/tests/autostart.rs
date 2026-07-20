//! Exercises the autostart plugin end-to-end: enabling registers the test
//! binary with the OS launcher (HKCU Run key on Windows) and disabling
//! removes it again, leaving the system clean.

use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[test]
fn autostart_enable_disable_roundtrip() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app with autostart plugin");

    let autolaunch = app.autolaunch();

    autolaunch.enable().expect("enable() failed");
    assert!(
        autolaunch.is_enabled().expect("is_enabled() failed"),
        "autostart should report enabled after enable()"
    );

    // On Windows, independently confirm the OS-level registration exists:
    // the HKCU Run key must point at this test binary while enabled.
    #[cfg(windows)]
    {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            ])
            .output()
            .expect("failed to run reg query");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let exe = std::env::current_exe().expect("current_exe failed");
        let exe_name = exe.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            stdout.contains(&exe_name),
            "Run key should contain an entry pointing at {exe_name}, got:\n{stdout}"
        );
    }

    autolaunch.disable().expect("disable() failed");
    assert!(
        !autolaunch.is_enabled().expect("is_enabled() failed"),
        "autostart should report disabled after disable()"
    );
}
