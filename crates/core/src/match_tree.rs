use crate::meta_var::{MetaVarEnv, MetaVariable};
use crate::Language;
use crate::Node;

fn match_leaf_meta_var<'tree, L: Language>(
  goal: &Node<L>,
  candidate: Node<'tree, L>,
  env: &mut MetaVarEnv<'tree, L>,
) -> Option<Node<'tree, L>> {
  let extracted = extract_var_from_node(goal)?;
  use MetaVariable as MV;
  match extracted {
    MV::Named(name, named) => {
      if named && !candidate.is_named() {
        None
      } else {
        env.insert(name, candidate.clone())?;
        Some(candidate)
      }
    }
    MV::Anonymous(named) => {
      if named && !candidate.is_named() {
        None
      } else {
        Some(candidate)
      }
    }
    // Ellipsis will be matched in parent level
    MV::Ellipsis => {
      debug_assert!(false, "Ellipsis should be matched in parent level");
      Some(candidate)
    }
    MV::NamedEllipsis(name) => {
      env.insert(name, candidate.clone())?;
      Some(candidate)
    }
  }
}

fn try_get_ellipsis_mode(node: &Node<impl Language>) -> Result<Option<String>, ()> {
  match extract_var_from_node(node).ok_or(())? {
    MetaVariable::Ellipsis => Ok(None),
    MetaVariable::NamedEllipsis(n) => Ok(Some(n)),
    _ => Err(()),
  }
}

fn update_ellipsis_env<'t, L: Language>(
  optional_name: &Option<String>,
  mut matched: Vec<Node<'t, L>>,
  env: &mut MetaVarEnv<'t, L>,
  cand_children: impl Iterator<Item = Node<'t, L>>,
  skipped_anonymous: usize,
) {
  if let Some(name) = optional_name.as_ref() {
    matched.extend(cand_children);
    let skipped = matched.len().saturating_sub(skipped_anonymous);
    drop(matched.drain(skipped..));
    env.insert_multi(name.to_string(), matched);
  }
}

pub fn match_end_non_recursive<L: Language>(goal: &Node<L>, candidate: Node<L>) -> Option<usize> {
  let is_leaf = goal.is_named_leaf();
  if is_leaf && extract_var_from_node(goal).is_some() {
    return Some(candidate.range().end);
  }
  if goal.kind_id() != candidate.kind_id() {
    return None;
  }
  if is_leaf {
    if extract_var_from_node(goal).is_some() {
      return None;
    }
    return if goal.text() == candidate.text() {
      Some(candidate.range().end)
    } else {
      None
    };
  }
  let goal_children = goal.children();
  let cand_children = candidate.children();
  match_multi_nodes_end_non_recursive(goal_children, cand_children)
}

fn match_multi_nodes_end_non_recursive<'g, 'c, L: Language + 'g + 'c>(
  goals: impl Iterator<Item = Node<'g, L>>,
  candidates: impl Iterator<Item = Node<'c, L>>,
) -> Option<usize> {
  let mut goal_children = goals.peekable();
  let mut cand_children = candidates.peekable();
  let mut end = cand_children.peek()?.range().end;
  loop {
    let curr_node = goal_children.peek().unwrap();
    if try_get_ellipsis_mode(curr_node).is_ok() {
      goal_children.next();
      // goal has all matched
      if goal_children.peek().is_none() {
        return Some(end);
      }
      while !goal_children.peek().unwrap().is_named() {
        goal_children.next();
        if goal_children.peek().is_none() {
          return Some(end);
        }
      }
      // if next node is a Ellipsis, consume one candidate node
      if try_get_ellipsis_mode(goal_children.peek().unwrap()).is_ok() {
        cand_children.peek()?;
        continue;
      }
      loop {
        if match_end_non_recursive(
          goal_children.peek().unwrap(),
          cand_children.peek().unwrap().clone(),
        )
        .is_some()
        {
          // found match non Ellipsis,
          break;
        }
        cand_children.peek()?;
      }
    }
    end = match_end_non_recursive(
      goal_children.peek().unwrap(),
      cand_children.peek().unwrap().clone(),
    )?;
    goal_children.next();
    if goal_children.peek().is_none() {
      // all goal found, return
      return Some(end);
    }
    cand_children.next();
    cand_children.peek()?;
  }
}

