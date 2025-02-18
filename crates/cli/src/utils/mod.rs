mod args;
mod debug_query;
mod error_context;
mod inspect;
mod rule_overwrite;
mod worker;

pub use args::{ContextArgs, InputArgs, OutputArgs, OverwriteArgs};
pub use debug_query::DebugFormat;
use encoding_rs::Encoding;
pub use error_context::{exit_with_error, ErrorContext};
pub use inspect::{FileTrace, Granularity, RuleTrace, RunTrace, ScanTrace};
pub use rule_overwrite::RuleOverwrite;
pub use worker::{Items, PathWorker, StdInWorker, Worker};

use crate::lang::SgLang;

use anyhow::{Context, Result};
use crossterm::{
  cursor::MoveTo,
  event::{self, Event, KeyCode},
  execute,
  terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
  terminal::{Clear, ClearType},
};

use ast_grep_config::{CombinedScan, PreScan, RuleCollection};
use ast_grep_core::Pattern;
use ast_grep_core::{Matcher, StrDoc};
use ast_grep_language::Language;

use std::fs::read;
use std::io::stdout;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

type AstGrep = ast_grep_core::AstGrep<StrDoc<SgLang>>;

fn read_char() -> Result<char> {
  loop {
    if let Event::Key(evt) = event::read()? {
      match evt.code {
        KeyCode::Enter => break Ok('\n'),
        KeyCode::Char(c) => break Ok(c),
        _ => (),
      }
    }
  }
}

/// Prompts for user input on STDOUT
fn prompt_reply_stdout(prompt: &str) -> Result<char> {
  let mut stdout = std::io::stdout();
  write!(stdout, "{}", prompt)?;
  stdout.flush()?;
  terminal::enable_raw_mode()?;
  let ret = read_char();
  terminal::disable_raw_mode()?;
  ret
}

// clear screen
fn clear() -> Result<()> {
  execute!(stdout(), Clear(ClearType::All), MoveTo(0, 0))?;
  Ok(())
  // https://github.com/console-rs/console/blob/be1c2879536c90ffc2b54938b5964084f5fef67d/src/common_term.rs#L56
  // print!("\r\x1b[2J\r\x1b[H");
}

pub fn run_in_alternate_screen<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
  execute!(stdout(), EnterAlternateScreen)?;
  clear()?;
  let ret = f();
  execute!(stdout(), LeaveAlternateScreen)?;
  ret
}

pub fn prompt(prompt_text: &str, letters: &str, default: Option<char>) -> Result<char> {
  loop {
    let input = prompt_reply_stdout(prompt_text)?;
    if let Some(default) = default {
      if input == '\n' {
        return Ok(default);
      }
    }
    if letters.contains(input) {
      return Ok(input);
    }
    eprintln!("Unrecognized command, try again?")
  }
}

fn read_file(path: &Path, encoding: &'static Encoding) -> Option<String> {
  let file_bytes = read(path)
    .with_context(|| format!("Cannot read file {}", path.to_string_lossy()))
    .map_err(|err| eprintln!("{err:#}"))
    .ok()?;
  let (file_content, _encoding_used, had_errors) = encoding.decode(&file_bytes);
  if had_errors {
    // TODO add output
    return None;
  }
  let file_content: String = file_content.into();
  // skip large files or empty file
  if file_too_large(&file_content) || file_content.is_empty() {
    // TODO add output
    None
  } else {
    Some(file_content)
  }
}

fn filter(
  grep: &AstGrep,
  path: &Path,
  lang: SgLang,
  configs: &RuleCollection<SgLang>,
  rule_stats: &ScanTrace,
) -> Option<PreScan> {
  let rules = configs.get_rule_from_lang(path, lang);
  rule_stats.print_file(path, lang, &rules).ok()?;
  let combined = CombinedScan::new(rules);
  let pre_scan = combined.find(grep);
  if pre_scan.is_empty() {
    None
  } else {
    Some(pre_scan)
  }
}

