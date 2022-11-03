use crate::language::Language;
use crate::matcher::{Matcher, NodeMatch};
use crate::replacer::Replacer;
use crate::ts_parser::{parse, perform_edit, Edit};

use std::borrow::Cow;

/// Represents [`tree_sitter::Tree`] and owns source string
/// Note: Root is generic against [`Language`](crate::language::Language)
#[derive(Clone)]
pub struct Root<L: Language> {
  pub(crate) inner: tree_sitter::Tree,
  pub(crate) source: String,
  pub(crate) lang: L,
}

impl<L: Language> Root<L> {
  pub fn new(src: &str, lang: L) -> Self {
    Self {
      inner: parse(src, None, lang.get_ts_language()).unwrap(),
      source: src.into(),
      lang,
    }
  }
  // extract non generic implementation to reduce code size
  pub fn do_edit(&mut self, edit: Edit) {
    let input = unsafe { self.source.as_mut_vec() };
    let input_edit = perform_edit(&mut self.inner, input, &edit);
    self.inner.edit(&input_edit);
    self.inner = parse(&self.source, Some(&self.inner), self.lang.get_ts_language()).unwrap();
  }

  pub fn root(&self) -> Node<L> {
    Node {
      inner: self.inner.root_node(),
      root: self,
    }
  }
}

// the lifetime r represents root
#[derive(Clone)]
pub struct Node<'r, L: Language> {
  pub(crate) inner: tree_sitter::Node<'r>,
  pub(crate) root: &'r Root<L>,
}
pub type KindId = u16;

struct NodeWalker<'tree, L: Language> {
  cursor: tree_sitter::TreeCursor<'tree>,
  root: &'tree Root<L>,
  count: usize,
}

impl<'tree, L: Language> Iterator for NodeWalker<'tree, L> {
  type Item = Node<'tree, L>;
  fn next(&mut self) -> Option<Self::Item> {
    if self.count == 0 {
      return None;
    }
    let ret = Some(Node {
      inner: self.cursor.node(),
      root: self.root,
    });
    self.cursor.goto_next_sibling();
    self.count -= 1;
    ret
  }
}

impl<'tree, L: Language> ExactSizeIterator for NodeWalker<'tree, L> {
  fn len(&self) -> usize {
    self.count
  }
}

pub struct Dfs<'tree, L: Language> {
  cursor: tree_sitter::TreeCursor<'tree>,
  root: &'tree Root<L>,
  // record the starting node, if we return back to starting point
  // we should terminate the dfs.
  start_id: Option<usize>,
}

impl<'tree, L: Language> Dfs<'tree, L> {
  fn new(node: &Node<'tree, L>) -> Self {
    Self {
      cursor: node.inner.walk(),
      root: node.root,
      start_id: Some(node.inner.id()),
    }
  }
}

impl<'tree, L: Language> Iterator for Dfs<'tree, L> {
  type Item = Node<'tree, L>;
  fn next(&mut self) -> Option<Self::Item> {
    let start = self.start_id?;
    let cursor = &mut self.cursor;
    let inner = cursor.node();
    let ret = Some(Node {
      inner,
      root: self.root,
    });
    if cursor.goto_first_child() {
      return ret;
    }
    while cursor.node().id() != start {
      if cursor.goto_next_sibling() {
        return ret;
      }
      cursor.goto_parent();
    }
    self.start_id = None;
    ret
  }
}

// internal API
impl<'r, L: Language> Node<'r, L> {
  pub fn is_leaf(&self) -> bool {
    self.inner.child_count() == 0
  }
  pub fn kind(&self) -> Cow<str> {
    self.inner.kind()
  }
  pub fn kind_id(&self) -> KindId {
    self.inner.kind_id()
  }

  pub fn is_named(&self) -> bool {
    self.inner.is_named()
  }

