mod deserialize_env;
mod nth_child;
mod range;
pub mod referent_rule;
mod relational_rule;
mod stop_by;

pub use deserialize_env::DeserializeEnv;
pub use relational_rule::Relation;
pub use stop_by::StopBy;

use crate::maybe::Maybe;
use nth_child::{NthChild, NthChildError, SerializableNthChild};
use range::{RangeMatcher, RangeMatcherError, SerializableRange};
use referent_rule::{ReferentRule, ReferentRuleError};
use relational_rule::{Follows, Has, Inside, Precedes};

use ast_grep_core::language::Language;
use ast_grep_core::matcher::{
  KindMatcher, KindMatcherError, RegexMatcher, RegexMatcherError, SupertypeMatcher,
};
use ast_grep_core::meta_var::MetaVarEnv;
use ast_grep_core::ops as o;
use ast_grep_core::{Doc, MatchStrictness, Matcher, Node, Pattern, PatternError};

use bit_set::BitSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use thiserror::Error;

/// A rule object to find matching AST nodes. We have three categories of rules in ast-grep.
///
/// * Atomic: the most basic rule to match AST. We have two variants: Pattern and Kind.
///
/// * Relational: filter matched target according to their position relative to other nodes.
///
/// * Composite: use logic operation all/any/not to compose the above rules to larger rules.
///
/// Every rule has it's unique name so we can combine several rules in one object.
#[derive(Serialize, Deserialize, Clone, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SerializableRule {
  // avoid embedding AtomicRule/RelationalRule/CompositeRule with flatten here for better error message

  // atomic
  /// A pattern string or a pattern object.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub pattern: Maybe<PatternStyle>,
  /// The kind name of the node to match. You can look up code's kind names in playground.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub kind: Maybe<String>,
  /// A Rust regular expression to match the node's text. https://docs.rs/regex/latest/regex/#syntax
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub regex: Maybe<String>,
  /// `nth_child` accepts number, string or object.
  /// It specifies the position in nodes' sibling list.
  #[serde(default, skip_serializing_if = "Maybe::is_absent", rename = "nthChild")]
  pub nth_child: Maybe<SerializableNthChild>,
  /// `range` accepts a range object.
  /// the target node must exactly appear in the range.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub range: Maybe<SerializableRange>,
  /// The supertype name of the node to match.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub supertype: Maybe<String>,

  // relational
  /// `inside` accepts a relational rule object.
  /// the target node must appear inside of another node matching the `inside` sub-rule.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub inside: Maybe<Box<Relation>>,
  /// `has` accepts a relational rule object.
  /// the target node must has a descendant node matching the `has` sub-rule.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub has: Maybe<Box<Relation>>,
  /// `precedes` accepts a relational rule object.
  /// the target node must appear before another node matching the `precedes` sub-rule.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub precedes: Maybe<Box<Relation>>,
  /// `follows` accepts a relational rule object.
  /// the target node must appear after another node matching the `follows` sub-rule.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub follows: Maybe<Box<Relation>>,
  // composite
  /// A list of sub rules and matches a node if all of sub rules match.
  /// The meta variables of the matched node contain all variables from the sub-rules.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub all: Maybe<Vec<SerializableRule>>,
  /// A list of sub rules and matches a node if any of sub rules match.
  /// The meta variables of the matched node only contain those of the matched sub-rule.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub any: Maybe<Vec<SerializableRule>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  /// A single sub-rule and matches a node if the sub rule does not match.
  pub not: Maybe<Box<SerializableRule>>,
  /// A utility rule id and matches a node if the utility rule matches.
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub matches: Maybe<String>,
}

struct Categorized {
  pub atomic: AtomicRule,
  pub relational: RelationalRule,
  pub composite: CompositeRule,
}

impl SerializableRule {
  fn categorized(self) -> Categorized {
    Categorized {
      atomic: AtomicRule {
        pattern: self.pattern.into(),
        kind: self.kind.into(),
        regex: self.regex.into(),
        nth_child: self.nth_child.into(),
        range: self.range.into(),
        supertype: self.supertype.into(),
      },
      relational: RelationalRule {
        inside: self.inside.into(),
        has: self.has.into(),
        precedes: self.precedes.into(),
        follows: self.follows.into(),
      },
      composite: CompositeRule {
        all: self.all.into(),
        any: self.any.into(),
        not: self.not.into(),
        matches: self.matches.into(),
      },
    }
  }
}

