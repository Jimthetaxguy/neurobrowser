//! Manual smoke test proving the app's real provider code makes a
//! successful call to a real LLM API using Infisical-injected keys.
//!
//! This exercises `neurobrowser::providers::create_provider` +
//! `AiProvider::complete` — the exact code path `src/agent/mod.rs` and
//! `src/session/mod.rs` use for live agent runs (`start_agent_run` in
//! `src-tauri/src/main.rs`). No mocks, no stubs: real HTTP calls to
//! api.anthropic.com / api.openai.com.
//!
//! `#[ignore]`d by default (costs money, needs network + real credentials).
//! Run explicitly, from the repo root, with keys injected by Infisical:
//!
//!   cd ~/code && infisical run --env=dev --silent -- \
//!     cargo test --manifest-path <repo>/Cargo.toml \
//!       --test real_provider_smoke -- --ignored --nocapture
//!
//! Never prints API key values — only response content/lengths.

use neurobrowser::providers::create_provider;
use neurobrowser::{AiContext, ProviderConfig, ProviderType};

fn ctx() -> AiContext {
    AiContext {
        current_url: "https://example.com".to_string(),
        page_title: "Example Domain".to_string(),
        tool_results: Vec::new(),
        personal_memory: false,
    }
}

#[tokio::test]
#[ignore]
async fn anthropic_real_call_smoke() {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY not set — run under `infisical run --env=dev`");

    // Same default model + temperature handling as `provider_config_for` in
    // `src-tauri/src/main.rs`: current Claude models reject an explicit
    // `temperature` with a 400, so it is left unset here too.
    let config = ProviderConfig {
        provider_type: ProviderType::Anthropic,
        api_key: Some(api_key),
        base_url: None,
        model: "claude-sonnet-5".to_string(),
        max_tokens: Some(16),
        temperature: None,
    };

    let provider = create_provider(&config);
    let response = provider
        .complete("Reply with only the digit: what is 2+2?", &ctx())
        .await
        .expect("real Anthropic API call failed");

    println!("[anthropic] provider_name={}", provider.provider_name());
    println!("[anthropic] response_content={:?}", response.content);
    assert!(
        !response.content.trim().is_empty(),
        "expected a non-empty response from the real Anthropic API"
    );
}

#[tokio::test]
#[ignore]
async fn openai_real_call_smoke() {
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY not set — run under `infisical run --env=dev`");

    let config = ProviderConfig {
        provider_type: ProviderType::Openai,
        api_key: Some(api_key),
        base_url: None,
        model: "gpt-4o-mini".to_string(),
        max_tokens: Some(16),
        temperature: Some(0.0),
    };

    let provider = create_provider(&config);
    let response = provider
        .complete("Reply with only the digit: what is 2+2?", &ctx())
        .await
        .expect("real OpenAI API call failed");

    println!("[openai] provider_name={}", provider.provider_name());
    println!("[openai] response_content={:?}", response.content);
    assert!(
        !response.content.trim().is_empty(),
        "expected a non-empty response from the real OpenAI API"
    );
}
