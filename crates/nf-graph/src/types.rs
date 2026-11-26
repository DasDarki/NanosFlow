use std::collections::{HashMap, HashSet};

use nf_api::{ApiDatabase, ClassKind, TypeExpr};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypeCategory {
    Boolean,
    Number,
    Integer,
    String,
    AssetPath,
    Table,
    Function,
    Any,
    Enum,
    Struct,
    Object,
}

#[derive(Debug, Clone, Default)]
pub struct TypeSystem {
    classes: HashMap<String, (ClassKind, Vec<String>)>,
    enums: HashSet<String>,
    structs: HashSet<String>,
}

impl TypeSystem {
    pub fn from_database(database: &ApiDatabase) -> TypeSystem {
        TypeSystem {
            classes: database
                .classes()
                .map(|(name, kind, definition)| (name.to_owned(), (kind, definition.inheritance.clone())))
                .collect(),
            enums: database.enums().map(|(name, _)| name.to_owned()).collect(),
            structs: HashSet::new(),
        }
    }

    pub fn category(&self, name: &str) -> TypeCategory {
        match name {
            "boolean" => TypeCategory::Boolean,
            "number" | "float" => TypeCategory::Number,
            "integer" => TypeCategory::Integer,
            "string" => TypeCategory::String,
            "table" => TypeCategory::Table,
            "function" => TypeCategory::Function,
            _ if self.structs.contains(name) => TypeCategory::Table,
            _ if name.ends_with("Path") && !self.classes.contains_key(name) => TypeCategory::AssetPath,
            _ if self.enums.contains(name) => TypeCategory::Enum,
            _ => match self.classes.get(name) {
                Some((ClassKind::Struct, _)) => TypeCategory::Struct,
                Some((ClassKind::Class, _)) => TypeCategory::Object,
                _ => TypeCategory::Any,
            },
        }
    }

    pub fn with_project_classes(&self, classes: &[(String, Vec<String>)]) -> TypeSystem {
        let mut extended = self.clone();
        for (name, parents) in classes {
            extended
                .classes
                .insert(name.clone(), (ClassKind::Class, parents.clone()));
        }
        extended
    }

    pub fn with_project_enums(mut self, names: impl IntoIterator<Item = String>) -> TypeSystem {
        self.enums.extend(names);
        self
    }

    pub fn with_project_structs(mut self, names: impl IntoIterator<Item = String>) -> TypeSystem {
        self.structs.extend(names);
        self
    }

    pub fn is_project_struct(&self, name: &str) -> bool {
        self.structs.contains(name)
    }

    pub fn parents_of(&self, name: &str) -> Vec<String> {
        self.classes
            .get(name)
            .map(|(_, parents)| parents.clone())
            .unwrap_or_default()
    }

    pub fn is_known(&self, name: &str) -> bool {
        matches!(
            name,
            "boolean" | "number" | "float" | "integer" | "string" | "table" | "function" | "any"
        ) || self.enums.contains(name)
            || self.structs.contains(name)
            || self.classes.contains_key(name)
            || name.ends_with("Path")
    }

    pub fn is_subclass_of(&self, class: &str, ancestor: &str) -> bool {
        class == ancestor
            || self
                .classes
                .get(class)
                .is_some_and(|(_, parents)| parents.iter().any(|parent| parent == ancestor))
    }

    pub fn is_assignable(&self, from: &TypeExpr, to: &TypeExpr) -> bool {
        match (from, to) {
            (_, TypeExpr::Variadic(inner)) | (_, TypeExpr::Optional(inner)) => self.is_assignable(from, inner),
            (TypeExpr::Variadic(inner), _) | (TypeExpr::Optional(inner), _) => self.is_assignable(inner, to),
            (TypeExpr::Union(members), _) => members.iter().all(|member| self.is_assignable(member, to)),
            (_, TypeExpr::Union(members)) => members.iter().any(|member| self.is_assignable(from, member)),
            (TypeExpr::Array(from_element), TypeExpr::Array(to_element)) => {
                self.is_assignable(from_element, to_element)
            }
            (TypeExpr::Array(_), TypeExpr::Named(name)) => {
                matches!(self.category(name), TypeCategory::Table | TypeCategory::Any)
            }
            (TypeExpr::Named(name), TypeExpr::Array(_)) => {
                matches!(self.category(name), TypeCategory::Table | TypeCategory::Any)
            }
            (TypeExpr::Named(from_name), TypeExpr::Named(to_name)) => self.is_name_assignable(from_name, to_name),
        }
    }

