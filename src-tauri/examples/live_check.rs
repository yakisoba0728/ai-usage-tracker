//! Live check: runs the real provider `fetch()` against live endpoints.
//!   cargo run --example live_check --release

#[tokio::main]
async fn main() {
    use ai_usage_tracker_lib::providers::{
        claude::ClaudeProvider, codex::CodexProvider, copilot::CopilotProvider,
        cursor::CursorProvider, fetch_all, gemini::GeminiProvider, ProviderApi,
    };
    let providers: Vec<Box<dyn ProviderApi>> = vec![
        Box::new(ClaudeProvider::new()),
        Box::new(CodexProvider::new()),
        Box::new(GeminiProvider::new()),
        Box::new(CopilotProvider::new()),
        Box::new(CursorProvider::new()),
    ];
    let results = fetch_all(providers).await;
    for u in &results {
        let wins: Vec<String> = u
            .windows
            .iter()
            .map(|w| {
                format!(
                    "{}={}%",
                    w.label,
                    w.used_percent.map(|p| p.to_string()).unwrap_or("?".into())
                )
            })
            .collect();
        println!(
            "{:?}: connected={} plan={:?} account={:?} err={:?} [ {} ]",
            u.provider,
            u.connected,
            u.plan,
            u.account,
            u.error,
            wins.join(", ")
        );
    }
}
