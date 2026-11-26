use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "of", rename_all = "camelCase")]
pub enum TypeExpr {
    Named(String),
    Array(Box<TypeExpr>),
    Optional(Box<TypeExpr>),
    Variadic(Box<TypeExpr>),
    Union(Vec<TypeExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid type expression '{input}': {reason}")]
pub struct ParseTypeError {
    pub input: String,
    pub reason: &'static str,
}

impl TypeExpr {
    pub fn parse(input: &str) -> Result<TypeExpr, ParseTypeError> {
        let error = |reason| ParseTypeError {
            input: input.to_owned(),
            reason,
        };

        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(error("type is empty"));
        }

        if let Some(inner) = trimmed.strip_suffix("...") {
            return Ok(TypeExpr::Variadic(Box::new(Self::parse(inner)?)));
        }

        if trimmed.contains('|') {
            let members = trimmed
                .split('|')
                .map(Self::parse_single)
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| error("union member is malformed"))?;
            return Ok(TypeExpr::Union(members));
        }

        Self::parse_single(trimmed).ok_or_else(|| error("type is malformed"))
    }

    fn parse_single(input: &str) -> Option<TypeExpr> {
        let input = input.trim();
        if let Some(inner) = input.strip_suffix('?') {
            return Some(TypeExpr::Optional(Box::new(Self::parse_single(inner)?)));
        }
        if let Some(inner) = input.strip_suffix("[]") {
            return Some(TypeExpr::Array(Box::new(Self::parse_single(inner)?)));
        }
        let is_identifier = !input.is_empty()
            && input
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_');
        is_identifier.then(|| TypeExpr::Named(input.to_owned()))
    }

    pub fn named(name: impl Into<String>) -> TypeExpr {
        TypeExpr::Named(name.into())
    }

    pub fn array(element: TypeExpr) -> TypeExpr {
        TypeExpr::Array(Box::new(element))
    }

    pub fn referenced_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        self.collect_names(&mut names);
        names
    }

    fn collect_names<'a>(&'a self, names: &mut Vec<&'a str>) {
        match self {
            TypeExpr::Named(name) => names.push(name),
            TypeExpr::Array(inner) | TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => {
                inner.collect_names(names)
            }
            TypeExpr::Union(members) => members.iter().for_each(|member| member.collect_names(names)),
        }
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExpr::Named(name) => formatter.write_str(name),
            TypeExpr::Array(inner) => write!(formatter, "{inner}[]"),
            TypeExpr::Optional(inner) => write!(formatter, "{inner}?"),
            TypeExpr::Variadic(inner) => write!(formatter, "{inner}..."),
            TypeExpr::Union(members) => {
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str("|")?;
                    }
                    write!(formatter, "{member}")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> TypeExpr {
        TypeExpr::Named(name.to_owned())
    }

    #[test]
    fn parses_all_forms_used_by_the_api() {
        assert_eq!(TypeExpr::parse("Vector").unwrap(), named("Vector"));
        assert_eq!(
            TypeExpr::parse("Actor[]").unwrap(),
            TypeExpr::Array(Box::new(named("Actor")))
        );
        assert_eq!(
            TypeExpr::parse("Player?").unwrap(),
            TypeExpr::Optional(Box::new(named("Player")))
        );
        assert_eq!(
            TypeExpr::parse("any...").unwrap(),
            TypeExpr::Variadic(Box::new(named("any")))
        );
        assert_eq!(
            TypeExpr::parse("CollisionChannel|integer").unwrap(),
            TypeExpr::Union(vec![named("CollisionChannel"), named("integer")])
        );
    }

    #[test]
    fn round_trips_through_display() {
        for input in ["table[]", "string|table", "float?", "integer...", "Prop|Pickable"] {
            assert_eq!(TypeExpr::parse(input).unwrap().to_string(), input);
        }
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(TypeExpr::parse("").is_err());
        assert!(TypeExpr::parse("Vector<").is_err());
        assert!(TypeExpr::parse("a||b").is_err());
    }
}
