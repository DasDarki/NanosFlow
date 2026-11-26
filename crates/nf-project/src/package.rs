use std::fmt::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fs_util::{read_toml, write_toml};
use crate::{CustomSetting, Id, ProjectError};

pub const DEFAULT_COMPATIBILITY_VERSION: &str = "1.144";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageType {
    Script,
    GameMode,
    Map,
}

impl PackageType {
    pub fn nanos_section(self) -> &'static str {
        match self {
            PackageType::Script => "script",
            PackageType::GameMode => "game_mode",
            PackageType::Map => "map",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageManifest {
    pub id: Id,
    pub package_type: PackageType,
    pub title: String,
    pub author: String,
    pub version: String,
    pub compatibility_version: String,
    #[serde(default)]
    pub auto_cleanup: bool,
    #[serde(default)]
    pub load_level_entities: bool,
    #[serde(default)]
    pub packages_requirements: Vec<String>,
    #[serde(default)]
    pub assets_requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_asset: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_settings: Vec<CustomSetting>,
}

impl PackageManifest {
    pub const FILE_NAME: &'static str = "package.toml";

    pub fn read(directory: &Path) -> Result<PackageManifest, ProjectError> {
        read_toml(&directory.join(Self::FILE_NAME))
    }

    pub fn write(&self, directory: &Path) -> Result<(), ProjectError> {
        write_toml(&directory.join(Self::FILE_NAME), self)
    }

    pub fn to_nanos_package_toml(&self) -> String {
        let mut output = String::new();
        let quoted = |value: &str| toml::Value::String(value.to_owned()).to_string();
        let list = |values: &[String]| {
            let items = values.iter().map(|value| quoted(value)).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        };

        let _ = writeln!(output, "[meta]");
        let _ = writeln!(output, "    title = {}", quoted(&self.title));
        let _ = writeln!(output, "    author = {}", quoted(&self.author));
        let _ = writeln!(output, "    version = {}", quoted(&self.version));
        let _ = writeln!(output);
        let _ = writeln!(output, "[{}]", self.package_type.nanos_section());
        if let Some(map_asset) = &self.map_asset {
            let _ = writeln!(output, "    map_asset = {}", quoted(map_asset));
        }
        if self.package_type != PackageType::Map {
            let _ = writeln!(output, "    force_no_map_package = false");
        }
        let _ = writeln!(output, "    auto_cleanup = {}", self.auto_cleanup);
        let _ = writeln!(output, "    load_level_entities = {}", self.load_level_entities);
        let _ = writeln!(
            output,
            "    compatibility_version = {}",
            quoted(&self.compatibility_version)
        );
        let _ = writeln!(
            output,
            "    packages_requirements = {}",
            list(&self.packages_requirements)
        );
        let _ = writeln!(
            output,
            "    assets_requirements = {}",
            list(&self.assets_requirements)
        );
        match self.package_type {
            PackageType::GameMode => {
                let _ = writeln!(output, "    compatible_maps = []");
                let _ = writeln!(output);
                let _ = writeln!(output, "[custom_settings]");
                for setting in &self.custom_settings {
                    let _ = writeln!(output, "    {}", setting.nanos_entry());
                }
            }
            PackageType::Script => {
                let _ = writeln!(output, "    compatible_game_modes = []");
            }
            PackageType::Map => {}
        }
        output
    }
}
