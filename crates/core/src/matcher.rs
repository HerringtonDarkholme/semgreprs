//! This module defines the core `Matcher` trait in ast-grep.
//!
//! `Matcher` has three notable implementations in this module:
//! * Pattern: matches against a tree-sitter node based on its tree structure.
//! * KindMatcher: matches a node based on its `kind`
//! * RegexMatcher: matches a node based on its textual content using regex.

mod kind;
mod node_match;
mod pattern;
mod supertype;
#[cfg(feature = "regex")]
mod text;

use crate::meta_var::MetaVarEnv;
use crate::traversal::Pre;
use crate::{Doc, Language, Node};

use bit_set::BitSet;
use std::borrow::Cow;

pub use kind::{kind_utils, KindMatcher, KindMatcherError};
pub use node_match::NodeMatch;
pub use pattern::{Pattern, PatternError, PatternNode};
pub use supertype::{SupertypeMatcher, SupertypeMatcherError};
#[cfg(feature = "regex")]
pub use text::{RegexMatcher, RegexMatcherError};

/// `Matcher` defines whether a tree-sitter node matches certain pattern,
/// and update the matched meta-variable values in `MetaVarEnv`.
/// N.B. At least one positive term is required for matching
pub trait Matcher<L: Language> {
  /// Returns the node why the input is matched or None if not matched.
  /// The return value is usually input node itself, but it can be different node.
  /// For example `Has` matcher can return the child or descendant node.
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    _node: Node<'tree, D>,
    _env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>>;

  /// Returns a bitset for all possible target node kind ids.
  /// Returns None if the matcher needs to try against all node kind.
  fn potential_kinds(&self) -> Option<BitSet> {
    None
  }

  /// get_match_len will skip trailing anonymous child node to exclude punctuation.
  // This is not included in NodeMatch since it is only used in replace
  fn get_match_len<D: Doc<Lang = L>>(&self, _node: Node<D>) -> Option<usize> {
    None
  }

  fn match_node<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
  ) -> Option<NodeMatch<'tree, D>> {
    // in future we might need to customize initial MetaVarEnv
    let mut env = Cow::Owned(MetaVarEnv::new());
    let node = self.match_node_with_env(node, &mut env)?;
    Some(NodeMatch::new(node, env.into_owned()))
  }

  fn find_node<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
  ) -> Option<NodeMatch<'tree, D>> {
    for n in node.dfs() {
      if let Some(ret) = self.match_node(n.clone()) {
        return Some(ret);
      }
    }
    None
  }
}

impl<L: Language> Matcher<L> for str {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    let pattern = Pattern::str(self, node.lang().clone());
    pattern.match_node_with_env(node, env)
  }

  fn get_match_len<D: Doc<Lang = L>>(&self, node: Node<D>) -> Option<usize> {
    let pattern = Pattern::str(self, node.lang().clone());
    pattern.get_match_len(node)
  }
}

impl<L, T> Matcher<L> for &T
where
  L: Language,
  T: Matcher<L> + ?Sized,
{
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    (**self).match_node_with_env(node, env)
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    (**self).potential_kinds()
  }

  fn match_node<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
  ) -> Option<NodeMatch<'tree, D>> {
    (**self).match_node(node)
  }

  fn find_node<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
  ) -> Option<NodeMatch<'tree, D>> {
    (**self).find_node(node)
  }

  fn get_match_len<D: Doc<Lang = L>>(&self, node: Node<D>) -> Option<usize> {
    (**self).get_match_len(node)
  }
}

pub struct FindAllNodes<'tree, D: Doc, M: Matcher<D::Lang>> {
  // using dfs is not universally correct, say, when we want replace nested matches
  // e.g. for pattern Some($A) with replacement $A, Some(Some(1)) will cause panic
  dfs: Pre<'tree, D>,
  matcher: M,
}

impl<'tree, D: Doc, M: Matcher<D::Lang>> FindAllNodes<'tree, D, M> {
  pub fn new(matcher: M, node: Node<'tree, D>) -> Self {
    Self {
      dfs: node.dfs(),
      matcher,
    }
  }
}

impl<'tree, D: Doc, M: Matcher<D::Lang>> Iterator for FindAllNodes<'tree, D, M> {
  type Item = NodeMatch<'tree, D>;
  fn next(&mut self) -> Option<Self::Item> {
    let kinds = self.matcher.potential_kinds();
    for cand in self.dfs.by_ref() {
      if let Some(k) = &kinds {
        if !k.contains(cand.kind_id().into()) {
          continue;
        }
      }
      if let Some(matched) = self.matcher.match_node(cand) {
        return Some(matched);
      }
    }
    None
  }
}

pub struct MatchAll;
impl<L: Language> Matcher<L> for MatchAll {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    _env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    Some(node)
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    // return None to match anything
    None
  }
}

pub struct MatchNone;
impl<L: Language> Matcher<L> for MatchNone {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    _node: Node<'tree, D>,
    _env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    None
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    // matches nothing
    Some(BitSet::new())
  }
}
