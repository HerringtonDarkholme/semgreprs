use super::Matcher;

use crate::meta_var::MetaVarEnv;
use crate::node::KindId;
use crate::{Doc, Language, Node};

use std::borrow::Cow;
use std::marker::PhantomData;

use bit_set::BitSet;
use thiserror::Error;

const TS_BUILTIN_SYM_END: KindId = 0;

#[derive(Debug, Error)]
pub enum SupertypeMatcherError {
  #[error("Kind `{0}` is invalid.")]
  InvalidKindName(String),
}

#[derive(Clone)]
pub struct SupertypeMatcher<L: Language> {
  supertype: KindId,
  subtypes: BitSet,
  lang: PhantomData<L>,
}

impl<L: Language> SupertypeMatcher<L> {
  pub fn new(supertype_kind: &str, lang: L) -> Self {
    let mut supertype_kind = lang
      .get_ts_language()
      .id_for_node_kind(supertype_kind, /*named*/ true);
    if !lang
      .get_ts_language()
      .node_kind_is_supertype(supertype_kind)
    {
      supertype_kind = 0;
    }
    Self::from_id(supertype_kind, lang)
  }

  pub fn try_new(supertype_kind: &str, lang: L) -> Result<Self, SupertypeMatcherError> {
    let s = Self::new(supertype_kind, lang);
    if s.is_invalid() {
      Err(SupertypeMatcherError::InvalidKindName(
        supertype_kind.into(),
      ))
    } else {
      Ok(s)
    }
  }

  pub fn from_id(supertype: KindId, lang: L) -> Self {
    let mut subtypes = BitSet::new();
    for kind in lang.get_ts_language().subtypes_for_supertype(supertype) {
      subtypes.insert(kind as usize);
    }
    Self {
      supertype,
      subtypes,
      lang: PhantomData,
    }
  }

  /// Whether the kind matcher contains undefined tree-sitter kind.
  pub fn is_invalid(&self) -> bool {
    self.supertype == TS_BUILTIN_SYM_END
  }
}

impl<L: Language> Matcher<L> for SupertypeMatcher<L> {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    _env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    if self.subtypes.contains(node.kind_id() as usize) {
      Some(node)
    } else {
      None
    }
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    Some(self.subtypes.clone())
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::language::Tsx;
  use crate::matcher::KindMatcher;
  use crate::{Root, StrDoc};

  fn pattern_node(s: &str) -> Root<StrDoc<Tsx>> {
    Root::new(s, Tsx)
  }
  #[test]
  fn test_supertype_match() {
    let supertype_kind = "declaration";
    let cand = pattern_node("class A { a = 123 }");
    let cand = cand.root();
    let pattern = SupertypeMatcher::new(supertype_kind, Tsx);
    assert!(
      pattern.find_node(cand.clone()).is_some(),
      "goal: {}, candidate: {}",
      supertype_kind,
      cand.to_sexp(),
    );
  }

  #[test]
  fn test_supertype_non_match() {
    let kind = "number";
    let cand = pattern_node("const a = 123;");
    let cand = cand.root();
    let kind_pattern = KindMatcher::new(kind, Tsx);
    let cand = cand.find(kind_pattern).unwrap().get_node().clone();
    let supertype_kind = "declaration";
    let pattern = SupertypeMatcher::new(supertype_kind, Tsx);

    assert!(
      pattern.find_node(cand.clone()).is_none(),
      "goal: {}, candidate: {}",
      kind,
      cand.to_sexp(),
    );
  }

  #[test]
  fn test_kind_potential_kinds() {
    let kind = "declaration";
    let matcher = SupertypeMatcher::new(kind, Tsx);
    let potential_kinds = matcher
      .potential_kinds()
      .expect("should have potential kinds");
    // tsx's declaration should have 14 potential kinds
    assert_eq!(potential_kinds.len(), 14);
  }
}
