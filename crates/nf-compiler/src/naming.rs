use std::collections::HashSet;

use crate::lua::{KEYWORDS, STANDARD_GLOBALS, is_identifier};

fn words(text: &str) -> Vec<String> {
    let characters: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &character) in characters.iter().enumerate() {
        if !character.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|position| characters[position]);
        let next = characters.get(index + 1).copied();
        let starts_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || (previous.is_ascii_uppercase() && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
        if starts_word && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn snake_case(text: &str) -> String {
    let joined = words(text)
        .iter()
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_");
    if joined.is_empty() {
        "value".to_owned()
    } else if joined.starts_with(|character: char| character.is_ascii_digit()) {
        format!("value_{joined}")
    } else {
        joined
    }
}

pub fn pascal_case(text: &str) -> String {
    let joined: String = words(text)
        .iter()
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if joined.is_empty() {
        "Unnamed".to_owned()
    } else if joined.starts_with(|character: char| character.is_ascii_digit()) {
        format!("N{joined}")
    } else {
        joined
    }
}

#[derive(Debug, Clone, Default)]
pub struct NameScope {
    used: HashSet<String>,
}

impl NameScope {
    pub fn with_reserved<'a>(reserved: impl IntoIterator<Item = &'a str>) -> NameScope {
        let mut scope = NameScope::default();
        for name in KEYWORDS.iter().chain(STANDARD_GLOBALS.iter()).copied().chain(reserved) {
            scope.used.insert(name.to_owned());
        }
        scope
    }

    pub fn claim(&mut self, base: &str) -> String {
        let base = snake_case(base);
        let mut candidate = base.clone();
        let mut counter = 2;
        while self.used.contains(&candidate) || !is_identifier(&candidate) {
            candidate = format!("{base}_{counter}");
            counter += 1;
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_cases() {
        assert_eq!(snake_case("GetHTTPData"), "get_http_data");
        assert_eq!(snake_case("New Health"), "new_health");
        assert_eq!(snake_case("Vector2D"), "vector2d");
        assert_eq!(snake_case("2fast"), "value_2fast");
        assert_eq!(pascal_case("heal player"), "HealPlayer");
        assert_eq!(pascal_case("round-manager_v2"), "RoundManagerV2");
        assert_eq!(pascal_case("HealPlayer"), "HealPlayer");
    }

    #[test]
    fn claims_unique_names_that_avoid_keywords_and_globals() {
        let mut scope = NameScope::with_reserved(["player"]);
        assert_eq!(scope.claim("player"), "player_2");
        assert_eq!(scope.claim("player"), "player_3");
        assert_eq!(scope.claim("end"), "end_2");
        assert_eq!(scope.claim("string"), "string_2");
        assert_eq!(scope.claim("Hit Location"), "hit_location");
    }
}
