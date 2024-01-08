use crate::maybe::Maybe;
use crate::rule::{Relation, Rule, RuleSerializeError, StopBy};
use crate::transform::Transformation;
use crate::DeserializeEnv;
use ast_grep_core::replacer::{IndentSensitive, Replacer, TemplateFix, TemplateFixError};
use ast_grep_core::{Doc, Language};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum FixerError {
  #[error("Fixer template is invalid.")]
  InvalidTemplate(#[from] TemplateFixError),
  #[error("Fixer expansion contains invalid rule.")]
  WrongExpansion(#[from] RuleSerializeError),
}

struct Expander<L: Language> {
  matches: Rule<L>,
  stop_by: StopBy<L>,
}

impl<L: Language> Expander<L> {
  fn parse(
    relation: &Maybe<Relation>,
    env: &DeserializeEnv<L>,
  ) -> Result<Option<Self>, FixerError> {
    let inner = match relation {
      Maybe::Absent => return Ok(None),
      Maybe::Present(r) => r.clone(),
    };
    let stop_by = StopBy::try_from(inner.stop_by, env)?;
    let matches = env.deserialize_rule(inner.rule)?;
    Ok(Some(Self { matches, stop_by }))
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
    serialized: &SerializableFixConfig,
    env: &DeserializeEnv<L>,
  ) -> Result<Self, FixerError> {
    let SerializableFixConfig {
      template,
      expand_end,
      expand_start,
    } = serialized;
    let expand_start = Expander::parse(expand_start, env)?;
    let expand_end = Expander::parse(expand_end, env)?;
    Ok(Self {
      template: TemplateFix::try_new(template, &env.lang)?,
      expand_start,
      expand_end,
    })
  }

  pub fn parse(
    fixer: &SerializableFixer,
    env: &DeserializeEnv<L>,
    transform: &Option<HashMap<String, Transformation>>,
  ) -> Result<Self, FixerError> {
    let fixer = match fixer {
      SerializableFixer::Str(fix) => {
        let template = if let Some(trans) = transform {
          let keys: Vec<_> = trans.keys().cloned().collect();
          TemplateFix::with_transform(fix, &env.lang, &keys)
        } else {
          TemplateFix::try_new(fix, &env.lang)?
        };
        Self {
          template,
          expand_end: None,
          expand_start: None,
        }
      }
      SerializableFixer::Config(cfg) => Self::do_parse(cfg, env)?,
    };
    Ok(fixer)
  }

  pub fn from_str(src: &str, lang: &L) -> Result<Self, FixerError> {
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
    // simple forwarding to template
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
    let Maybe::Present(relation) = cfg.expand_end else {
      panic!("wrong parsing")
    };
    let rule = relation.rule;
    assert_eq!(rule.regex, Maybe::Present(",".to_string()));
    assert!(rule.pattern.is_absent());
    Ok(())
  }

  #[test]
  fn test_parse_config() -> Result<(), FixerError> {
    let relation = from_str("{regex: ',', stopBy: neighbor}").expect("should deser");
    let config = SerializableFixConfig {
      expand_end: Maybe::Present(relation),
      expand_start: Maybe::Absent,
      template: "abcd".to_string(),
    };
    let config = SerializableFixer::Config(config);
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let ret = Fixer::<String, _>::parse(&config, &env, &Some(Default::default()))?;
    assert!(ret.expand_start.is_none());
    assert!(ret.expand_end.is_some());
    assert!(matches!(ret.template, TemplateFix::Textual(_)));
    Ok(())
  }

  #[test]
  fn test_parse_str() -> Result<(), FixerError> {
    let config = SerializableFixer::Str("abcd".to_string());
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let ret = Fixer::<String, _>::parse(&config, &env, &Some(Default::default()))?;
    assert!(ret.expand_end.is_none());
    assert!(ret.expand_start.is_none());
    assert!(matches!(ret.template, TemplateFix::Textual(_)));
    Ok(())
  }

  #[test]
  fn test_replace_fixer() -> Result<(), FixerError> {
    let expand_end = from_str("{regex: ',', stopBy: neighbor}").expect("should word");
    let config = SerializableFixConfig {
      expand_end: Maybe::Present(expand_end),
      expand_start: Maybe::Absent,
      template: "var $A = 456".to_string(),
    };
    let config = SerializableFixer::Config(config);
    let env = DeserializeEnv::new(TypeScript::Tsx);
    let ret = Fixer::<String, _>::parse(&config, &env, &Some(Default::default()))?;
    let grep = TypeScript::Tsx.ast_grep("let a = 123");
    let node = grep.root().find("let $A = 123").expect("should found");
    let edit = ret.generate_replacement(&node);
    assert_eq!(String::from_utf8_lossy(&edit), "var a = 456");
    Ok(())
  }
}
