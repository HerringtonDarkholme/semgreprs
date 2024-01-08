use crate::maybe::Maybe;
use crate::rule::{Relation, Rule, StopBy};
use crate::transform::Transformation;
use crate::DeserializeEnv;
use ast_grep_core::replacer::{IndentSensitive, Replacer, TemplateFix, TemplateFixError};
use ast_grep_core::{Doc, Language};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

/// A pattern string or fix object to auto fix the issue.
/// It can reference metavariables appeared in rule.
#[derive(Serialize, Deserialize, Clone, JsonSchema)]
#[serde(untagged)]
pub enum SerializableFixer {
  Str(String),
  Config(SerializableFixConfig),
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SerializableFixConfig {
  template: String,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  expand_end: Maybe<Relation>,
  #[serde(default, skip_serializing_if = "Maybe::is_absent")]
  expand_start: Maybe<Relation>,
  // TODO: add these
  // prepend: String,
}

struct Expander<L: Language> {
  matches: Rule<L>,
  stop_by: StopBy<L>,
}

impl<L: Language> Expander<L> {
  fn parse(relation: Maybe<Relation>, env: &DeserializeEnv<L>) -> Option<Self> {
    let Maybe::Present(inner) = relation else {
      return None;
    };
    // TODO
    let stop_by = StopBy::try_from(inner.stop_by, env).unwrap();
    let matches = env.deserialize_rule(inner.rule).unwrap();
    Some(Self { matches, stop_by })
  }
}

pub struct Fixer<C: IndentSensitive, L: Language> {
  template: TemplateFix<C>,
  expand_start: Option<Expander<L>>,
  expand_end: Option<Expander<L>>,
}

impl<C, L> Fixer<C, L>
where
  C: IndentSensitive,
  L: Language,
{
  fn do_parse(
    serialized: SerializableFixConfig,
    env: &DeserializeEnv<L>,
  ) -> Result<Self, TemplateFixError> {
    let SerializableFixConfig {
      template,
      expand_end,
      expand_start,
    } = serialized;
    let expand_start = Expander::parse(expand_start, env);
    let expand_end = Expander::parse(expand_end, env);
    Ok(Self {
      template: TemplateFix::try_new(&template, &env.lang)?,
      expand_start,
      expand_end,
    })
  }

  pub fn parse(
    fixer: &SerializableFixer,
    env: &DeserializeEnv<L>,
    transform: &Option<HashMap<String, Transformation>>,
  ) -> Result<Option<Self>, TemplateFixError> {
    let SerializableFixer::Str(fix) = fixer else {
      return Ok(None);
    };
    let template = if let Some(trans) = transform {
      let keys: Vec<_> = trans.keys().cloned().collect();
      TemplateFix::with_transform(fix, &env.lang, &keys)
    } else {
      TemplateFix::try_new(fix, &env.lang)?
    };
    Ok(Some(Self {
      template,
      expand_end: None,
      expand_start: None,
    }))
  }

  pub fn from_str(src: &str, lang: &L) -> Result<Self, TemplateFixError> {
    let template = TemplateFix::try_new(src, lang)?;
    Ok(Self {
      template,
      expand_start: None,
      expand_end: None,
    })
  }
}

impl<D, L, C> Replacer<D> for Fixer<C, L>
where
  D: Doc<Source = C, Lang = L>,
  L: Language,
  C: IndentSensitive,
{
  fn generate_replacement(&self, nm: &ast_grep_core::NodeMatch<D>) -> Vec<C::Underlying> {
    // TODO: refine this method
    self.template.generate_replacement(nm)
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::from_str;
  use crate::maybe::Maybe;
  use crate::test::TypeScript;

  #[test]
  fn test_parse() {
    let fixer: SerializableFixer = from_str("test").expect("should parse");
    assert!(matches!(fixer, SerializableFixer::Str(_)));
  }

  #[test]
  fn test_deserialize_object() -> Result<(), serde_yaml::Error> {
    let src = "{template: 'abc', expandEnd: {regex: ',', stopBy: neighbor}}";
    let SerializableFixer::Config(cfg) = from_str(src)? else {
      panic!("wrong parsing")
    };
    assert_eq!(cfg.template, "abc");
    let rule = cfg.expand_end.unwrap().rule;
    assert_eq!(rule.regex, Maybe::Present(",".to_string()));
    assert!(rule.pattern.is_absent());
    Ok(())
  }

  #[test]
  fn test_parse_config() -> Result<(), serde_yaml::Error> {
    let config = SerializableFixConfig {
      expand_end: Maybe::Present(from_str("{regex: ',', stopBy: neighbor}")?),
      expand_start: Maybe::Absent,
      template: "abcd".to_string(),
    };
    let config = SerializableFixer::Config(config);
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let ret = Fixer::<String, _>::parse(&config, &env, &Some(Default::default())).unwrap();
    assert!(ret.is_none());
    Ok(())
  }

  #[test]
  fn test_parse_str() -> Result<(), serde_yaml::Error> {
    let config = SerializableFixer::Str("abcd".to_string());
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let ret = Fixer::<String, _>::parse(&config, &env, &Some(Default::default())).unwrap();
    let ret = ret.unwrap();
    assert!(ret.expand_end.is_none());
    assert!(ret.expand_start.is_none());
    Ok(())
  }
}