pub fn filter_file_interactive(
  path: &Path,
  configs: &RuleCollection<SgLang>,
  trace: &ScanTrace,
  encoding: &'static Encoding,
) -> Option<Vec<(PathBuf, AstGrep, PreScan)>> {
  let lang = SgLang::from_path(path)?;
  let file_content = read_file(path, encoding)?;
  let grep = lang.ast_grep(file_content);
  let mut ret = vec![];
  let root = filter(&grep, path, lang, configs, trace)
    .map(|pre_scan| (path.to_path_buf(), grep.clone(), pre_scan));
  ret.extend(root);
  if let Some(injected) = lang.injectable_sg_langs() {
    let docs = grep.inner.get_injections(|s| SgLang::from_str(s).ok());
    let inj = injected.filter_map(|l| {
      let doc = docs.iter().find(|d| *d.lang() == l)?;
      let grep = AstGrep { inner: doc.clone() };
      let pre_scan = filter(&grep, path, l, configs, trace)?;
      Some((path.to_path_buf(), grep, pre_scan))
    });
    ret.extend(inj)
  }
  Some(ret)
}

pub fn filter_file_pattern(
  path: &Path,
  lang: SgLang,
  root_matcher: Option<Pattern<SgLang>>,
  matchers: impl Iterator<Item = (SgLang, Pattern<SgLang>)>,
  encoding: &'static Encoding,
) -> Option<Vec<(MatchUnit<Pattern<SgLang>>, SgLang)>> {
  let file_content = read_file(path, encoding)?;
  let grep = lang.ast_grep(&file_content);
  let do_match = |ast_grep: AstGrep, matcher: Pattern<SgLang>, lang: SgLang| {
    let fixed = matcher.fixed_string();
    if !fixed.is_empty() && !file_content.contains(&*fixed) {
      return None;
    }
    let has_match = ast_grep.root().find(&matcher).is_some();
    has_match.then(|| {
      (
        MatchUnit {
          grep: ast_grep,
          path: path.to_path_buf(),
          matcher,
        },
        lang,
      )
    })
  };
  let mut ret = vec![];
  if let Some(matcher) = root_matcher {
    ret.extend(do_match(grep.clone(), matcher, lang));
  }
  let injections = grep.inner.get_injections(|s| SgLang::from_str(s).ok());
  for (i_lang, matcher) in matchers {
    let Some(injection) = injections.iter().find(|i| *i.lang() == i_lang) else {
      continue;
    };
    let injected = AstGrep {
      inner: injection.clone(),
    };
    ret.extend(do_match(injected, matcher, i_lang));
  }
  Some(ret)
}

const MAX_FILE_SIZE: usize = 3_000_000;
const MAX_LINE_COUNT: usize = 200_000;

// skip files that are too large in size AND have too many lines
fn file_too_large(file_content: &str) -> bool {
  // the && operator is intentional here to include more files
  file_content.len() > MAX_FILE_SIZE && file_content.lines().count() > MAX_LINE_COUNT
}

/// A single atomic unit where matches happen.
/// It contains the file path, sg instance and matcher.
/// An analogy to compilation unit in C programming language.
pub struct MatchUnit<M: Matcher<SgLang>> {
  pub path: PathBuf,
  pub grep: AstGrep,
  pub matcher: M,
}

#[cfg(test)]
mod test {
  use super::*;
  use ast_grep_language::SupportLang;

  #[test]
  fn test_html_embedding() {
    let root =
      SgLang::Builtin(SupportLang::Html).ast_grep("<script lang=typescript>alert(123)</script>");
    let docs = root.inner.get_injections(|s| SgLang::from_str(s).ok());
    assert_eq!(docs.len(), 1);
    let script = docs[0].root().child(0).expect("should exist");
    assert_eq!(script.kind(), "expression_statement");
  }

  #[test]
  fn test_html_embedding_lang_not_found() {
    let root = SgLang::Builtin(SupportLang::Html).ast_grep("<script lang=xxx>alert(123)</script>");
    let docs = root.inner.get_injections(|s| SgLang::from_str(s).ok());
    assert_eq!(docs.len(), 0);
  }
}