  pub fn range(&self) -> std::ops::Range<usize> {
    (self.inner.start_byte() as usize)..(self.inner.end_byte() as usize)
  }
  pub fn start_pos(&self) -> (usize, usize) {
    let pos = self.inner.start_position();
    (pos.row() as usize, pos.column() as usize)
  }
  pub fn end_pos(&self) -> (usize, usize) {
    let pos = self.inner.end_position();
    (pos.row() as usize, pos.column() as usize)
  }
  pub fn text(&self) -> Cow<'r, str> {
    self
      .inner
      .utf8_text(self.root.source.as_bytes())
      .expect("invalid source text encoding")
  }
  pub fn to_sexp(&self) -> Cow<'_, str> {
    self.inner.to_sexp()
  }

  pub fn display_context(&self, context_lines: usize) -> DisplayContext<'r> {
    let bytes = self.root.source.as_bytes();
    let start = self.inner.start_byte() as usize;
    let end = self.inner.end_byte() as usize;
    let (mut leading, mut trailing) = (start, end);
    let mut lines_before = context_lines + 1;
    while leading > 0 {
      if bytes[leading - 1] == b'\n' {
        lines_before -= 1;
        if lines_before == 0 {
          break;
        }
      }
      leading -= 1;
    }
    // tree-sitter will append line ending to source so trailing can be out of bound
    trailing = trailing.min(bytes.len() - 1);
    let mut lines_after = context_lines + 1;
    while trailing < bytes.len() - 1 {
      if bytes[trailing + 1] == b'\n' {
        lines_after -= 1;
        if lines_after == 0 {
          break;
        }
      }
      trailing += 1;
    }
    DisplayContext {
      matched: self.text(),
      leading: &self.root.source[leading..start],
      trailing: &self.root.source[end..=trailing],
      start_line: self.inner.start_position().row() as usize + 1,
    }
  }

  pub fn lang(&self) -> &L {
    &self.root.lang
  }
}

/**
 * Corresponds to inside/has/precedes/follows
 */
impl<'r, L: Language> Node<'r, L> {
  pub fn matches<M: Matcher<L>>(&self, m: M) -> bool {
    m.match_node(self.clone()).is_some()
  }

  pub fn inside<M: Matcher<L>>(&self, m: M) -> bool {
    self.ancestors().find_map(|n| m.match_node(n)).is_some()
  }

  pub fn has<M: Matcher<L>>(&self, m: M) -> bool {
    self.dfs().skip(1).find_map(|n| m.match_node(n)).is_some()
  }

  pub fn precedes<M: Matcher<L>>(&self, m: M) -> bool {
    self.next_all().find_map(|n| m.match_node(n)).is_some()
  }

  pub fn follows<M: Matcher<L>>(&self, m: M) -> bool {
    self.prev_all().find_map(|n| m.match_node(n)).is_some()
  }
}

pub struct DisplayContext<'r> {
  /// content for the matched node
  pub matched: Cow<'r, str>,
  /// content before the matched node
  pub leading: &'r str,
  /// content after the matched node
  pub trailing: &'r str,
  /// start line of the matched node
  pub start_line: usize,
}