pub struct AtomicRule {
  pub pattern: Option<PatternStyle>,
  pub kind: Option<String>,
  pub regex: Option<String>,
  pub nth_child: Option<SerializableNthChild>,
  pub range: Option<SerializableRange>,
  pub supertype: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Strictness {
  /// all nodes are matched
  Cst,
  /// all nodes except source trivial nodes are matched.
  Smart,
  /// only ast nodes are matched
  Ast,
  /// ast-nodes excluding comments are matched
  Relaxed,
  /// ast-nodes excluding comments, without text
  Signature,
}

impl From<MatchStrictness> for Strictness {
  fn from(value: MatchStrictness) -> Self {
    use MatchStrictness as M;
    use Strictness as S;
    match value {
      M::Cst => S::Cst,
      M::Smart => S::Smart,
      M::Ast => S::Ast,
      M::Relaxed => S::Relaxed,
      M::Signature => S::Signature,
    }
  }
}

impl From<Strictness> for MatchStrictness {
  fn from(value: Strictness) -> Self {
    use MatchStrictness as M;
    use Strictness as S;
    match value {
      S::Cst => M::Cst,
      S::Smart => M::Smart,
      S::Ast => M::Ast,
      S::Relaxed => M::Relaxed,
      S::Signature => M::Signature,
    }
  }
}

/// A String pattern will match one single AST node according to pattern syntax.
/// Or an object with field `context`, `selector` and optionally `strictness`.
#[derive(Serialize, Deserialize, Clone, JsonSchema)]
#[serde(untagged)]
pub enum PatternStyle {
  Str(String),
  Contextual {
    /// The surrounding code that helps to resolve any ambiguity in the syntax.
    context: String,
    /// The sub-syntax node kind that is the actual matcher of the pattern.
    selector: Option<String>,
    /// Strictness of the pattern. More strict pattern matches fewer nodes.
    strictness: Option<Strictness>,
  },
}

pub struct RelationalRule {
  pub inside: Option<Box<Relation>>,
  pub has: Option<Box<Relation>>,
  pub precedes: Option<Box<Relation>>,
  pub follows: Option<Box<Relation>>,
}

pub struct CompositeRule {
  pub all: Option<Vec<SerializableRule>>,
  pub any: Option<Vec<SerializableRule>>,
  pub not: Option<Box<SerializableRule>>,
  pub matches: Option<String>,
}

pub enum Rule<L: Language> {
  // atomic
  Pattern(Pattern<L>),
  Kind(KindMatcher<L>),
  Supertype(SupertypeMatcher<L>),
  Regex(RegexMatcher<L>),
  NthChild(NthChild<L>),
  Range(RangeMatcher<L>),
  // relational
  Inside(Box<Inside<L>>),
  Has(Box<Has<L>>),
  Precedes(Box<Precedes<L>>),
  Follows(Box<Follows<L>>),
  // composite
  All(o::All<L, Rule<L>>),
  Any(o::Any<L, Rule<L>>),
  Not(Box<o::Not<L, Rule<L>>>),
  Matches(ReferentRule<L>),
}
impl<L: Language> Rule<L> {
  pub fn is_atomic(&self) -> bool {
    use Rule::*;
    matches!(
      self,
      Pattern(_) | Kind(_) | Regex(_) | NthChild(_) | Range(_)
    )
  }
  pub fn is_relational(&self) -> bool {
    use Rule::*;
    matches!(self, Inside(_) | Has(_) | Precedes(_) | Follows(_))
  }

  pub fn is_composite(&self) -> bool {
    use Rule::*;
    matches!(self, All(_) | Any(_) | Not(_) | Matches(_))
  }

  /// Check if it has a cyclic referent rule with the id.
  pub(crate) fn check_cyclic(&self, id: &str) -> bool {
    match self {
      Rule::All(all) => all.inner().iter().any(|r| r.check_cyclic(id)),
      Rule::Any(any) => any.inner().iter().any(|r| r.check_cyclic(id)),
      Rule::Not(not) => not.inner().check_cyclic(id),
      Rule::Matches(m) => m.rule_id == id,
      rule => {
        debug_assert!(!rule.is_composite());
        false
      }
    }
  }

  pub fn defined_vars(&self) -> HashSet<&str> {
    match self {
      Rule::Pattern(p) => p.defined_vars(),
      Rule::Kind(_) => HashSet::new(),
      Rule::Supertype(_) => HashSet::new(),
      Rule::Regex(_) => HashSet::new(),
      Rule::NthChild(n) => n.defined_vars(),
      Rule::Range(_) => HashSet::new(),
      Rule::Has(c) => c.defined_vars(),
      Rule::Inside(p) => p.defined_vars(),
      Rule::Precedes(f) => f.defined_vars(),
      Rule::Follows(f) => f.defined_vars(),
      Rule::All(sub) => sub.inner().iter().flat_map(|r| r.defined_vars()).collect(),
      Rule::Any(sub) => sub.inner().iter().flat_map(|r| r.defined_vars()).collect(),
      Rule::Not(sub) => sub.inner().defined_vars(),
      // TODO: this is not correct, we are collecting util vars else where
      Rule::Matches(_r) => HashSet::new(),
    }
  }

