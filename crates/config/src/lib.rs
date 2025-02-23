mod check_var;
mod combined;
mod fixer;
mod maybe;
mod rule;
mod rule_collection;
mod rule_config;
mod rule_core;
mod transform;

use serde::Deserialize;
use serde_yaml::{with::singleton_map_recursive::deserialize, Deserializer, Error as YamlError};

use ast_grep_core::language::Language;

pub use combined::{CombinedScan, PreScan};
pub use fixer::Fixer;
pub use rule::referent_rule::GlobalRules;
pub use rule::DeserializeEnv;
pub use rule::{Rule, RuleSerializeError, SerializableRule};
pub use rule_collection::RuleCollection;
pub use rule_config::{RuleConfig, RuleConfigError, SerializableRuleConfig, Severity};
pub use rule_core::{RuleCore, RuleCoreError, SerializableRuleCore};
pub use transform::Transformation;

pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, YamlError> {
  let deserializer = Deserializer::from_str(s);
  deserialize(deserializer)
}

pub fn from_yaml_string<'a, L: Language + Deserialize<'a>>(
  yamls: &'a str,
  registration: &GlobalRules<L>,
) -> Result<Vec<RuleConfig<L>>, RuleConfigError> {
  let mut ret = vec![];
  for yaml in Deserializer::from_str(yamls) {
    let config = RuleConfig::deserialize(yaml, registration)?;
    ret.push(config);
  }
  Ok(ret)
}
#[cfg(test)]
mod test {

  use super::*;
  use ast_grep_core::language::TSLanguage;
  use std::path::Path;

  #[derive(Clone, Deserialize, PartialEq, Eq)]
  pub enum TypeScript {
    Tsx,
  }
  impl Language for TypeScript {
    fn from_path<P: AsRef<Path>>(_path: P) -> Option<Self> {
      Some(TypeScript::Tsx)
    }
    fn get_ts_language(&self) -> TSLanguage {
      tree_sitter_typescript::LANGUAGE_TSX.into()
    }
  }

  fn test_rule_match(yaml: &str, source: &str) {
    let globals = GlobalRules::default();
    let config = &from_yaml_string::<TypeScript>(yaml, &globals).expect("rule should parse")[0];
    let grep = config.language.ast_grep(source);
    assert!(grep.root().find(&config.matcher).is_some());
  }

  fn test_rule_unmatch(yaml: &str, source: &str) {
    let globals = GlobalRules::default();
    let config = &from_yaml_string::<TypeScript>(yaml, &globals).expect("rule should parse")[0];
    let grep = config.language.ast_grep(source);
    assert!(grep.root().find(&config.matcher).is_none());
  }

  fn make_yaml(rule: &str) -> String {
    format!(
      r"
id: test
message: test rule
severity: info
language: Tsx
rule:
{rule}
"
    )
  }

  #[test]
  fn test_deserialize_rule_config() {
    let yaml = &make_yaml(
      "
  pattern: let a = 123
",
    );
    test_rule_match(yaml, "let a = 123; let b = 33;");
    test_rule_match(yaml, "class B { func() {let a = 123; }}");
    test_rule_unmatch(yaml, "const a = 33");
  }

  #[test]
  fn test_deserialize_nested() {
    let yaml = &make_yaml(
      "
  all:
    - pattern: let $A = 123
    - pattern: let a = $B
",
    );
    test_rule_match(yaml, "let a = 123; let b = 33;");
    test_rule_match(yaml, "class B { func() {let a = 123; }}");
    test_rule_unmatch(yaml, "const a = 33");
    test_rule_unmatch(yaml, "let a = 33");
  }

  #[test]
  fn test_deserialize_kind() {
    let yaml = &make_yaml(
      "
    kind: class_body
",
    );
    test_rule_match(yaml, "class B { func() {let a = 123; }}");
    test_rule_unmatch(yaml, "const B = { func() {let a = 123; }}");
  }

  #[test]
  fn test_deserialize_inside() {
    let yaml = &make_yaml(
      "
  all:
    - inside:
        kind: class_body
        stopBy: end
    - pattern: let a = 123
",
    );
    test_rule_unmatch(yaml, "let a = 123; let b = 33;");
    test_rule_match(yaml, "class B { func() {let a = 123; }}");
    test_rule_unmatch(yaml, "let a = 123");
  }

  #[test]
  fn test_deserialize_not_inside() {
    let yaml = &make_yaml(
      "
  all:
    - not:
        inside:
          kind: class_body
          stopBy: end
    - pattern: let a = 123
",
    );
    test_rule_match(yaml, "let a = 123; let b = 33;");
    test_rule_unmatch(yaml, "class B { func() {let a = 123; }}");
    test_rule_unmatch(yaml, "let a = 13");
  }

  #[test]
  fn test_deserialize_meta_var() {
    let yaml = &make_yaml(
      "
  all:
    - inside:
        any:
          - pattern: function $A($$$) { $$$ }
          - pattern: let $A = ($$$) => $$$
        stopBy: end
    - pattern: $A($$$)
",
    );
    test_rule_match(yaml, "function recursion() { recursion() }");
    test_rule_match(yaml, "let recursion = () => { recursion() }");
    test_rule_unmatch(yaml, "function callOther() { other() }");
  }

  #[test]
  fn test_deserialize_constraints() {
    let yaml = r"
id: test
message: test rule
severity: info
language: Tsx
rule:
  all:
    - pattern: console.log($A)
    - inside:
        pattern: function $B() {$$$}
        stopBy: end
constraints:
  B:
    regex: test
";
    test_rule_match(yaml, "function test() { console.log(1) }");
    test_rule_match(yaml, "function test() { console.log(2) }");
    test_rule_unmatch(yaml, "function tt() { console.log(2) }");
  }

  // https://github.com/ast-grep/ast-grep/issues/813
  #[test]
  fn test_util_rule_with_vaargs() {
    let yaml = r"
id: sibling
language: Tsx
utils:
  utilpat:
    pattern: '$A($$$B);'
rule:
  matches: utilpat
  follows:
    matches: utilpat
    stopBy: end
";
    test_rule_match(yaml, "a();a(123);a();a(123)");
  }
}
