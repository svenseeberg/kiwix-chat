mod agent;
mod config;
mod kiwix;
mod llm;
mod tui;

use anyhow::{bail, Context, Result};
use clap::Parser;

use config::{Cli, LLM_AUTODETECT_URLS, LLM_PROBE_TIMEOUT_SECS};
use kiwix::KiwixClient;
use llm::LlmClient;
use tui::App;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _guard = init_logging(cli.verbose);

    // Resolve the LLM backend (explicit URL or autodetection).
    let (llm_base, model) = resolve_llm(&cli)
        .await
        .context("no usable OpenAI-compatible LLM backend found")?;
    let llm = LlmClient::new(llm_base, model)?;

    // Set up the Kiwix client and check reachability (non-fatal).
    let kiwix = KiwixClient::new(cli.kiwix_base())?;
    let kiwix_reachable = kiwix.is_reachable().await;

    let app = App::new(
        llm,
        kiwix,
        kiwix_reachable,
        cli.lang.clone(),
        cli.max_rounds,
    );
    tui::run(app).await
}

/// Determine the LLM base URL and model to use.
async fn resolve_llm(cli: &Cli) -> Result<(String, String)> {
    // Explicit URL: probe once and honor any explicit model.
    if let Some(url) = &cli.llm_url {
        let models = LlmClient::probe_models(url, LLM_PROBE_TIMEOUT_SECS)
            .await
            .with_context(|| format!("probing LLM backend at {url}"))?;
        let model = pick_model(cli.model.clone(), models, url)?;
        return Ok((url.clone(), model));
    }

    // Autodetect: try each candidate; use the first that reports at least one model.
    let mut last_err = None;
    for url in LLM_AUTODETECT_URLS {
        match LlmClient::probe_models(url, LLM_PROBE_TIMEOUT_SECS).await {
            Ok(models) if !models.is_empty() => {
                let model = pick_model(cli.model.clone(), models, url)?;
                eprintln!("Detected LLM backend at {url} (model: {model})");
                return Ok((url.to_string(), model));
            }
            Ok(_) => last_err = Some(format!("{url}: reachable but no models loaded")),
            Err(e) => last_err = Some(format!("{url}: {e}")),
        }
    }
    bail!(
        "could not autodetect an LLM backend on {}. Last error: {}. \
         Start llama.cpp or Ollama, or pass --llm-url.",
        LLM_AUTODETECT_URLS.join(", "),
        last_err.unwrap_or_else(|| "none".to_string())
    )
}

/// Choose the model id: explicit override if given, otherwise the first advertised model.
fn pick_model(explicit: Option<String>, available: Vec<String>, url: &str) -> Result<String> {
    if let Some(m) = explicit {
        return Ok(m);
    }
    available
        .into_iter()
        .next()
        .with_context(|| format!("{url} reported no models; pass --model to choose one"))
}

/// Initialize file-based logging when `--verbose` is set. Returns a guard that must be
/// kept alive for the duration of the program so buffered logs are flushed.
fn init_logging(verbose: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    if !verbose {
        return None;
    }
    use tracing_subscriber::EnvFilter;
    let file = tracing_appender::rolling::never(".", "kiwix-chat.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("kiwix_chat=debug,info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Some(guard)
}