pub fn match_node_non_recursive<'tree, L: Language>(
  goal: &Node<L>,
  candidate: Node<'tree, L>,
  env: &mut MetaVarEnv<'tree, L>,
) -> Option<Node<'tree, L>> {
  let is_leaf = goal.is_named_leaf();
  if is_leaf {
    if let Some(matched) = match_leaf_meta_var(goal, candidate.clone(), env) {
      return Some(matched);
    }
  }
  if goal.kind_id() != candidate.kind_id() {
    return None;
  }
  if is_leaf {
    if extract_var_from_node(goal).is_some() {
      return None;
    }
    return if goal.text() == candidate.text() {
      Some(candidate)
    } else {
      None
    };
  }
  let goal_children = goal.children();
  let cand_children = candidate.children();
  if match_nodes_non_recursive(goal_children, cand_children, env).is_some() {
    Some(candidate)
  } else {
    None
  }
}

fn match_nodes_non_recursive<'goal, 'tree, L: Language + 'tree + 'goal>(
  goals: impl Iterator<Item = Node<'goal, L>>,
  candidates: impl Iterator<Item = Node<'tree, L>>,
  env: &mut MetaVarEnv<'tree, L>,
) -> Option<()> {
  let mut goal_children = goals.peekable();
  let mut cand_children = candidates.peekable();
  cand_children.peek()?;
  loop {
    let curr_node = goal_children.peek().unwrap();
    if let Ok(optional_name) = try_get_ellipsis_mode(curr_node) {
      let mut matched = vec![];
      goal_children.next();
      // goal has all matched
      if goal_children.peek().is_none() {
        update_ellipsis_env(&optional_name, matched, env, cand_children, 0);
        return Some(());
      }
      let mut skipped_anonymous = 0;
      while !goal_children.peek().unwrap().is_named() {
        goal_children.next();
        skipped_anonymous += 1;
        if goal_children.peek().is_none() {
          update_ellipsis_env(
            &optional_name,
            matched,
            env,
            cand_children,
            skipped_anonymous,
          );
          return Some(());
        }
      }
      // if next node is a Ellipsis, consume one candidate node
      if try_get_ellipsis_mode(goal_children.peek().unwrap()).is_ok() {
        matched.push(cand_children.next().unwrap());
        cand_children.peek()?;
        update_ellipsis_env(
          &optional_name,
          matched,
          env,
          std::iter::empty(),
          skipped_anonymous,
        );
        continue;
      }
      loop {
        if match_node_non_recursive(
          goal_children.peek().unwrap(),
          cand_children.peek().unwrap().clone(),
          env,
        )
        .is_some()
        {
          // found match non Ellipsis,
          update_ellipsis_env(
            &optional_name,
            matched,
            env,
            std::iter::empty(),
            skipped_anonymous,
          );
          break;
        }
        matched.push(cand_children.next().unwrap());
        cand_children.peek()?;
      }
    }
    match_node_non_recursive(
      goal_children.peek().unwrap(),
      cand_children.peek().unwrap().clone(),
      env,
    )?;
    goal_children.next();
    if goal_children.peek().is_none() {
      // all goal found, return
      return Some(());
    }
    cand_children.next();
    cand_children.peek()?;
  }
}

pub fn does_node_match_exactly<L: Language>(goal: &Node<L>, candidate: Node<L>) -> bool {
  if goal.kind_id() != candidate.kind_id() {
    return false;
  }
  if goal.is_named_leaf() {
    return goal.text() == candidate.text();
  }
  let goal_children = goal.children();
  let cand_children = candidate.children();
  if goal_children.len() != cand_children.len() {
    return false;
  }
  goal_children
    .zip(cand_children)
    .all(|(g, c)| does_node_match_exactly(&g, c))
}

