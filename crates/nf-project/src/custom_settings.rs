use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::FieldIssue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CustomSettingKind {
    Boolean,
    Integer,
    Floating,
    Select,
    Text,
}

impl CustomSettingKind {
    pub fn nanos_name(self) -> &'static str {
        match self {
            CustomSettingKind::Boolean => "boolean",
            CustomSettingKind::Integer => "integer",
            CustomSettingKind::Floating => "floating",
            CustomSettingKind::Select => "select",
            CustomSettingKind::Text => "text",
        }
    }

    pub fn lua_type(self) -> &'static str {
        match self {
            CustomSettingKind::Boolean => "boolean",
            CustomSettingKind::Integer => "integer",
            CustomSettingKind::Floating => "number",
            CustomSettingKind::Select | CustomSettingKind::Text => "string",
        }
    }

    pub fn fallback(self) -> toml::Value {
        match self {
            CustomSettingKind::Boolean => toml::Value::Boolean(false),
            CustomSettingKind::Integer => toml::Value::Integer(0),
            CustomSettingKind::Floating => toml::Value::Float(0.0),
            CustomSettingKind::Select | CustomSettingKind::Text => toml::Value::String(String::new()),
        }
    }

    pub fn accepts(self, value: &toml::Value) -> bool {
        matches!(
            (self, value),
            (CustomSettingKind::Boolean, toml::Value::Boolean(_))
                | (CustomSettingKind::Integer, toml::Value::Integer(_))
                | (CustomSettingKind::Floating, toml::Value::Float(_) | toml::Value::Integer(_))
                | (CustomSettingKind::Select | CustomSettingKind::Text, toml::Value::String(_))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomSetting {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(rename = "type")]
    pub kind: CustomSettingKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub default: toml::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

impl CustomSetting {
    pub fn display_label(&self) -> &str {
        if self.label.trim().is_empty() { &self.key } else { &self.label }
    }

    pub fn default_value(&self) -> toml::Value {
        match (&self.kind, &self.default) {
            (CustomSettingKind::Floating, toml::Value::Integer(value)) => toml::Value::Float(*value as f64),
            (kind, value) if kind.accepts(value) => value.clone(),
            (kind, _) => kind.fallback(),
        }
    }

    pub fn nanos_entry(&self) -> String {
        let quoted = |value: &str| toml::Value::String(value.to_owned()).to_string();
        let mut parts = vec![
            format!("label = {}", quoted(self.display_label())),
            format!("type = {}", quoted(self.kind.nanos_name())),
        ];
        if !self.description.trim().is_empty() {
            parts.push(format!("description = {}", quoted(self.description.trim())));
        }
        parts.push(format!("default = {}", self.default_value()));
        if self.kind == CustomSettingKind::Select {
            let options: Vec<String> = self.options.iter().map(|option| quoted(option)).collect();
            parts.push(format!("options = [ {} ]", options.join(", ")));
        }
        format!("{} = {{ {} }}", self.key, parts.join(", "))
    }
}

fn is_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters.next().is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_')
}

pub fn validate_custom_settings(settings: &[CustomSetting]) -> Vec<FieldIssue> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for setting in settings {
        let key = setting.key.as_str();
        if !is_key(key) {
            issues.push(FieldIssue::error(
                "customSettings",
                format!("'{key}' is not a valid key. Use lower case letters, digits and underscores, for example 'max_props'."),
            ));
        } else if !seen.insert(key) {
            issues.push(FieldIssue::error("customSettings", format!("The key '{key}' is used twice.")));
        }
        if !setting.kind.accepts(&setting.default) {
            issues.push(FieldIssue::error(
                "customSettings",
                format!("The default of '{key}' does not match its type {}.", setting.kind.nanos_name()),
            ));
        }
        if setting.kind == CustomSettingKind::Select {
            if setting.options.iter().all(|option| option.trim().is_empty()) {
                issues.push(FieldIssue::error("customSettings", format!("'{key}' is a select and needs at least one option.")));
            } else if let toml::Value::String(default) = &setting.default
                && !setting.options.contains(default)
            {
                issues.push(FieldIssue::error("customSettings", format!("The default of '{key}' must be one of its options.")));
            }
        }
    }
    issues
}

pub fn command_line_argument(values: &BTreeMap<String, toml::Value>) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let pairs: Vec<String> = values
        .iter()
        .filter(|(key, _)| is_key(key))
        .map(|(key, value)| match value {
            toml::Value::String(text) => format!("{key} = '{}'", text.replace('\'', "")),
            other => format!("{key} = {other}"),
        })
        .collect();
    (!pairs.is_empty()).then(|| pairs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(key: &str, kind: CustomSettingKind, default: toml::Value) -> CustomSetting {
        CustomSetting {
            key: key.to_owned(),
            label: String::new(),
            kind,
            description: String::new(),
            default,
            options: Vec::new(),
        }
    }

    #[test]
    fn validates_and_exports_settings() {
        let mut mode = setting("game_mode", CustomSettingKind::Select, toml::Value::String("Classic".into()));
        mode.label = "Game Mode".into();
        mode.options = vec!["Classic".into(), "Hardcore".into()];
        let props = setting("max_props", CustomSettingKind::Integer, toml::Value::Integer(1000));
        assert!(validate_custom_settings(&[mode.clone(), props.clone()]).is_empty());
        assert_eq!(
            mode.nanos_entry(),
            r#"game_mode = { label = "Game Mode", type = "select", default = "Classic", options = [ "Classic", "Hardcore" ] }"#
        );
        assert_eq!(props.nanos_entry(), r#"max_props = { label = "max_props", type = "integer", default = 1000 }"#);

        let broken = setting("Max Props", CustomSettingKind::Boolean, toml::Value::Integer(1));
        assert_eq!(validate_custom_settings(&[broken, props.clone(), props]).len(), 3);

        let values = BTreeMap::from([
            ("max_props".to_owned(), toml::Value::Integer(5)),
            ("game_mode".to_owned(), toml::Value::String("Hardcore".into())),
        ]);
        assert_eq!(command_line_argument(&values).unwrap(), "game_mode = 'Hardcore', max_props = 5");
    }
}
