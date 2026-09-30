use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ActionBrowser {
    fail_action: bool,
    ready: bool,
    actions: AtomicUsize,
    waits: AtomicUsize,
    submission_url: Option<String>,
}

impl ActionBrowser {
    fn new(fail_action: bool, ready: bool) -> Self {
        Self {
            fail_action,
            ready,
            actions: AtomicUsize::new(0),
            waits: AtomicUsize::new(0),
            submission_url: None,
        }
    }

    fn action(&self) -> Result<(), String> {
        self.actions.fetch_add(1, Ordering::SeqCst);
        if self.fail_action {
            Err("No matching element".to_string())
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl BrowserInterface for ActionBrowser {
    async fn navigate(&self, _url: &str) -> Result<(), String> {
        self.action()
    }
    async fn click(&self, _selector: &str) -> Result<(), String> {
        self.action()
    }
    async fn submit_form(&self, _selector: &str) -> Result<(), String> {
        self.action()?;
        if let Some(url) = &self.submission_url {
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .map_err(|error| error.to_string())?
                .post(url)
                .send()
                .await
                .map_err(|error| error.to_string())?
                .error_for_status()
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
    async fn keypress(&self, _key: &str) -> Result<(), String> {
        self.action()
    }
    async fn browser_reload(&self) -> Result<(), String> {
        self.action()
    }
    async fn query_selector(&self, _selector: &str) -> Result<Vec<ElementInfo>, String> {
        Err("Unused in this test".to_string())
    }
    async fn get_text(&self, _selector: &str) -> Result<String, String> {
        Err("Unused in this test".to_string())
    }
    async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), String> {
        Err("Unused in this test".to_string())
    }
    async fn scroll_to(&self, _selector: &str) -> Result<(), String> {
        Err("Unused in this test".to_string())
    }
    async fn scroll_by(&self, _x: f32, _y: f32) -> Result<(), String> {
        Err("Unused in this test".to_string())
    }
    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        Err("Unused in this test".to_string())
    }
    async fn wait_for_navigation(&self) -> Result<(), String> {
        self.waits.fetch_add(1, Ordering::SeqCst);
        if let Some(url) = &self.submission_url {
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .map_err(|error| error.to_string())?
                .get(format!("{url}/receipt"))
                .timeout(std::time::Duration::from_millis(50))
                .send()
                .await
                .map_err(|error| format!("Receipt page readiness failed: {error}"))?;
        }
        if self.ready {
            Ok(())
        } else {
            Err("Timed out waiting for page to finish loading".to_string())
        }
    }
}

#[tokio::test]
async fn executed_actions_keep_success_and_surface_readiness_timeout() {
    for tool_name in ["click", "submit_form", "keypress", "reload"] {
        let browser = ActionBrowser::new(false, false);
        let result = default_tool_registry()
            .get(tool_name)
            .unwrap()
            .execute(HashMap::new(), &browser)
            .await;

        assert!(result.success, "{tool_name}: {}", result.result);
        assert!(result.result.contains("page readiness is unconfirmed"));
        assert!(result.result.contains("Timed out waiting for page"));
        assert!(result.result.contains("Do not repeat the action"));
        assert_eq!(browser.actions.load(Ordering::SeqCst), 1);
        assert_eq!(browser.waits.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn action_failures_remain_failures_and_do_not_wait() {
    for tool_name in ["click", "submit_form", "keypress", "reload"] {
        let browser = ActionBrowser::new(true, false);
        let result = default_tool_registry()
            .get(tool_name)
            .unwrap()
            .execute(HashMap::new(), &browser)
            .await;

        assert!(!result.success);
        assert_eq!(result.result, "No matching element");
        assert_eq!(browser.actions.load(Ordering::SeqCst), 1);
        assert_eq!(browser.waits.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn ready_actions_have_no_warning_and_navigate_does_not_wait_twice() {
    for tool_name in ["click", "submit_form", "keypress", "reload", "navigate"] {
        let browser = ActionBrowser::new(false, true);
        let result = default_tool_registry()
            .get(tool_name)
            .unwrap()
            .execute(HashMap::new(), &browser)
            .await;

        assert!(result.success);
        assert!(!result.result.contains("unconfirmed"));
        assert_eq!(browser.actions.load(Ordering::SeqCst), 1);
        assert_eq!(
            browser.waits.load(Ordering::SeqCst),
            usize::from(tool_name != "navigate")
        );
    }
}

#[tokio::test]
async fn explicit_wait_still_reports_readiness_failure() {
    let browser = ActionBrowser::new(false, false);
    let result = default_tool_registry()
        .get("wait")
        .unwrap()
        .execute(HashMap::new(), &browser)
        .await;
    assert!(!result.success);
    assert!(result.result.contains("Timed out waiting for page"));
    assert_eq!(browser.actions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn real_post_is_not_reported_failed_when_receipt_loading_times_out() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let submission_url = format!("http://{}/submit", listener.local_addr().unwrap());
    let posts = Arc::new(AtomicUsize::new(0));
    let server_posts = posts.clone();
    let server = std::thread::spawn(move || {
        for expected in ["POST /submit ", "GET /submit/receipt "] {
            let deadline = Instant::now() + Duration::from_secs(3);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "HTTP request never arrived: {expected}"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("HTTP accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0_u8; 4096];
            let len = stream.read(&mut bytes).unwrap();
            let request = std::str::from_utf8(&bytes[..len]).unwrap();
            assert!(request.starts_with(expected), "{request}");
            if expected.starts_with("POST") {
                server_posts.fetch_add(1, Ordering::SeqCst);
                stream
                    .write_all(b"HTTP/1.1 303 See Other\r\nLocation: /submit/receipt\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            } else {
                // The external action already happened; only its destination
                // page is slow. This is a real local HTTP timeout, not a
                // canned tool error or an external paid service.
                std::thread::sleep(Duration::from_millis(150));
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
            }
        }
    });
    let mut browser = ActionBrowser::new(false, true);
    browser.submission_url = Some(submission_url);
    let result = default_tool_registry()
        .get("submit_form")
        .unwrap()
        .execute(
            HashMap::from([("selector".into(), "form".into())]),
            &browser,
        )
        .await;
    server.join().unwrap();

    assert!(result.success, "{}", result.result);
    assert!(result.result.contains("Form submission dispatched"));
    assert!(result.result.contains("page readiness is unconfirmed"));
    assert!(result.result.contains("Receipt page readiness failed"));
    assert!(result.result.contains("Do not repeat the action"));
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    assert_eq!(browser.actions.load(Ordering::SeqCst), 1);
}
