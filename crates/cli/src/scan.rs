use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ast_grep_config::{
  from_yaml_string, CombinedScan, PreScan, RuleCollection, RuleConfig, Severity,
};
use ast_grep_core::{NodeMatch, StrDoc};
use ast_grep_language::SupportLang;
use clap::Args;
use ignore::WalkParallel;

use crate::config::{read_rule_file, with_rule_stats, ProjectConfig};
use crate::lang::SgLang;
use crate::print::{
  CloudPrinter, ColoredPrinter, Diff, InteractivePrinter, JSONPrinter, Platform, Printer,
  ReportStyle, SimpleFile,
};
use crate::utils::ErrorContext as EC;
use crate::utils::RuleOverwrite;
use crate::utils::{filter_file_interactive, ContextArgs, InputArgs, OutputArgs, OverwriteArgs};
use crate::utils::{FileTrace, ScanTrace};
use crate::utils::{Items, PathWorker, StdInWorker, Worker};

use std::collections::HashSet;

type AstGrep = ast_grep_core::AstGrep<StrDoc<SgLang>>;

#[derive(Args)]
pub struct ScanArg {
  /// Scan the codebase with the single rule located at the path RULE_FILE.
  ///
  /// It is useful to run single rule without project setup or sgconfig.yml.
  #[clap(short, long, value_name = "RULE_FILE")]
  rule: Option<PathBuf>,

  /// Scan the codebase with a rule defined by the provided RULE_TEXT.
  ///
  /// Use this argument if you want to test a rule without creating a YAML file on disk.
  /// You can run multiple rules by separating them with `---` in the RULE_TEXT.
  /// --inline-rules is incompatible with --rule.
  #[clap(long, conflicts_with = "rule", value_name = "RULE_TEXT")]
  inline_rules: Option<String>,

  /// Output warning/error messages in GitHub Action format.
  ///
  /// Currently, only GitHub is supported.
  #[clap(long, conflicts_with = "json", conflicts_with = "interactive")]
  format: Option<Platform>,

  #[clap(long, default_value = "rich", conflicts_with = "json")]
  report_style: ReportStyle,

  /// severity related options
  #[clap(flatten)]
  overwrite: OverwriteArgs,

  /// input related options
  #[clap(flatten)]
  input: InputArgs,
  /// output related options
  #[clap(flatten)]
  output: OutputArgs,
  /// context related options
  #[clap(flatten)]
  context: ContextArgs,
}

impl ScanArg {
  // whether the scan includes all rules available in the project
  fn include_all_rules(&self) -> bool {
    self.overwrite.include_all_rules() && self.rule.is_none() && self.inline_rules.is_none()
  }
}

pub fn run_with_config(arg: ScanArg, project: Result<ProjectConfig>) -> Result<()> {
  let project_trace = arg.output.inspect.project_trace();
  project_trace.print_project(&project)?;
  let context = arg.context.get();
  if let Some(_format) = &arg.format {
    let printer = CloudPrinter::stdout();
    return run_scan(arg, printer, project);
  }
  if let Some(json) = arg.output.json {
    let printer = JSONPrinter::stdout(json);
    return run_scan(arg, printer, project);
  }
  let printer = ColoredPrinter::stdout(arg.output.color)
    .style(arg.report_style)
    .context(context);
  let interactive = arg.output.needs_interactive();
  if interactive {
    let from_stdin = arg.input.stdin;
    let printer = InteractivePrinter::new(printer, arg.output.update_all, from_stdin)?;
    run_scan(arg, printer, project)
  } else {
    run_scan(arg, printer, project)
  }
}

fn run_scan<P: Printer + 'static>(
  arg: ScanArg,
  printer: P,
  project: Result<ProjectConfig>,
) -> Result<()> {
  if arg.input.stdin {
    let worker = ScanWithRule::try_new(arg)?;
    // TODO: report a soft error if rules have different languages
    worker.run_std_in(printer)
  } else {
    let worker = ScanWithConfig::try_new(arg, project)?;
    worker.run_path(printer)
  }
}

