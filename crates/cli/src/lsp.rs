use crate::config::find_rules;
use crate::error::ErrorContext as EC;
use anyhow::{Context, Result};
use ast_grep_lsp::{Backend, LspService, Server};

async fn run_language_server_impl() -> Result<()> {
  // env_logger::init();

  let stdin = tokio::io::stdin();
  let stdout = tokio::io::stdout();
  let config = find_rules(None)?;

  let (service, socket) = LspService::build(|client| Backend::new(client, config))
    .custom_method("ast-grep/scan", Backend::scan)
    .finish();
  Server::new(stdin, stdout, socket).serve(service).await;
  Ok(())
}

pub fn run_language_server() -> Result<()> {
  tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()
    .context(EC::StartLanguageServer)?
    .block_on(async { run_language_server_impl().await })
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn test_lsp_start() {
    assert!(run_language_server().is_err())
  }
}
