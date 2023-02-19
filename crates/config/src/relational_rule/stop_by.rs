use crate::deserialize_env::DeserializeEnv;
use crate::rule::{deserialize_rule, Rule, RuleSerializeError, SerializableRule};

use ast_grep_core::language::Language;
use ast_grep_core::Node;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub enum SerializableStopBy {
  Neighbor,
  #[default]
  End,
  Rule(SerializableRule),
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

pub enum StopBy<L: Language> {
  Neighbor,
  End,
  Rule(Rule<L>),
}

impl<L: Language> StopBy<L> {
  pub(crate) fn try_from(
    relation: SerializableStopBy,
    env: &DeserializeEnv<L>,
  ) -> Result<Self, RuleSerializeError> {
    use SerializableStopBy as S;
    Ok(match relation {
      S::Neighbor => StopBy::Neighbor,
      S::End => StopBy::End,
      S::Rule(r) => StopBy::Rule(deserialize_rule(r, env)?),
    })
  }
}

impl<L: Language> StopBy<L> {
  pub(crate) fn find<'t, I, F>(&self, mut iter: I, mut finder: F) -> Option<Node<'t, L>>
  where
    L: 't,
    I: Iterator<Item = Node<'t, L>>,
    F: FnMut(Node<'t, L>) -> Option<Node<'t, L>>,
  {
    match self {
      StopBy::End => iter.find_map(finder),
      StopBy::Neighbor => finder(iter.next()?),
      StopBy::Rule(stop) => iter.take_while(inclusive_until(stop)).find_map(finder),
    }
  }
}

fn inclusive_until<L: Language>(rule: &Rule<L>) -> impl FnMut(&Node<L>) -> bool + '_ {
  let mut matched = false;
  move |n| {
    if matched {
      false
    } else {
      matched = n.matches(rule);
      true
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::from_str;

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
