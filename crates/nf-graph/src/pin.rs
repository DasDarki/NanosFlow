use nf_api::TypeExpr;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PinType {
    Exec,
    Data {
        #[serde(rename = "type")]
        type_expr: TypeExpr,
    },
}

impl PinType {
    pub fn data(type_expr: TypeExpr) -> PinType {
        PinType::Data { type_expr }
    }

    pub fn named(name: &str) -> PinType {
        PinType::data(TypeExpr::named(name))
    }

    pub fn is_exec(&self) -> bool {
        matches!(self, PinType::Exec)
    }

    pub fn type_expr(&self) -> Option<&TypeExpr> {
        match self {
            PinType::Exec => None,
            PinType::Data { type_expr } => Some(type_expr),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinDefinition {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub pin_type: PinType,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    pub optional: bool,
}

impl PinDefinition {
    pub fn exec(name: &str, label: &str) -> PinDefinition {
        PinDefinition {
            name: name.to_owned(),
            label: label.to_owned(),
            pin_type: PinType::Exec,
            description: String::new(),
            default: None,
            optional: true,
        }
    }

    pub fn data(name: &str, label: &str, type_expr: TypeExpr) -> PinDefinition {
        PinDefinition {
            name: name.to_owned(),
            label: label.to_owned(),
            pin_type: PinType::data(type_expr),
            description: String::new(),
            default: None,
            optional: false,
        }
    }

    pub fn with_default(mut self, default: &str) -> PinDefinition {
        self.default = Some(default.to_owned());
        self.optional = true;
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> PinDefinition {
        self.description = description.into();
        self
    }
}
