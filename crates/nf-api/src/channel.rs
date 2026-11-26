use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiChannel {
    #[default]
    Stable,
    Latest,
}

impl ApiChannel {
    pub const ALL: [ApiChannel; 2] = [ApiChannel::Stable, ApiChannel::Latest];

    pub fn directory_name(self) -> &'static str {
        match self {
            ApiChannel::Stable => "stable",
            ApiChannel::Latest => "latest",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ApiChannel::Stable => "Stable",
            ApiChannel::Latest => "Latest (bleeding edge)",
        }
    }
}