    fn is_name_assignable(&self, from: &str, to: &str) -> bool {
        if from == to {
            return true;
        }
        let from_category = self.category(from);
        let to_category = self.category(to);
        match (from_category, to_category) {
            (TypeCategory::Any, _) | (_, TypeCategory::Any) => true,
            (TypeCategory::Number | TypeCategory::Integer, TypeCategory::Number | TypeCategory::Integer) => true,
            (TypeCategory::Enum, TypeCategory::Number | TypeCategory::Integer) => true,
            (TypeCategory::String, TypeCategory::AssetPath) | (TypeCategory::AssetPath, TypeCategory::String) => true,
            (TypeCategory::Object, TypeCategory::Object) => self.is_subclass_of(from, to),
            (TypeCategory::Table, TypeCategory::Table) => from == "table" || to == "table",
            _ => false,
        }
    }

    pub fn describe_mismatch(&self, from: &TypeExpr, to: &TypeExpr) -> Option<String> {
        let (TypeExpr::Named(from_name), TypeExpr::Named(to_name)) = (strip(from), strip(to)) else {
            return None;
        };
        match (self.category(from_name), self.category(to_name)) {
            (TypeCategory::Object, TypeCategory::Object) if self.is_subclass_of(to_name, from_name) => Some(format!(
                "Every {to_name} is a {from_name}, but not every {from_name} is a {to_name}. Check the type first, for example with a branch on IsA."
            )),
            (TypeCategory::Integer | TypeCategory::Number, TypeCategory::Enum) => Some(format!(
                "Pick a value of {to_name} instead of passing a plain number."
            )),
            (_, TypeCategory::String) => Some("Convert the value with a 'To String' node first.".to_owned()),
            _ => None,
        }
    }
}

fn strip(type_expr: &TypeExpr) -> &TypeExpr {
    match type_expr {
        TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => strip(inner),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nf_api::ApiChannel;

    fn parse(input: &str) -> TypeExpr {
        TypeExpr::parse(input).unwrap()
    }

    #[test]
    fn follows_nanos_world_typing_rules() {
        let database = ApiDatabase::bundled(ApiChannel::Stable).unwrap();
        let types = TypeSystem::from_database(&database);
        let assignable = |from: &str, to: &str| types.is_assignable(&parse(from), &parse(to));

        assert!(assignable("Character", "Actor"));
        assert!(!assignable("Actor", "Character"));
        assert!(assignable("integer", "float"));
        assert!(assignable("string", "SoundPath"));
        assert!(!assignable("SoundPath", "StaticMeshPath"));
        assert!(assignable("Player?", "Player"));
        assert!(assignable("Character[]", "Actor[]"));
        assert!(assignable("Character[]", "table"));
        assert!(assignable("Prop|Pickable", "Actor"));
        assert!(assignable("Prop|Pickable", "Prop|Pickable"));
        assert!(!assignable("string|table", "string"));
        assert!(assignable("CollisionChannel", "CollisionChannel|integer"));
        assert!(assignable("Vector", "any..."));
        assert!(!assignable("Vector", "Rotator"));
        assert!(!assignable("integer", "CollisionChannel"));
        assert!(assignable("CollisionChannel", "integer"));
    }

    #[test]
    fn categorizes_named_types() {
        let database = ApiDatabase::bundled(ApiChannel::Stable).unwrap();
        let types = TypeSystem::from_database(&database);
        assert_eq!(types.category("Vector"), TypeCategory::Struct);
        assert_eq!(types.category("Character"), TypeCategory::Object);
        assert_eq!(types.category("CollisionChannel"), TypeCategory::Enum);
        assert_eq!(types.category("SkeletalMeshPath"), TypeCategory::AssetPath);
        assert_eq!(types.category("float"), TypeCategory::Number);
    }
}
