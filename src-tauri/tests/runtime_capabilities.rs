//! Exercise the shipped ACL with Tauri's real resolver; no app or provider is launched.
use tauri::ipc::Origin;

fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

fn remote(url: &str) -> Origin {
    Origin::Remote {
        url: url.parse().unwrap(),
    }
}

#[test]
fn page_reports_work_on_initial_blank_and_remote_documents() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for url in [
        "about:blank",
        "https://example.com/path",
        "http://example.com/path",
    ] {
        assert!(
            authority
                .resolve_access(
                    "browser_runtime_report",
                    "main",
                    "page-runtime-1",
                    &remote(url)
                )
                .is_some(),
            "page runtime must be able to reply from {url}"
        );
    }
}

#[test]
fn page_webviews_never_inherit_control_commands() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    let manifests: serde_json::Value =
        serde_json::from_str(include_str!("../gen/schemas/acl-manifests.json")).unwrap();
    let main_capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/main.json")).unwrap();
    let permissions = main_capability["permissions"].as_array().unwrap();
    let mut commands = Vec::new();
    for permission in permissions {
        let permission = permission.as_str().unwrap();
        let allowed = manifests["__app-acl__"]["permissions"][permission]["commands"]["allow"]
            .as_array()
            .unwrap();
        commands.extend(allowed.iter().map(|command| command.as_str().unwrap()));
    }
    commands.retain(|command| *command != "browser_runtime_report");
    assert!(commands.contains(&"submit_approval"));
    assert!(commands.contains(&"set_provider"));
    assert!(commands.contains(&"ask_assistant"));
    assert!(commands.contains(&"cancel_assistant_request"));
    assert!(commands.contains(&"get_provider_status"));
    assert!(commands.contains(&"find_in_page"));
    assert!(commands.contains(&"adjust_page_zoom"));
    for command in commands {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_some(),
            "control webview lost {command}"
        );
        for origin in [
            Origin::Local,
            remote("about:blank"),
            remote("http://example.com/"),
            remote("https://example.com/"),
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "page-runtime-1", &origin)
                    .is_none(),
                "page webview gained {command} from {origin}"
            );
        }
        assert!(
            authority
                .resolve_access(command, "main", "main", &remote("https://example.com/"))
                .is_none(),
            "remote control-webview content gained {command}"
        );
    }
}

#[test]
fn reports_require_a_page_webview_and_an_allowed_document() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for url in ["about:blank", "https://example.com/", "http://example.com/"] {
        for label in ["main", "untrusted", "page-runtime"] {
            assert!(
                authority
                    .resolve_access("browser_runtime_report", "main", label, &remote(url))
                    .is_none(),
                "unexpected report authority for {label} at {url}"
            );
        }
    }
    for origin in [
        Origin::Local,
        remote("about:srcdoc"),
        remote("about:blankevil"),
        remote("file:///tmp/page.html"),
        remote("data:text/html,hello"),
    ] {
        assert!(
            authority
                .resolve_access("browser_runtime_report", "main", "page-runtime-1", &origin)
                .is_none(),
            "unexpected report authority for {origin}"
        );
    }
}

#[test]
fn stop_page_load_is_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    assert!(
        authority
            .resolve_access("stop_page_load", "main", "main", &Origin::Local)
            .is_some(),
        "control webview lost stop_page_load"
    );
    for origin in [
        Origin::Local,
        remote("about:blank"),
        remote("http://example.com/"),
        remote("https://example.com/"),
    ] {
        assert!(
            authority
                .resolve_access("stop_page_load", "main", "page-runtime-1", &origin)
                .is_none(),
            "page webview gained stop_page_load from {origin}"
        );
    }
    assert!(
        authority
            .resolve_access(
                "stop_page_load",
                "main",
                "main",
                &remote("https://example.com/")
            )
            .is_none(),
        "remote control-webview content gained stop_page_load"
    );
}

#[test]
fn find_in_page_is_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    assert!(
        authority
            .resolve_access("find_in_page", "main", "main", &Origin::Local)
            .is_some(),
        "control webview lost find_in_page"
    );
    for origin in [
        Origin::Local,
        remote("about:blank"),
        remote("http://example.com/"),
        remote("https://example.com/"),
    ] {
        assert!(
            authority
                .resolve_access("find_in_page", "main", "page-runtime-1", &origin)
                .is_none(),
            "page webview gained find_in_page from {origin}"
        );
    }
    assert!(
        authority
            .resolve_access(
                "find_in_page",
                "main",
                "main",
                &remote("https://example.com/")
            )
            .is_none(),
        "remote control-webview content gained find_in_page"
    );
}

