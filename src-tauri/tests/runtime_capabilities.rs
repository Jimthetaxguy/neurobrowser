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
    let permissions = manifests["__app-acl__"]["permissions"].as_object().unwrap();
    let commands: Vec<_> = permissions
        .values()
        .flat_map(|p| p["commands"]["allow"].as_array().unwrap())
        .map(|c| c.as_str().unwrap())
        .filter(|c| *c != "browser_runtime_report")
        .collect();
    assert!(commands.contains(&"submit_approval"));
    assert!(commands.contains(&"set_provider"));
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
