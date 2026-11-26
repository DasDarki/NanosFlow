pub fn humanize(identifier: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let characters: Vec<char> = identifier.chars().collect();

    for (index, &character) in characters.iter().enumerate() {
        if character == '_' || character == ' ' || character == '-' {
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
                    || previous.is_ascii_digit()
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
        .into_iter()
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().chain(characters).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn strip_markup(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut inside_tag = false;
    for character in text.chars() {
        match character {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => plain.push(character),
            _ => {}
        }
    }
    plain
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_api_identifiers() {
        assert_eq!(humanize("SetHealth"), "Set Health");
        assert_eq!(humanize("skeletal_mesh_asset"), "Skeletal Mesh Asset");
        assert_eq!(humanize("GetVOIPChannel"), "Get VOIP Channel");
        assert_eq!(humanize("IsInVehicle"), "Is In Vehicle");
        assert_eq!(humanize("X"), "X");
    }

    #[test]
    fn strips_html_from_descriptions() {
        assert_eq!(strip_markup("Pass <code>true</code> &amp; done"), "Pass true & done");
    }
}
