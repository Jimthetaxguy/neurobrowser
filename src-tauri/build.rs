fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "browser_back",
            "browser_forward",
            "browser_reload",
            "browser_runtime_report",
            "cancel_agent_run",
            "close_page",
            "create_page",
            "create_session",
            "explain_memory_result",
            "forget_memory",
            "get_action_policy",
            "get_page_snapshot",
            "navigate",
            "search_local_memory",
            "set_action_policy",
            "set_active_page",
            "set_provider",
            "start_agent_run",
            "submit_approval",
            "sync_browser_viewport",
        ]),
    ))
    .expect("failed to run tauri build script")
}
