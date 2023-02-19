use crate::maybe::Maybe;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

use std::fmt;

/// We have three kinds of rules in ast-grep.
/// * Atomic: the most basic rule to match AST. We have two variants: Pattern and Kind.
/// * Relational: filter matched target according to their position relative to other nodes.
/// * Composite: use logic operation all/any/not to compose the above rules to larger rules.
/// Every rule has it's unique name so we can combine several rules in one object.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SerializableRule {
  // avoid embedding AtomicRule/RelationalRule/CompositeRule with flatten here for better error message

  // atomic
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub pattern: Maybe<PatternStyle>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub kind: Maybe<String>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub regex: Maybe<String>,
  // relational
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub inside: Maybe<Box<Relation>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub has: Maybe<Box<Relation>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub precedes: Maybe<Box<Relation>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub follows: Maybe<Box<Relation>>,
  // composite
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub all: Maybe<Vec<SerializableRule>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub any: Maybe<Vec<SerializableRule>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub not: Maybe<Box<SerializableRule>>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  pub matches: Maybe<String>,
}

pub struct Categorized {
  pub atomic: AtomicRule,
  pub relational: RelationalRule,
  pub composite: CompositeRule,
}

impl SerializableRule {
  pub fn categorized(self) -> Categorized {
    Categorized {
      atomic: AtomicRule {
        pattern: self.pattern.into(),
        kind: self.kind.into(),
        regex: self.regex.into(),
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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AtomicRule {
  pub pattern: Option<PatternStyle>,
  pub kind: Option<String>,
  pub regex: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum PatternStyle {
  Str(String),
  Contextual { context: String, selector: String },
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RelationalRule {
  pub inside: Option<Box<Relation>>,
  pub has: Option<Box<Relation>>,
  pub precedes: Option<Box<Relation>>,
  pub follows: Option<Box<Relation>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
  #[serde(flatten)]
  pub rule: SerializableRule,
  #[serde(default)]
  pub stop_by: SerializableStopBy,
  pub field: Option<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub enum SerializableStopBy {
  Neighbor,
  #[default]
  End,
  Rule(SerializableRule),
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CompositeRule {
  pub all: Option<Vec<SerializableRule>>,
  pub any: Option<Vec<SerializableRule>>,
  pub not: Option<Box<SerializableRule>>,
  pub matches: Option<String>,
}

struct StopByVisitor;
impl<'de> Visitor<'de> for StopByVisitor {
  type Value = SerializableStopBy;
  fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
    formatter.write_str("`neighbor`, `end` or a rule object")
  }

  fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
  where
    E: de::Error,
  {
    match value {
      "neighbor" => Ok(SerializableStopBy::Neighbor),
      "end" => Ok(SerializableStopBy::End),
      v => Err(de::Error::custom(format!(
        "unknown variant `{v}`, expected `neighbor`, `end` or a rule object",
      ))),
    }
  }

  fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
  where
    A: MapAccess<'de>,
  {
    let rule = Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
    Ok(SerializableStopBy::Rule(rule))
  }
}

impl<'de> Deserialize<'de> for SerializableStopBy {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    deserializer.deserialize_any(StopByVisitor)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::from_str;
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
  fn test_relational() {
    let src = r"
inside:
  pattern: class A {}
  stopBy: neighbor
";
    let rule: SerializableRule = from_str(src).expect("cannot parse rule");
    let stop_by = rule.inside.unwrap().stop_by;
    assert!(matches!(stop_by, SerializableStopBy::Neighbor));
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

  fn to_stop_by(src: &str) -> Result<SerializableStopBy, serde_yaml::Error> {
    from_str(src)
  }

  #[test]
  fn test_stop_by_ok() {
    let stop = to_stop_by("'neighbor'").expect("cannot parse stopBy");
    assert!(matches!(stop, SerializableStopBy::Neighbor));
    let stop = to_stop_by("'end'").expect("cannot parse stopBy");
    assert!(matches!(stop, SerializableStopBy::End));
    let stop = to_stop_by("kind: some-kind").expect("cannot parse stopBy");
    assert!(matches!(stop, SerializableStopBy::Rule(_)));
  }

  macro_rules! cast_err {
    ($reg: expr) => {
      match $reg {
        Err(a) => a,
        _ => panic!("non-matching variant"),
      }
    };
  }

  #[test]
  fn test_stop_by_err() {
    let err = cast_err!(to_stop_by("'ddd'")).to_string();
    assert!(err.contains("unknown variant"));
    assert!(err.contains("ddd"));
    let err = cast_err!(to_stop_by("pattern: 1233"));
    assert!(err.to_string().contains("variant"));
  }
}