#[test]
fn adjust_page_zoom_is_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    assert!(
        authority
            .resolve_access("adjust_page_zoom", "main", "main", &Origin::Local)
            .is_some(),
        "control webview lost adjust_page_zoom"
    );
    for origin in [
        Origin::Local,
        remote("about:blank"),
        remote("http://example.com/"),
        remote("https://example.com/"),
    ] {
        assert!(
            authority
                .resolve_access("adjust_page_zoom", "main", "page-runtime-1", &origin)
                .is_none(),
            "page webview gained adjust_page_zoom from {origin}"
        );
    }
    assert!(
        authority
            .resolve_access(
                "adjust_page_zoom",
                "main",
                "main",
                &remote("https://example.com/")
            )
            .is_none(),
        "remote control-webview content gained adjust_page_zoom"
    );
}

#[test]
fn search_browsing_history_is_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    assert!(
        authority
            .resolve_access("search_browsing_history", "main", "main", &Origin::Local)
            .is_some(),
        "control webview lost search_browsing_history"
    );
    for origin in [
        Origin::Local,
        remote("about:blank"),
        remote("http://example.com/"),
        remote("https://example.com/"),
    ] {
        assert!(
            authority
                .resolve_access("search_browsing_history", "main", "page-runtime-1", &origin)
                .is_none(),
            "page webview gained search_browsing_history from {origin}"
        );
    }
    assert!(
        authority
            .resolve_access(
                "search_browsing_history",
                "main",
                "main",
                &remote("https://example.com/")
            )
            .is_none(),
        "remote control-webview content gained search_browsing_history"
    );
}

#[test]
fn download_commands_are_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for command in ["open_download", "show_download_in_folder"] {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_some(),
            "control webview lost {command}"
        );
        for origin in [
            Origin::Local,
            remote("about:blank"),
            remote("http://example.com/"),
            remote("https://example.com/"),
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "page-runtime-1", &origin)
                    .is_none(),
                "page webview gained {command} from {origin}"
            );
        }
        assert!(
            authority
                .resolve_access(command, "main", "main", &remote("https://example.com/"))
                .is_none(),
            "remote control-webview content gained {command}"
        );
    }
}

#[test]
fn page_lifecycle_listen_is_main_webview_only() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for command in ["plugin:event|listen", "plugin:event|unlisten"] {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_some(),
            "main webview lost {command}"
        );
        for origin in [
            Origin::Local,
            remote("about:blank"),
            remote("https://example.com/"),
            remote("http://example.com/"),
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "page-runtime-1", &origin)
                    .is_none(),
                "page webview gained {command} from {origin}"
            );
        }
    }
    for command in ["plugin:event|emit", "plugin:event|emit_to"] {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_none(),
            "main webview gained {command}"
        );
        assert!(
            authority
                .resolve_access(
                    command,
                    "main",
                    "page-runtime-1",
                    &remote("https://example.com/")
                )
                .is_none(),
            "page webview gained {command}"
        );
    }
}

#[test]
fn popup_handling_does_not_grant_page_webviews_a_command() {
    let page_capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/page-runtime.json")).unwrap();
    let permissions: Vec<&str> = page_capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap())
        .collect();
    assert_eq!(
        permissions,
        vec!["allow-browser-runtime-report"],
        "page webviews stay report-only"
    );

    let mut context = context();
    let authority = context.runtime_authority_mut();
    assert!(
        authority
            .resolve_access(
                "browser_runtime_report",
                "main",
                "page-runtime-1",
                &remote("https://example.com/")
            )
            .is_some(),
        "a blocked popup is reported on the existing page command"
    );
    for command in ["open_popup", "create_popup", "allow_popup"] {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_none(),
            "main webview gained an unshipped {command}"
        );
        assert!(
            authority
                .resolve_access(
                    command,
                    "main",
                    "page-runtime-1",
                    &remote("https://example.com/")
                )
                .is_none(),
            "page webview gained {command}"
        );
    }
}
