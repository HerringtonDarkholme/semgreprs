#![cfg(not(feature = "napi-noop-in-unit-test"))]

mod doc;
mod sg_node;

use ast_grep_config::{RuleWithConstraint, SerializableRuleCore};
use ast_grep_core::language::Language;
use ast_grep_core::pinned::{NodeData, PinnedNodeData};
use ast_grep_core::{AstGrep, NodeMatch};
use ignore::types::TypesBuilder;
use ignore::{WalkBuilder, WalkState};
use napi::anyhow::{anyhow, Context, Error, Result as Ret};
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{JsNumber, Task};
use napi_derive::napi;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::channel;

use doc::{FrontEndLanguage, JsDoc};
use sg_node::{SgNode, SgRoot};

#[napi(object)]
pub struct NapiConfig {
  pub rule: serde_json::Value,
  pub constraints: Option<serde_json::Value>,
  pub language: Option<FrontEndLanguage>,
  pub utils: Option<serde_json::Value>,
}

fn parse_config(
  config: NapiConfig,
  language: FrontEndLanguage,
) -> Result<RuleWithConstraint<FrontEndLanguage>> {
  let lang = config.language.unwrap_or(language);
  let rule = SerializableRuleCore {
    language: lang,
    rule: serde_json::from_value(config.rule)?,
    constraints: config.constraints.map(serde_json::from_value).transpose()?,
    utils: config.utils.map(serde_json::from_value).transpose()?,
  };
  rule.get_matcher(&Default::default()).map_err(|e| {
    let error = Error::from(e)
      .chain()
      .map(ToString::to_string)
      .collect::<Vec<_>>();
    napi::Error::new(napi::Status::InvalidArg, error.join("\n |->"))
  })
}

macro_rules! impl_lang_mod {
    ($name: ident, $lang: ident) =>  {
      #[napi]
      pub mod $name {
        use super::*;
        use super::FrontEndLanguage::*;
        #[napi]
        pub fn parse(src: String) -> SgRoot {
          let doc = JsDoc::new(src, $lang);
          SgRoot(AstGrep::doc(doc), "anonymous".into())
        }
        #[napi]
        pub fn kind(kind_name: String) -> u16 {
          $lang.get_ts_language().id_for_node_kind(&kind_name, /* named */ true)
        }
        #[napi]
        pub fn pattern(pattern: String) -> NapiConfig {
          NapiConfig {
            rule: serde_json::json!({
              "pattern": pattern,
            }),
            constraints: None,
            language: Some($lang),
            utils: None,
          }
        }
        #[napi(
          ts_args_type = "config: FindConfig, callback: (err: null | Error, result: SgNode[]) => void",
          ts_return_type = "Promise<number>"
        )]
        pub fn find_in_files(config: FindConfig, callback: JsFunction) -> Result<AsyncTask<FindInFiles>> {
          find_in_files_impl($lang, config, callback)
        }
      }
    }
}

impl_lang_mod!(html, Html);
impl_lang_mod!(js, JavaScript);
impl_lang_mod!(jsx, JavaScript);
impl_lang_mod!(ts, TypeScript);
impl_lang_mod!(tsx, Tsx);

pub struct IterateFiles<D> {
  paths: Vec<String>,
  tsfn: D,
  producer: fn(&D, std::result::Result<ignore::DirEntry, ignore::Error>) -> Ret<bool>,
}

impl<T: 'static + Send + Sync> Task for IterateFiles<T> {
  type Output = u32;
  type JsValue = JsNumber;

  fn compute(&mut self) -> Result<Self::Output> {
    if self.paths.is_empty() {
      return Err(anyhow!("paths cannot be empty.").into());
    }
    let types = TypesBuilder::new()
      .add_defaults()
      .select("css")
      .select("html")
      .select("js")
      .select("ts")
      .build()
      .unwrap();
    let tsfn = &self.tsfn;
    let mut paths = self.paths.drain(..);
    let mut builder = WalkBuilder::new(paths.next().unwrap());
    for path in paths {
      builder.add(path);
    }
    let file_count = AtomicU32::new(0);
    let (tx, rx) = channel();
    let walker = builder.types(types).build_parallel();
    let producer = self.producer;
    walker.run(|| {
      let tx = tx.clone();
      let file_count = &file_count;
      Box::new(move |entry| match producer(tsfn, entry) {
        Ok(true) => {
          // file is sent to JS thread, increment file count
          if tx.send(()).is_ok() {
            file_count.fetch_add(1, Ordering::AcqRel);
            WalkState::Continue
          } else {
            WalkState::Quit
          }
        }
        Ok(false) => WalkState::Continue,
        Err(_) => WalkState::Skip,
      })
    });
    // Drop the last sender to stop `rx` waiting for message.
    // The program will not complete if we comment this out.
    drop(tx);
    while rx.recv().is_ok() {
      // pass
    }
    Ok(file_count.load(Ordering::Acquire))
  }
  fn resolve(&mut self, env: Env, output: Self::Output) -> Result<Self::JsValue> {
    env.create_uint32(output)
  }
}

// See https://github.com/ast-grep/ast-grep/issues/206
// NodeJS has a 1000 file limitation on sync iteration count.
// https://github.com/nodejs/node/blob/8ba54e50496a6a5c21d93133df60a9f7cb6c46ce/src/node_api.cc#L336
const THREAD_FUNC_QUEUE_SIZE: usize = 1000;

