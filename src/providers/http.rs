//! Shared reqwest client for provider egress.
//!
//! Browser paths already attach netguard's resolver and redirect policy. The
//! three provider clients used to be a bare timeout, so `base_url`,
//! `CUSTOM_PROVIDER_BASE_URL`, and the default Ollama loopback origin could
//! redirect or resolve internal. This builder is the single remaining
//! construction path.

use crate::netguard::{
    blocked_reason, guarded_resolver, guarded_resolver_allowing, redirect_policy,
    redirect_policy_allowing, BlockReason,
};
use reqwest::Client;
use std::time::Duration;

const PROVIDER_TIMEOUT: Duration = Duration::from_secs(30);

/// HTTP client for one operator-configured provider origin.
///
/// The configured origin — including loopback, such as Ollama's default
/// `http://localhost:11434` — is reachable. Every redirect hop that leaves
/// that origin is judged with the same SSRF check as the browser client.
pub(crate) fn client_for_origin(origin: &str) -> Client {
    let mut builder = Client::builder().timeout(PROVIDER_TIMEOUT);

    builder = match url::Url::parse(origin) {
        Ok(allowed) => {
            let resolver = match (allowed.host_str(), blocked_reason(allowed.as_str())) {
                (Some(host), Some(BlockReason::InternalAddress(_))) => {
                    guarded_resolver_allowing(host)
                }
                _ => guarded_resolver(),
            };
            builder
                .redirect(redirect_policy_allowing(Some(allowed)))
                .dns_resolver(resolver)
        }
        Err(_) => builder
            .redirect(redirect_policy())
            .dns_resolver(guarded_resolver()),
    };

    builder
        .build()
        .expect("failed to create provider HTTP client")
}

#[cfg(test)]
mod tests {
    use super::client_for_origin;
    use crate::netguard::blocked_reason;
    use crate::providers::{
        AiContext, AiProvider, OllamaProvider, ProviderConfig, ProviderType, ScrollPosition,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    fn empty_ctx() -> AiContext {
        AiContext {
            current_url: String::new(),
            page_title: String::new(),
            dom_snapshot: String::new(),
            accessibility_tree: None,
            scroll_position: ScrollPosition { x: 0.0, y: 0.0 },
            tool_results: Vec::new(),
            conversation_history: Vec::new(),
        }
    }

    fn serve_one(
        status_line: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("accept timeout");
        let port = listener.local_addr().expect("local addr").port();
        let status_line = status_line.to_string();
        let headers: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        let body = body.to_vec();
        let handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let mut out = format!("{status_line}\r\n");
                for (k, v) in &headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ));
                let _ = stream.write_all(out.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (port, handle)
    }

    #[test]
    fn default_ollama_origin_is_blocked_for_browser_paths() {
        assert!(
            blocked_reason("http://localhost:11434").is_some(),
            "the browser guard must still refuse default Ollama; the allow is provider-only"
        );
    }

    #[tokio::test]
    async fn configured_loopback_ollama_still_works() {
        let json = br#"{"response":"hello from ollama","done":true}"#;
        let (port, server) = serve_one(
            "HTTP/1.1 200 OK",
            &[("Content-Type", "application/json")],
            json,
        );
        let origin = format!("http://127.0.0.1:{port}");

        let provider = OllamaProvider::new(ProviderConfig {
            provider_type: ProviderType::Ollama,
            api_key: None,
            base_url: Some(origin),
            model: "test".to_string(),
            max_tokens: Some(16),
            temperature: Some(0.0),
        });

        let response = provider
            .complete("hi", &empty_ctx())
            .await
            .expect("configured loopback Ollama must remain reachable");
        assert_eq!(response.content, "hello from ollama");
        let _ = server.join();
    }

    #[tokio::test]
    async fn redirect_hop_to_blocked_internal_host_fails_closed() {
        let (port, server) = serve_one(
            "HTTP/1.1 302 Found",
            &[("Location", "http://169.254.169.254/latest/meta-data/")],
            b"",
        );
        let origin = format!("http://127.0.0.1:{port}");
        let client = client_for_origin(&origin);

        let err = client
            .get(format!("{origin}/v1"))
            .send()
            .await
            .expect_err("a hop onto cloud metadata must not be followed");
        let msg = err.to_string();
        assert!(
            msg.contains("internal") || msg.contains("169.254") || msg.contains("Refusing"),
            "error must mention the blocked hop, got: {msg}"
        );
        let _ = server.join();
    }
}
