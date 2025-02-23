use super::Matcher;

use crate::meta_var::MetaVarEnv;
use crate::node::KindId;
use crate::{Doc, Language, Node};

use std::borrow::Cow;
use std::marker::PhantomData;

use bit_set::BitSet;
use thiserror::Error;

// 0 is symbol_end for not found, 65535 is builtin symbol ERROR
// see https://tree-sitter.docsforge.com/master/api/#TREE_SITTER_MIN_COMPATIBLE_LANGUAGE_VERSION
// and https://tree-sitter.docsforge.com/master/api/ts_language_symbol_for_name/
const TS_BUILTIN_SYM_END: KindId = 0;
const TS_BUILTIN_SYM_ERROR: KindId = 65535;

#[derive(Debug, Error)]
pub enum KindMatcherError {
  #[error("Kind `{0}` is invalid.")]
  InvalidKindName(String),
}

#[derive(Clone)]
pub struct KindMatcher<L: Language> {
  kind: KindId,
  lang: PhantomData<L>,
}

impl<L: Language> KindMatcher<L> {
  pub fn new(node_kind: &str, lang: L) -> Self {
    let mut kind = lang
      .get_ts_language()
      .id_for_node_kind(node_kind, /*named*/ true);
    if lang.get_ts_language().node_kind_is_supertype(kind) {
      kind = 0;
    }
    Self {
      kind,
      lang: PhantomData,
    }
  }

  pub fn try_new(node_kind: &str, lang: L) -> Result<Self, KindMatcherError> {
    let s = Self::new(node_kind, lang);
    if s.is_invalid() {
      Err(KindMatcherError::InvalidKindName(node_kind.into()))
    } else {
      Ok(s)
    }
  }

  pub fn from_id(kind: KindId) -> Self {
    Self {
      kind,
      lang: PhantomData,
    }
  }

  /// Whether the kind matcher contains undefined tree-sitter kind.
  pub fn is_invalid(&self) -> bool {
    self.kind == TS_BUILTIN_SYM_END
  }

  /// Construct a matcher that only matches ERROR
  pub fn error_matcher() -> Self {
    Self::from_id(TS_BUILTIN_SYM_ERROR)
  }
}

pub mod kind_utils {
  use super::*;

  /// Whether the kind will match parsing error occurred in the source code.
  /// for example, we can use `kind: ERROR` in YAML to find invalid syntax in source.
  /// the name `is_error` implies the matcher itself is error.
  /// But here the matcher itself is valid and it is what it matches is error.
  pub fn is_error_kind(kind: KindId) -> bool {
    kind == TS_BUILTIN_SYM_ERROR
  }

  pub fn are_kinds_matching(goal: KindId, candidate: KindId) -> bool {
    goal == candidate || is_error_kind(goal)
  }
}

impl<L: Language> Matcher<L> for KindMatcher<L> {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    _env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    if node.kind_id() == self.kind {
      Some(node)
    } else {
      None
    }
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    let mut set = BitSet::new();
    set.insert(self.kind.into());
    Some(set)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::language::Tsx;
  use crate::{Root, StrDoc};

  fn pattern_node(s: &str) -> Root<StrDoc<Tsx>> {
    Root::new(s, Tsx)
  }
  #[test]
  fn test_kind_match() {
    let kind = "public_field_definition";
    let cand = pattern_node("class A { a = 123 }");
    let cand = cand.root();
    let pattern = KindMatcher::new(kind, Tsx);
    assert!(
      pattern.find_node(cand.clone()).is_some(),
      "goal: {}, candidate: {}",
      kind,
      cand.to_sexp(),
    );
  }

  #[test]
  fn test_kind_non_match() {
    let kind = "field_definition";
    let cand = pattern_node("const a = 123");
    let cand = cand.root();
    let pattern = KindMatcher::new(kind, Tsx);
    assert!(
      pattern.find_node(cand.clone()).is_none(),
      "goal: {}, candidate: {}",
      kind,
      cand.to_sexp(),
    );
  }

  #[test]
  fn test_kind_potential_kinds() {
    let kind = "field_definition";
    let matcher = KindMatcher::new(kind, Tsx);
    let potential_kinds = matcher
      .potential_kinds()
      .expect("should have potential kinds");
    // should has exactly one potential kind
    assert_eq!(potential_kinds.len(), 1);
  }
}