  /// check if util rules used are defined
  pub fn verify_util(&self) -> Result<(), RuleSerializeError> {
    match self {
      Rule::Pattern(_) => Ok(()),
      Rule::Kind(_) => Ok(()),
      Rule::Supertype(_) => Ok(()),
      Rule::Regex(_) => Ok(()),
      Rule::NthChild(n) => n.verify_util(),
      Rule::Range(_) => Ok(()),
      Rule::Has(c) => c.verify_util(),
      Rule::Inside(p) => p.verify_util(),
      Rule::Precedes(f) => f.verify_util(),
      Rule::Follows(f) => f.verify_util(),
      Rule::All(sub) => sub.inner().iter().try_for_each(|r| r.verify_util()),
      Rule::Any(sub) => sub.inner().iter().try_for_each(|r| r.verify_util()),
      Rule::Not(sub) => sub.inner().verify_util(),
      Rule::Matches(r) => Ok(r.verify_util()?),
    }
  }
}

impl<L: Language> Matcher<L> for Rule<L> {
  fn match_node_with_env<'tree, D: Doc<Lang = L>>(
    &self,
    node: Node<'tree, D>,
    env: &mut Cow<MetaVarEnv<'tree, D>>,
  ) -> Option<Node<'tree, D>> {
    use Rule::*;
    match self {
      // atomic
      Pattern(pattern) => pattern.match_node_with_env(node, env),
      Kind(kind) => kind.match_node_with_env(node, env),
      Supertype(supertype) => supertype.match_node_with_env(node, env),
      Regex(regex) => regex.match_node_with_env(node, env),
      NthChild(nth_child) => nth_child.match_node_with_env(node, env),
      Range(range) => range.match_node_with_env(node, env),
      // relational
      Inside(parent) => match_and_add_label(&**parent, node, env),
      Has(child) => match_and_add_label(&**child, node, env),
      Precedes(latter) => match_and_add_label(&**latter, node, env),
      Follows(former) => match_and_add_label(&**former, node, env),
      // composite
      All(all) => all.match_node_with_env(node, env),
      Any(any) => any.match_node_with_env(node, env),
      Not(not) => not.match_node_with_env(node, env),
      Matches(rule) => rule.match_node_with_env(node, env),
    }
  }

  fn potential_kinds(&self) -> Option<BitSet> {
    use Rule::*;
    match self {
      // atomic
      Pattern(pattern) => pattern.potential_kinds(),
      Kind(kind) => kind.potential_kinds(),
      Supertype(supertype) => supertype.potential_kinds(),
      Regex(regex) => regex.potential_kinds(),
      NthChild(nth_child) => nth_child.potential_kinds(),
      Range(range) => range.potential_kinds(),
      // relational
      Inside(parent) => parent.potential_kinds(),
      Has(child) => child.potential_kinds(),
      Precedes(latter) => latter.potential_kinds(),
      Follows(former) => former.potential_kinds(),
      // composite
      All(all) => all.potential_kinds(),
      Any(any) => any.potential_kinds(),
      Not(not) => not.potential_kinds(),
      Matches(rule) => rule.potential_kinds(),
    }
  }
}

/// Rule matches nothing by default.
/// In Math jargon, Rule is vacuously false.
impl<L: Language> Default for Rule<L> {
  fn default() -> Self {
    Self::Any(o::Any::new(std::iter::empty()))
  }
}

fn match_and_add_label<'tree, D: Doc, M: Matcher<D::Lang>>(
  inner: &M,
  node: Node<'tree, D>,
  env: &mut Cow<MetaVarEnv<'tree, D>>,
) -> Option<Node<'tree, D>> {
  let matched = inner.match_node_with_env(node, env)?;
  env.to_mut().add_label("secondary", matched.clone());
  Some(matched)
}

