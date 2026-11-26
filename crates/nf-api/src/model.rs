use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Authority {
    Both,
    Server,
    Client,
    Authority,
    NetworkAuthority,
    BothNetAuthorityFirst,
}

impl Authority {
    pub fn allows_server(self) -> bool {
        !matches!(self, Authority::Client)
    }

    pub fn allows_client(self) -> bool {
        !matches!(self, Authority::Server)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Efficiency {
    Fast,
    Moderate,
    Slow,
    Blocking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkDistribution {
    Enabled,
    Disabled,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub function_parameters: Vec<Parameter>,
    #[serde(default)]
    pub table_properties: Vec<Parameter>,
}

impl Parameter {
    pub fn is_optional(&self) -> bool {
        self.default.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnValue {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub table_properties: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Relations {
    #[serde(default)]
    pub functions: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub static_functions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub description_details: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub authority: Option<Authority>,
    #[serde(default)]
    pub efficiency: Option<Efficiency>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default, rename = "return")]
    pub returns: Vec<ReturnValue>,
    #[serde(default)]
    pub relations: Relations,
    #[serde(default)]
    pub examples: Vec<Example>,
    #[serde(default)]
    pub last_compatibility_version: Option<String>,
    #[serde(default)]
    pub is_native: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub description_details: Option<String>,
    #[serde(default)]
    pub authority: Option<Authority>,
    #[serde(default)]
    pub arguments: Vec<Parameter>,
    #[serde(default, rename = "return")]
    pub returns: Vec<ReturnValue>,
    #[serde(default)]
    pub relations: Relations,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Constructor {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    #[serde(default, rename = "type")]
    pub type_name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operator {
    pub name: String,
    pub operator: String,
    #[serde(default)]
    pub lhs: Option<String>,
    #[serde(default)]
    pub rhs: Option<String>,
    #[serde(rename = "return")]
    pub returns: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authority: Option<Authority>,
    #[serde(default)]
    pub network_distribution: Option<NetworkDistribution>,
    #[serde(default)]
    pub is_base: bool,
    #[serde(default)]
    pub inheritance: Vec<String>,
    #[serde(default)]
    pub constructors: Vec<Constructor>,
    #[serde(default)]
    pub functions: Vec<Function>,
    #[serde(default)]
    pub static_functions: Vec<Function>,
    #[serde(default)]
    pub events: Vec<Event>,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub static_properties: Vec<Property>,
    #[serde(default)]
    pub operators: Vec<Operator>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumValue {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDefinition {
    #[serde(default)]
    pub description: String,
    #[serde(rename = "enums")]
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ApiIndex {
    #[serde(rename = "Classes", default)]
    pub classes: BTreeMap<String, String>,
    #[serde(rename = "StaticClasses", default)]
    pub static_classes: BTreeMap<String, String>,
    #[serde(rename = "StandardLibraries", default)]
    pub standard_libraries: BTreeMap<String, String>,
    #[serde(rename = "Structs", default)]
    pub structs: BTreeMap<String, String>,
    #[serde(rename = "UtilityClasses", default)]
    pub utility_classes: BTreeMap<String, String>,
    #[serde(rename = "Enums", default)]
    pub enums: String,
}