/// tree traversal API
impl<'r, L: Language> Node<'r, L> {
  pub fn children<'s>(&'s self) -> impl ExactSizeIterator<Item = Node<'r, L>> + 's {
    let mut cursor = self.inner.walk();
    cursor.goto_first_child();
    NodeWalker {
      cursor,
      root: self.root,
      count: self.inner.child_count() as usize,
    }
  }

  pub fn dfs<'s>(&'s self) -> Dfs<'r, L> {
    Dfs::new(self)
  }

  #[must_use]
  pub fn find<M: Matcher<L>>(&self, pat: M) -> Option<NodeMatch<'r, L>> {
    pat.find_node(self.clone())
  }

  pub fn find_all<M: Matcher<L>>(&self, pat: M) -> impl Iterator<Item = NodeMatch<'r, L>> {
    pat.find_all_nodes(self.clone())
  }

  pub fn field(&self, name: &str) -> Option<Self> {
    let mut cursor = self.inner.walk();
    let inner = self
      .inner
      .children_by_field_name(name, &mut cursor)
      .next()?;
    Some(Node {
      inner,
      root: self.root,
    })
  }

  pub fn field_children(&self, name: &str) -> impl Iterator<Item = Node<'r, L>> {
    let field_id = self
      .root
      .lang
      .get_ts_language()
      .field_id_for_name(name)
      .unwrap_or(0);
    let root = self.root;
    let mut cursor = self.inner.walk();
    cursor.goto_first_child();
    let mut done = false;
    std::iter::from_fn(move || {
      if done {
        return None;
      }
      while cursor.field_id() != Some(field_id) {
        if !cursor.goto_next_sibling() {
          return None;
        }
      }
      let inner = cursor.node();
      if !cursor.goto_next_sibling() {
        done = true;
      }
      Some(Node { inner, root })
    })
  }

  #[must_use]
  pub fn parent(&self) -> Option<Self> {
    let inner = self.inner.parent()?;
    Some(Node {
      inner,
      root: self.root,
    })
  }

  #[must_use]
  pub fn child(&self, nth: usize) -> Option<Self> {
    // TODO: support usize
    let inner = self.inner.child(nth as u32)?;
    Some(Node {
      inner,
      root: self.root,
    })
  }

  pub fn ancestors(&self) -> impl Iterator<Item = Node<'r, L>> + '_ {
    let mut parent = self.inner.parent();
    std::iter::from_fn(move || {
      let inner = parent.clone()?;
      let ret = Some(Node {
        inner: inner.clone(),
        root: self.root,
      });
      parent = inner.parent();
      ret
    })
  }
  #[must_use]
  pub fn next(&self) -> Option<Self> {
    let inner = self.inner.next_sibling()?;
    Some(Node {
      inner,
      root: self.root,
    })
  }
  pub fn next_all(&self) -> impl Iterator<Item = Node<'r, L>> + '_ {
    let mut node = self.clone();
    std::iter::from_fn(move || {
      node.next().map(|n| {
        node = n.clone();
        n
      })
    })
  }
  #[must_use]
  pub fn prev(&self) -> Option<Node<'r, L>> {
    let inner = self.inner.prev_sibling()?;
    Some(Node {
      inner,
      root: self.root,
    })
  }

  // TODO: use cursor to optimize clone.
  // investigate why tree_sitter cursor cannot goto next_sibling
  pub fn prev_all(&self) -> impl Iterator<Item = Node<'r, L>> + '_ {
    let mut node = self.clone();
    std::iter::from_fn(move || {
      node.prev().map(|n| {
        node = n.clone();
        n
      })
    })
  }
}

/// Tree manipulation API
impl<'r, L: Language> Node<'r, L> {
  fn make_edit<R: Replacer<L>>(&self, matched: NodeMatch<L>, replacer: &R) -> Edit {
    let lang = self.root.lang.clone();
    let env = matched.get_env();
    let range = matched.range();
    let position = range.start;
    let deleted_length = range.len();
    let inserted_text = replacer.generate_replacement(env, lang);
    Edit {
      position,
      deleted_length,
      inserted_text,
    }
  }

  pub fn replace<M: Matcher<L>, R: Replacer<L>>(&self, matcher: M, replacer: R) -> Option<Edit> {
    let matched = matcher.find_node(self.clone())?;
    Some(self.make_edit(matched, &replacer))
  }

  pub fn replace_all<M: Matcher<L>, R: Replacer<L>>(&self, matcher: M, replacer: R) -> Vec<Edit> {
    self
      .find_all(matcher)
      .map(|matched| self.make_edit(matched, &replacer))
      .collect()
  }

  pub fn after(&self) {
    todo!()
  }
  pub fn before(&self) {
    todo!()
  }
  pub fn append(&self) {
    todo!()
  }
  pub fn prepend(&self) {
    todo!()
  }
  pub fn empty(&self) {
    todo!()
  }
  pub fn remove(&self) {
    todo!()
  }
}

#[cfg(test)]
mod test {
  use crate::language::{Language, Tsx};
  #[test]
  fn test_is_leaf() {
    let root = Tsx.ast_grep("let a = 123");
    let node = root.root();
    assert!(!node.is_leaf());
  }

  #[test]
  fn test_children() {
    let root = Tsx.ast_grep("let a = 123");
    let node = root.root();
    let children: Vec<_> = node.children().collect();
    assert_eq!(children.len(), 1);
    let texts: Vec<_> = children[0]
      .children()
      .map(|c| c.text().to_string())
      .collect();
    assert_eq!(texts, vec!["let", "a = 123"]);
  }

  #[test]
  fn test_display_context() {
    // display context should not panic
    let s = "i()";
    assert_eq!(s.len(), 3);
    let root = Tsx.ast_grep(s);
    let node = root.root();
    assert_eq!(node.display_context(0).trailing.len(), 0);
  }
}
