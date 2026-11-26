use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(Ulid);

impl Id {
    pub fn generate() -> Id {
        Id(Ulid::new())
    }

    pub fn derived(owner: Id, original: Id) -> Id {
        let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
        for byte in u128::from(owner.0).to_le_bytes().into_iter().chain(u128::from(original.0).to_le_bytes()) {
            hash ^= u128::from(byte);
            hash = hash.wrapping_mul(0x0000000001000000000000000000013b);
        }
        Id(Ulid::from(hash))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.0.to_string().to_lowercase();
        formatter.write_str(&text)
    }
}

impl FromStr for Id {
    type Err = ulid::DecodeError;

    fn from_str(value: &str) -> Result<Id, Self::Err> {
        Ulid::from_string(&value.to_uppercase()).map(Id)
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Id, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse()
            .map_err(|error| serde::de::Error::custom(format!("'{text}' is not a valid id: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_lowercase_and_reads_any_case() {
        let id = Id::generate();
        let json = serde_json_like(&id);
        assert_eq!(json, json.to_lowercase());
        let upper: Id = json.to_uppercase().parse().unwrap();
        assert_eq!(upper, id);
    }

    fn serde_json_like(id: &Id) -> String {
        #[derive(Serialize)]
        struct Wrapper {
            id: Id,
        }
        let text = toml::to_string(&Wrapper { id: *id }).unwrap();
        text.trim().trim_start_matches("id = ").trim_matches('"').to_owned()
    }
}