type ParseFiles = IterateFiles<ThreadsafeFunction<SgRoot, ErrorStrategy::CalleeHandled>>;

#[napi(
  ts_args_type = "paths: string[], callback: (err: null | Error, result: SgRoot) => void",
  ts_return_type = "Promise<number>"
)]
pub fn parse_files(paths: Vec<String>, callback: JsFunction) -> Result<AsyncTask<ParseFiles>> {
  let tsfn: ThreadsafeFunction<SgRoot, ErrorStrategy::CalleeHandled> =
    callback.create_threadsafe_function(THREAD_FUNC_QUEUE_SIZE, |ctx| Ok(vec![ctx.value]))?;
  Ok(AsyncTask::new(ParseFiles {
    paths,
    tsfn,
    producer: call_sg_root,
  }))
}

// returns if the entry is a file and sent to JavaScript queue
fn call_sg_root(
  tsfn: &ThreadsafeFunction<SgRoot, ErrorStrategy::CalleeHandled>,
  entry: std::result::Result<ignore::DirEntry, ignore::Error>,
) -> Ret<bool> {
  let entry = entry?;
  if !entry
    .file_type()
    .context("could not use stdin as file")?
    .is_file()
  {
    return Ok(false);
  }
  let (root, path) = get_root(entry)?;
  let sg = SgRoot(root, path);
  tsfn.call(Ok(sg), ThreadsafeFunctionCallMode::Blocking);
  Ok(true)
}

fn get_root(entry: ignore::DirEntry) -> Ret<(AstGrep<JsDoc>, String)> {
  use FrontEndLanguage::*;
  let path = entry.into_path();
  let file_content = std::fs::read_to_string(&path)?;
  let ext = path
    .extension()
    .context("check file")?
    .to_str()
    .context("to str")?;
  let lang = match ext {
    "css" | "scss" => Css,
    "html" | "htm" | "xhtml" => Html,
    "cjs" | "js" | "mjs" | "jsx" => JavaScript,
    "ts" => TypeScript,
    "tsx" => Tsx,
    _ => return Err(anyhow!("file not recognized")),
  };
  let doc = JsDoc::new(file_content, lang);
  Ok((AstGrep::doc(doc), path.to_string_lossy().into()))
}

type FindInFiles = IterateFiles<(
  ThreadsafeFunction<PinnedNodes, ErrorStrategy::CalleeHandled>,
  RuleWithConstraint<FrontEndLanguage>,
)>;

pub struct PinnedNodes(
  PinnedNodeData<JsDoc, Vec<NodeMatch<'static, JsDoc>>>,
  String,
);
unsafe impl Send for PinnedNodes {}
unsafe impl Sync for PinnedNodes {}

#[napi(object)]
pub struct FindConfig {
  pub paths: Vec<String>,
  pub matcher: NapiConfig,
}

fn find_in_files_impl(
  lang: FrontEndLanguage,
  config: FindConfig,
  callback: JsFunction,
) -> Result<AsyncTask<FindInFiles>> {
  let tsfn = callback.create_threadsafe_function(THREAD_FUNC_QUEUE_SIZE, |ctx| {
    from_pinned_data(ctx.value, ctx.env)
  })?;
  let rule = parse_config(config.matcher, lang)?;
  Ok(AsyncTask::new(FindInFiles {
    paths: config.paths,
    tsfn: (tsfn, rule),
    producer: call_sg_node,
  }))
}

// TODO: optimize
fn from_pinned_data(pinned: PinnedNodes, env: napi::Env) -> Result<Vec<Vec<SgNode>>> {
  let (root, nodes) = pinned.0.into_raw();
  let sg_root = SgRoot(AstGrep { inner: root }, pinned.1);
  let reference = SgRoot::into_reference(sg_root, env)?;
  let mut v = vec![];
  for mut node in nodes {
    let root_ref = reference.clone(env)?;
    let sg_node = SgNode {
      inner: root_ref.share_with(env, |root| {
        let r = &root.0.inner;
        node.visit_nodes(|n| unsafe { r.readopt(n) });
        Ok(node)
      })?,
    };
    v.push(sg_node);
  }
  Ok(vec![v])
}

fn call_sg_node(
  (tsfn, rule): &(
    ThreadsafeFunction<PinnedNodes, ErrorStrategy::CalleeHandled>,
    RuleWithConstraint<FrontEndLanguage>,
  ),
  entry: std::result::Result<ignore::DirEntry, ignore::Error>,
) -> Ret<bool> {
  let entry = entry?;
  if !entry
    .file_type()
    .context("could not use stdin as file")?
    .is_file()
  {
    return Ok(false);
  }
  let (root, path) = get_root(entry)?;
  let mut pinned = PinnedNodeData::new(root.inner, |r| r.root().find_all(rule).collect());
  let hits: &Vec<_> = pinned.get_data();
  if hits.is_empty() {
    return Ok(false);
  }
  let pinned = PinnedNodes(pinned, path);
  tsfn.call(Ok(pinned), ThreadsafeFunctionCallMode::Blocking);
  Ok(true)
}