struct ScanWithConfig {
  arg: ScanArg,
  configs: RuleCollection<SgLang>,
  unused_suppression_rule: RuleConfig<SgLang>,
  trace: ScanTrace,
}
impl ScanWithConfig {
  fn try_new(arg: ScanArg, project: Result<ProjectConfig>) -> Result<Self> {
    let overwrite = RuleOverwrite::new(&arg.overwrite)?;
    let unused_suppression_rule = unused_suppression_rule_config(&arg, &overwrite);
    let (configs, rule_trace) = if let Some(path) = &arg.rule {
      let rules = read_rule_file(path, None)?;
      with_rule_stats(rules)?
    } else if let Some(text) = &arg.inline_rules {
      let rules = from_yaml_string(text, &Default::default())
        .with_context(|| EC::ParseRule("INLINE_RULES".into()))?;
      with_rule_stats(rules)?
    } else {
      // NOTE: only query project here since -r does not need project
      let project_config = project?;
      project_config.find_rules(overwrite)?
    };
    let trace = arg.output.inspect.scan_trace(rule_trace);
    trace.print_rules(&configs)?;
    Ok(Self {
      arg,
      configs,
      unused_suppression_rule,
      trace,
    })
  }
}
impl Worker for ScanWithConfig {
  type Item = (PathBuf, AstGrep, PreScan);
  fn consume_items<P: Printer>(&self, items: Items<Self::Item>, mut printer: P) -> Result<()> {
    printer.before_print()?;
    let mut error_count = 0usize;
    for (path, grep, pre_scan) in items {
      let file_content = grep.source().to_string();
      let path = &path;
      let rules = self.configs.get_rule_from_lang(path, *grep.lang());
      let mut combined = CombinedScan::new(rules);
      combined.set_unused_suppression_rule(&self.unused_suppression_rule);
      let interactive = self.arg.output.needs_interactive();
      // exclude_fix rule because we already have diff inspection before
      let scanned = combined.scan(&grep, pre_scan, /* separate_fix*/ interactive);
      if interactive {
        let diffs = scanned.diffs;
        match_rule_diff_on_file(path, diffs, &mut printer)?;
      }
      for (rule, matches) in scanned.matches {
        if matches!(rule.severity, Severity::Error) {
          error_count = error_count.saturating_add(matches.len());
        }
        match_rule_on_file(path, matches, rule, &file_content, &mut printer)?;
      }
    }
    printer.after_print()?;
    self.trace.print()?;
    if error_count > 0 {
      Err(anyhow::anyhow!(EC::DiagnosticError(error_count)))
    } else {
      Ok(())
    }
  }
}

// we should only suggest unused suppression if scan includes all rules
// otherwise, keep silent about unused suppressions because they may used by other rules
// this is a "smart" heuristic but user always can override it
fn default_unused_suppression_rule_severity(arg: &ScanArg) -> Severity {
  if arg.include_all_rules() {
    Severity::Hint
  } else {
    Severity::Off
  }
}

fn unused_suppression_rule_config(arg: &ScanArg, overwrite: &RuleOverwrite) -> RuleConfig<SgLang> {
  let severity = overwrite
    .find("unused-suppression")
    .severity
    .unwrap_or_else(|| default_unused_suppression_rule_severity(arg));
  CombinedScan::unused_config(severity, SupportLang::Rust.into())
}

impl PathWorker for ScanWithConfig {
  fn get_trace(&self) -> &FileTrace {
    &self.trace.inner.file_trace
  }
  fn build_walk(&self) -> Result<WalkParallel> {
    let mut langs = HashSet::new();
    self.configs.for_each_rule(|rule| {
      langs.insert(rule.language);
    });
    self.arg.input.walk_langs(langs.into_iter())
  }
  fn produce_item(&self, path: &Path) -> Option<Vec<Self::Item>> {
    let encoding = self.arg.input.encoding;
    filter_file_interactive(path, &self.configs, &self.trace, encoding)
  }
}

struct ScanWithRule {
  rules: Vec<RuleConfig<SgLang>>,
}
impl ScanWithRule {
  fn try_new(arg: ScanArg) -> Result<Self> {
    let rules = if let Some(path) = &arg.rule {
      read_rule_file(path, None)?
    } else if let Some(text) = &arg.inline_rules {
      from_yaml_string(text, &Default::default())
        .with_context(|| EC::ParseRule("INLINE_RULES".into()))?
    } else {
      return Err(anyhow::anyhow!(EC::RuleNotSpecified));
    };
    Ok(Self { rules })
  }
}

impl Worker for ScanWithRule {
  type Item = (PathBuf, AstGrep, PreScan);
  fn consume_items<P: Printer>(&self, items: Items<Self::Item>, mut printer: P) -> Result<()> {
    printer.before_print()?;
    let mut error_count = 0usize;
    let combined = CombinedScan::new(self.rules.iter().collect());
    for (path, grep, pre_scan) in items {
      let file_content = grep.source().to_string();
      // do not exclude_fix rule in run_with_rule
      let scanned = combined.scan(&grep, pre_scan, false);
      for (rule, matches) in scanned.matches {
        if matches!(rule.severity, Severity::Error) {
          error_count = error_count.saturating_add(matches.len());
        }
        match_rule_on_file(&path, matches, rule, &file_content, &mut printer)?;
      }
    }
    printer.after_print()?;
    if error_count > 0 {
      Err(anyhow::anyhow!(EC::DiagnosticError(error_count)))
    } else {
      Ok(())
    }
  }
}

