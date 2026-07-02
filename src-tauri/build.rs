fn main() {
    println!("cargo:rerun-if-changed=../.env.local");
    println!("cargo:rerun-if-changed=../.env.production");
    println!("cargo:rerun-if-env-changed=VITE_SERVER_URL");

    dotenvy::from_path("../.env.local")
        .or_else(|_| dotenvy::from_path("../.env.production"))
        .ok();

    if let Ok(url) = std::env::var("VITE_SERVER_URL") {
        println!("cargo:rustc-env=VITE_SERVER_URL={url}");
    }

    // Test binaries don't get the Windows manifest tauri_build embeds into the
    // app, so they load Common Controls v5 and crash with
    // STATUS_ENTRYPOINT_NOT_FOUND (https://github.com/orgs/tauri-apps/discussions/11179).
    // Embed the v6 dependency into test binaries only.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' language='*' processorArchitecture='*'"
        );
    }

    tauri_build::build()
}