pub fn extract_var_from_node<L: Language>(goal: &Node<L>) -> Option<MetaVariable> {
  let key = goal.text();
  goal.lang().extract_meta_var(&key)
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::language::{Language, Tsx};
  use crate::ts_parser::parse as parse_base;
  use crate::{Root, StrDoc};
  use std::collections::HashMap;

  fn parse(src: &str) -> tree_sitter::Tree {
    parse_base(src, None, Tsx.get_ts_language()).unwrap()
  }
  fn find_node_recursive<'tree>(
    goal: &Node<Tsx>,
    node: Node<'tree, Tsx>,
    env: &mut MetaVarEnv<'tree, Tsx>,
  ) -> Option<Node<'tree, Tsx>> {
    match_node_non_recursive(goal, node.clone(), env).or_else(|| {
      node
        .children()
        .find_map(|sub| find_node_recursive(goal, sub, env))
    })
  }

  fn test_match(s1: &str, s2: &str) -> HashMap<String, String> {
    let goal = parse(s1);
    let goal = Node {
      inner: goal.root_node().child(0).unwrap(),
      root: &Root {
        inner: goal.clone(),
        doc: StrDoc::new(s1, Tsx),
      },
    };
    let cand = parse(s2);
    let cand = Node {
      inner: cand.root_node(),
      root: &Root {
        inner: cand.clone(),
        doc: StrDoc::new(s2, Tsx),
      },
    };
    let mut env = MetaVarEnv::new();
    let ret = find_node_recursive(&goal, cand.clone(), &mut env);
    assert!(
      ret.is_some(),
      "goal: {}, candidate: {}",
      goal.to_sexp(),
      cand.to_sexp(),
    );
    HashMap::from(env)
  }

  fn test_non_match(s1: &str, s2: &str) {
    let goal = parse(s1);
    let goal = Node {
      inner: goal.root_node().child(0).unwrap(),
      root: &Root {
        inner: goal.clone(),
        doc: StrDoc::new(s1, Tsx),
      },
    };
    let cand = parse(s2);
    let cand = Node {
      inner: cand.root_node(),
      root: &Root {
        inner: cand.clone(),
        doc: StrDoc::new(s2, Tsx),
      },
    };
    let mut env = MetaVarEnv::new();
    let ret = find_node_recursive(&goal, cand, &mut env);
    assert!(ret.is_none());
  }

  #[test]
  fn test_simple_match() {
    test_match("const a = 123", "const a=123");
    test_non_match("const a = 123", "var a = 123");
  }

  #[test]
  fn test_nested_match() {
    test_match("const a = 123", "function() {const a= 123;}");
    test_match("const a = 123", "class A { constructor() {const a= 123;}}");
    test_match(
      "const a = 123",
      "for (let a of []) while (true) { const a = 123;}",
    );
  }

  #[test]
  fn test_should_exactly_match() {
    test_match(
      "function foo() { let a = 123; }",
      "function foo() { let a = 123; }",
    );
    test_non_match(
      "function foo() { let a = 123; }",
      "function bar() { let a = 123; }",
    );
  }

  #[test]
  fn test_match_inner() {
    test_match(
      "function bar() { let a = 123; }",
      "function foo() { function bar() {let a = 123; }}",
    );
    test_non_match(
      "function foo() { let a = 123; }",
      "function foo() { function bar() {let a = 123; }}",
    );
  }

  #[test]
  fn test_single_ellipsis() {
    test_match("foo($$$)", "foo(a, b, c)");
    test_match("foo($$$)", "foo()");
  }
  #[test]
  fn test_named_ellipsis() {
    test_match("foo($$$A, c)", "foo(a, b, c)");
    test_match("foo($$$A, b, c)", "foo(a, b, c)");
    test_match("foo($$$A, a, b, c)", "foo(a, b, c)");
    test_non_match("foo($$$A, a, b, c)", "foo(b, c)");
  }

  #[test]
  fn test_leading_ellipsis() {
    test_match("foo($$$, c)", "foo(a, b, c)");
    test_match("foo($$$, b, c)", "foo(a, b, c)");
    test_match("foo($$$, a, b, c)", "foo(a, b, c)");
    test_non_match("foo($$$, a, b, c)", "foo(b, c)");
  }
  #[test]
  fn test_trailing_ellipsis() {
    test_match("foo(a, $$$)", "foo(a, b, c)");
    test_match("foo(a, b, $$$)", "foo(a, b, c)");
    // test_match("foo(a, b, c, $$$)", "foo(a, b, c)");
    test_non_match("foo(a, b, c, $$$)", "foo(b, c)");
  }

  #[test]
  fn test_meta_var_named() {
    test_match("return $A", "return 123;");
    test_match("return $_", "return 123;");
    test_non_match("return $A", "return;");
    test_non_match("return $_", "return;");
    test_match("return $$A", "return;");
    test_match("return $$_A", "return;");
  }

  #[test]
  fn test_meta_var_multiple_occurrence() {
    test_match("$A($$$)", "test(123)");
    test_match("$A($B)", "test(123)");
    test_non_match("$A($A)", "test(aaa)");
    test_non_match("$A($A)", "test(123)");
    test_non_match("$A($A, $A)", "test(123, 456)");
    test_match("$A($A)", "test(test)");
  }

  #[test]
  fn test_string() {
    test_match("'a'", "'a'");
    test_match("'abcdefg'", "'abcdefg'");
    test_match("`abcdefg`", "`abcdefg`");
    test_non_match("'a'", "'b'");
    test_non_match("'abcdefg'", "'gggggg'");
  }
}