impl StdInWorker for ScanWithRule {
  fn parse_stdin(&self, src: String) -> Option<Self::Item> {
    use ast_grep_core::Language;
    let lang = self.rules[0].language;
    let combined = CombinedScan::new(self.rules.iter().collect());
    let grep = lang.ast_grep(src);
    let pre_scan = combined.find(&grep);
    if !pre_scan.is_empty() {
      Some((PathBuf::from("STDIN"), grep, pre_scan))
    } else {
      None
    }
  }
}
fn match_rule_diff_on_file(
  path: &Path,
  matches: Vec<(&RuleConfig<SgLang>, NodeMatch<StrDoc<SgLang>>)>,
  reporter: &mut impl Printer,
) -> Result<()> {
  let diffs = matches
    .into_iter()
    .filter_map(|(rule, m)| {
      let fix = rule.matcher.fixer.as_ref()?;
      let diff = Diff::generate(m, &rule.matcher, fix);
      Some((diff, rule))
    })
    .collect();
  reporter.print_rule_diffs(diffs, path)?;
  Ok(())
}

fn match_rule_on_file(
  path: &Path,
  matches: Vec<NodeMatch<StrDoc<SgLang>>>,
  rule: &RuleConfig<SgLang>,
  file_content: &String,
  reporter: &mut impl Printer,
) -> Result<()> {
  let matches = matches.into_iter();
  let file = SimpleFile::new(path.to_string_lossy(), file_content);
  if let Some(fixer) = &rule.matcher.fixer {
    let diffs = matches
      .map(|m| (Diff::generate(m, &rule.matcher, fixer), rule))
      .collect();
    reporter.print_rule_diffs(diffs, path)?;
  } else {
    reporter.print_rule(matches, file, rule)?;
  }
  Ok(())
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::print::ColorArg;
  use std::fs::File;
  use std::io::Write;
  use tempfile::TempDir;

  const RULE: &str = r#"
id: test
message: Add your rule message here....
severity: error # error, warning, hint, info
language: Rust
rule:
  pattern: Some(123)
"#;

  // TODO: unify with verify::test
  pub fn create_test_files<'a>(
    names_and_contents: impl IntoIterator<Item = (&'a str, &'a str)>,
  ) -> TempDir {
    let dir = TempDir::new().unwrap();
    for (name, contents) in names_and_contents {
      let path = dir.path().join(name);
      let mut file = File::create(path.clone()).unwrap();
      file.write_all(contents.as_bytes()).unwrap();
      file.sync_all().unwrap();
    }
    dir
  }

  fn default_scan_arg() -> ScanArg {
    ScanArg {
      rule: None,
      inline_rules: None,
      report_style: ReportStyle::Rich,
      input: InputArgs {
        no_ignore: vec![],
        paths: vec![PathBuf::from(".")],
        stdin: false,
        follow: false,
        globs: vec![],
        threads: 0,
      },
      overwrite: OverwriteArgs {
        filter: None,
        error: None,
        warning: None,
        info: None,
        hint: None,
        off: None,
      },
      output: OutputArgs {
        interactive: false,
        json: None,
        update_all: false,
        color: ColorArg::Never,
        inspect: Default::default(),
      },
      context: ContextArgs {
        before: 0,
        after: 0,
        context: 0,
      },
      format: None,
    }
  }

  #[test]
  fn test_run_with_config() {
    let dir = create_test_files([("sgconfig.yml", "ruleDirs: [rules]")]);
    std::fs::create_dir_all(dir.path().join("rules")).unwrap();
    let mut file = File::create(dir.path().join("rules/test.yml")).unwrap();
    file.write_all(RULE.as_bytes()).unwrap();
    let mut file = File::create(dir.path().join("test.rs")).unwrap();
    file
      .write_all("fn test() { Some(123) }".as_bytes())
      .unwrap();
    file.sync_all().unwrap();
    let project_config = ProjectConfig::setup(Some(dir.path().join("sgconfig.yml"))).unwrap();
    let arg = default_scan_arg();
    assert!(run_with_config(arg, project_config).is_ok());
  }

  #[test]
  fn test_scan_with_inline_rules() {
    let inline_rules = "{id: test, language: ts, rule: {pattern: readFileSync}}".to_string();
    let arg = ScanArg {
      inline_rules: Some(inline_rules),
      ..default_scan_arg()
    };
    assert!(run_with_config(arg, Err(anyhow::anyhow!("not found"))).is_ok());
  }

  #[test]
  fn test_scan_with_inline_rules_diff() {
    let inline_rules =
      "{id: test, language: ts, rule: {pattern: readFileSync}, fix: 'nnn'}".to_string();
    let arg = ScanArg {
      inline_rules: Some(inline_rules),
      ..default_scan_arg()
    };
    assert!(run_with_config(arg, Err(anyhow::anyhow!("not found"))).is_ok());
  }

  // baseline test for coverage
  #[test]
  fn test_scan_with_inline_rules_error() {
    let inline_rules = "nonsense".to_string();
    let arg = ScanArg {
      inline_rules: Some(inline_rules),
      ..default_scan_arg()
    };
    let err = run_with_config(arg, Err(anyhow::anyhow!("not found"))).expect_err("should error");
    assert!(err.is::<EC>());
    assert_eq!(err.to_string(), "Cannot parse rule INLINE_RULES");
  }
}