#[derive(Debug, Error)]
pub enum RuleSerializeError {
  #[error("Rule must have one positive matcher.")]
  MissPositiveMatcher,
  #[error("Rule contains invalid kind matcher.")]
  InvalidKind(#[from] KindMatcherError),
  #[error("Rule contains invalid pattern matcher.")]
  InvalidPattern(#[from] PatternError),
  #[error("Rule contains invalid nthChild.")]
  NthChild(#[from] NthChildError),
  #[error("Rule contains invalid regex matcher.")]
  WrongRegex(#[from] RegexMatcherError),
  #[error("Rule contains invalid matches reference.")]
  MatchesReference(#[from] ReferentRuleError),
  #[error("Rule contains invalid range matcher.")]
  InvalidRange(#[from] RangeMatcherError),
  #[error("field is only supported in has/inside.")]
  FieldNotSupported,
  #[error("Relational rule contains invalid field {0}.")]
  InvalidField(String),
}

// TODO: implement positive/non positive
pub fn deserialize_rule<L: Language>(
  serialized: SerializableRule,
  env: &DeserializeEnv<L>,
) -> Result<Rule<L>, RuleSerializeError> {
  let mut rules = Vec::with_capacity(1);
  use Rule as R;
  let categorized = serialized.categorized();
  // ATTENTION, relational_rule should always come at last
  // after target node is decided by atomic/composite rule
  deserialze_atomic_rule(categorized.atomic, &mut rules, env)?;
  deserialze_composite_rule(categorized.composite, &mut rules, env)?;
  deserialize_relational_rule(categorized.relational, &mut rules, env)?;

  if rules.is_empty() {
    Err(RuleSerializeError::MissPositiveMatcher)
  } else if rules.len() == 1 {
    Ok(rules.pop().expect("should not be empty"))
  } else {
    Ok(R::All(o::All::new(rules)))
  }
}

fn deserialze_composite_rule<L: Language>(
  composite: CompositeRule,
  rules: &mut Vec<Rule<L>>,
  env: &DeserializeEnv<L>,
) -> Result<(), RuleSerializeError> {
  use Rule as R;
  let convert_rules = |rules: Vec<SerializableRule>| -> Result<_, RuleSerializeError> {
    let mut inner = Vec::with_capacity(rules.len());
    for rule in rules {
      inner.push(deserialize_rule(rule, env)?);
    }
    Ok(inner)
  };
  if let Some(all) = composite.all {
    rules.push(R::All(o::All::new(convert_rules(all)?)));
  }
  if let Some(any) = composite.any {
    rules.push(R::Any(o::Any::new(convert_rules(any)?)));
  }
  if let Some(not) = composite.not {
    let not = o::Not::new(deserialize_rule(*not, env)?);
    rules.push(R::Not(Box::new(not)));
  }
  if let Some(id) = composite.matches {
    let matches = ReferentRule::try_new(id, &env.registration)?;
    rules.push(R::Matches(matches));
  }
  Ok(())
}

fn deserialize_relational_rule<L: Language>(
  relational: RelationalRule,
  rules: &mut Vec<Rule<L>>,
  env: &DeserializeEnv<L>,
) -> Result<(), RuleSerializeError> {
  use Rule as R;
  // relational
  if let Some(inside) = relational.inside {
    rules.push(R::Inside(Box::new(Inside::try_new(*inside, env)?)));
  }
  if let Some(has) = relational.has {
    rules.push(R::Has(Box::new(Has::try_new(*has, env)?)));
  }
  if let Some(precedes) = relational.precedes {
    rules.push(R::Precedes(Box::new(Precedes::try_new(*precedes, env)?)));
  }
  if let Some(follows) = relational.follows {
    rules.push(R::Follows(Box::new(Follows::try_new(*follows, env)?)));
  }
  Ok(())
}

fn deserialze_atomic_rule<L: Language>(
  atomic: AtomicRule,
  rules: &mut Vec<Rule<L>>,
  env: &DeserializeEnv<L>,
) -> Result<(), RuleSerializeError> {
  use Rule as R;
  if let Some(pattern) = atomic.pattern {
    rules.push(match pattern {
      PatternStyle::Str(pat) => R::Pattern(Pattern::try_new(&pat, env.lang.clone())?),
      PatternStyle::Contextual {
        context,
        selector,
        strictness,
      } => {
        let pattern = if let Some(selector) = selector {
          Pattern::contextual(&context, &selector, env.lang.clone())?
        } else {
          Pattern::try_new(&context, env.lang.clone())?
        };
        let pattern = if let Some(strictness) = strictness {
          pattern.with_strictness(strictness.into())
        } else {
          pattern
        };
        R::Pattern(pattern)
      }
    });
  }
  if let Some(kind) = atomic.kind {
    rules.push(R::Kind(KindMatcher::try_new(&kind, env.lang.clone())?));
  }
  if let Some(supertype) = atomic.supertype {
    rules.push(R::Supertype(SupertypeMatcher::new(
      &supertype,
      env.lang.clone(),
    )));
  }
  if let Some(regex) = atomic.regex {
    rules.push(R::Regex(RegexMatcher::try_new(&regex)?));
  }
  if let Some(nth_child) = atomic.nth_child {
    rules.push(R::NthChild(NthChild::try_new(nth_child, env)?));
  }
  if let Some(range) = atomic.range {
    rules.push(R::Range(RangeMatcher::try_new(range.start, range.end)?));
  }
  Ok(())
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::from_str;
  use crate::test::TypeScript;
  use PatternStyle::*;

  #[test]
  fn test_pattern() {
    let src = r"
pattern: Test
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(rule.pattern.is_present());
    let src = r"
pattern:
  context: class $C { set $B() {} }
  selector: method_definition
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(matches!(rule.pattern, Maybe::Present(Contextual { .. }),));
  }

  #[test]
  fn test_augmentation() {
    let src = r"
pattern: class A {}
inside:
  pattern: function() {}
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(rule.inside.is_present());
    assert!(rule.pattern.is_present());
  }

  #[test]
  fn test_multi_augmentation() {
    let src = r"
pattern: class A {}
inside:
  pattern: function() {}
has:
  pattern: Some()
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(rule.inside.is_present());
    assert!(rule.has.is_present());
    assert!(rule.follows.is_absent());
    assert!(rule.precedes.is_absent());
    assert!(rule.pattern.is_present());
  }

  #[test]
  fn test_maybe_not() {
    let src = "not: 123";
    let ret: Result<SerializableRule, _> = from_str(src);
    assert!(ret.is_err());
    let src = "not:";
    let ret: Result<SerializableRule, _> = from_str(src);
    assert!(ret.is_err());
  }

  #[test]
  fn test_nested_augmentation() {
    let src = r"
pattern: class A {}
inside:
  pattern: function() {}
  inside:
    pattern:
      context: Some()
      selector: ss
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(rule.inside.is_present());
    let inside = rule.inside.unwrap();
    assert!(inside.rule.pattern.is_present());
    assert!(inside.rule.inside.unwrap().rule.pattern.is_present());
  }

  #[test]
  fn test_precedes_follows() {
    let src = r"
pattern: class A {}
precedes:
  pattern: function() {}
follows:
  pattern:
    context: Some()
    selector: ss
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    assert!(rule.precedes.is_present());
    assert!(rule.follows.is_present());
    let follows = rule.follows.unwrap();
    assert!(follows.rule.pattern.is_present());
    assert!(follows.rule.pattern.is_present());
  }

  #[test]
  fn test_deserialize_rule() {
    let src = r"
pattern: class A {}
kind: class_declaration
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let rule = deserialize_rule(rule, &env).expect("should deserialize");
    assert!(rule.is_composite());
    let root = TypeScript::Tsx.ast_grep("class A {}");
    assert!(root.root().find(rule).is_some());
  }

  #[test]
  fn test_deserialize_order() {
    let src = r"
pattern: class A {}
inside:
  kind: class
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let rule = deserialize_rule(rule, &env).expect("should deserialize");
    assert!(rule.is_composite());
    let Rule::All(rules) = rule else {
      panic!("should parsed as all");
    };
    // relational_rule should be the last
    let rules = rules.inner();
    assert!(rules[0].is_atomic());
    assert!(rules[1].is_relational());
  }

  #[test]
  fn test_defined_vars() {
    let src = r"
pattern: var $A = 123
inside:
  pattern: var $B = 456
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let rule = deserialize_rule(rule, &env).expect("should deserialize");
    assert_eq!(rule.defined_vars(), ["A", "B"].into_iter().collect());
  }

  #[test]
  fn test_issue_1164() {
    let src = r"
    kind: statement_block
    has:
      pattern: this.$A = promise()
      stopBy: end";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let rule = deserialize_rule(rule, &env).expect("should deserialize");
    let root = TypeScript::Tsx.ast_grep(
      "if (a) {
      this.a = b;
      this.d = promise()
    }",
    );
    assert!(root.root().find(rule).is_some());
  }

  #[test]
  fn test_issue_1225() {
    let src = r"
    kind: statement_block
    has:
      pattern: $A
      regex: const";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let rule = deserialize_rule(rule, &env).expect("should deserialize");
    let root = TypeScript::Tsx.ast_grep(
      "{
        let x = 1;
        const z = 9;
      }",
    );
    assert!(root.root().find(rule).is_some());
  }
}
