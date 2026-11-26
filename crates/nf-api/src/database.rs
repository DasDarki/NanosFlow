use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use serde::de::DeserializeOwned;

use crate::{ApiChannel, ApiError, ApiIndex, ClassDefinition, EnumDefinition, Event, Function};

static BUNDLED_API: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../resources/api");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassKind {
    Class,
    StaticClass,
    StandardLibrary,
    Struct,
    UtilityClass,
}

#[derive(Debug, Clone)]
pub struct ApiDatabase {
    channel: ApiChannel,
    revision: String,
    classes: BTreeMap<String, (ClassKind, ClassDefinition)>,
    enums: BTreeMap<String, EnumDefinition>,
}

pub struct ResolvedClass<'a> {
    database: &'a ApiDatabase,
    chain: Vec<&'a ClassDefinition>,
}

trait ApiSource {
    fn read(&self, relative_path: &Path) -> Result<String, ApiError>;
}

struct BundledSource {
    channel: ApiChannel,
}

impl ApiSource for BundledSource {
    fn read(&self, relative_path: &Path) -> Result<String, ApiError> {
        let path = Path::new(self.channel.directory_name()).join(relative_path);
        BUNDLED_API
            .get_file(&path)
            .and_then(|file| file.contents_utf8())
            .map(str::to_owned)
            .ok_or(ApiError::MissingBundledFile { path })
    }
}

struct DirectorySource {
    root: PathBuf,
}

impl ApiSource for DirectorySource {
    fn read(&self, relative_path: &Path) -> Result<String, ApiError> {
        let path = self.root.join(relative_path);
        std::fs::read_to_string(&path).map_err(|source| ApiError::Io { path, source })
    }
}

fn parse<T: DeserializeOwned>(source: &dyn ApiSource, relative_path: &Path) -> Result<T, ApiError> {
    let content = source.read(relative_path)?;
    serde_json::from_str(&content).map_err(|source| ApiError::Parse {
        path: relative_path.to_path_buf(),
        source,
    })
}

impl ApiDatabase {
    pub fn bundled_revision() -> String {
        BUNDLED_API
            .get_file(crate::snapshot::REVISION_FILE)
            .and_then(|file| file.contents_utf8())
            .map(|content| content.trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned())
    }

    pub fn bundled(channel: ApiChannel) -> Result<ApiDatabase, ApiError> {
        Self::load(&BundledSource { channel }, channel, Self::bundled_revision())
    }

    pub fn from_snapshot(root: &Path, channel: ApiChannel) -> Result<ApiDatabase, ApiError> {
        let revision_path = root.join(crate::snapshot::REVISION_FILE);
        let revision = std::fs::read_to_string(&revision_path)
            .map_err(|source| ApiError::Io { path: revision_path, source })?
            .trim()
            .to_owned();
        let source = DirectorySource {
            root: root.join(channel.directory_name()),
        };
        Self::load(&source, channel, revision)
    }

    pub fn from_directory(
        root: impl Into<PathBuf>,
        channel: ApiChannel,
    ) -> Result<ApiDatabase, ApiError> {
        let source = DirectorySource { root: root.into() };
        Self::load(&source, channel, "local".to_owned())
    }

    fn load(
        source: &dyn ApiSource,
        channel: ApiChannel,
        revision: String,
    ) -> Result<ApiDatabase, ApiError> {
        let index: ApiIndex = parse(source, Path::new("APIFiles.json"))?;
        let categories = [
            (ClassKind::Class, "Classes", &index.classes),
            (ClassKind::StaticClass, "StaticClasses", &index.static_classes),
            (ClassKind::StandardLibrary, "StandardLibraries", &index.standard_libraries),
            (ClassKind::Struct, "Structs", &index.structs),
            (ClassKind::UtilityClass, "UtilityClasses", &index.utility_classes),
        ];

        let mut classes = BTreeMap::new();
        for (kind, directory, files) in categories {
            for (name, file) in files {
                let definition: ClassDefinition = parse(source, &Path::new(directory).join(file))?;
                classes.insert(name.clone(), (kind, definition));
            }
        }

        let enums = parse(source, Path::new(&index.enums))?;
        let database = ApiDatabase {
            channel,
            revision,
            classes,
            enums,
        };
        database.validate_inheritance()?;
        Ok(database)
    }

    fn validate_inheritance(&self) -> Result<(), ApiError> {
        for (name, (_, definition)) in &self.classes {
            if let Some(parent) = definition
                .inheritance
                .iter()
                .find(|parent| !self.classes.contains_key(*parent))
            {
                return Err(ApiError::UnknownParent {
                    class: name.clone(),
                    parent: parent.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn channel(&self) -> ApiChannel {
        self.channel
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn class(&self, name: &str) -> Option<&ClassDefinition> {
        self.classes.get(name).map(|(_, definition)| definition)
    }

    pub fn class_kind(&self, name: &str) -> Option<ClassKind> {
        self.classes.get(name).map(|(kind, _)| *kind)
    }

    pub fn classes(&self) -> impl Iterator<Item = (&str, ClassKind, &ClassDefinition)> {
        self.classes
            .iter()
            .map(|(name, (kind, definition))| (name.as_str(), *kind, definition))
    }

    pub fn enum_definition(&self, name: &str) -> Option<&EnumDefinition> {
        self.enums.get(name)
    }

    pub fn enums(&self) -> impl Iterator<Item = (&str, &EnumDefinition)> {
        self.enums.iter().map(|(name, definition)| (name.as_str(), definition))
    }

    pub fn resolve(&self, name: &str) -> Option<ResolvedClass<'_>> {
        let definition = self.class(name)?;
        let mut chain = vec![definition];
        chain.extend(
            definition
                .inheritance
                .iter()
                .rev()
                .filter_map(|parent| self.class(parent)),
        );
        Some(ResolvedClass {
            database: self,
            chain,
        })
    }

    pub fn is_subclass_of(&self, class: &str, ancestor: &str) -> bool {
        class == ancestor
            || self
                .class(class)
                .is_some_and(|definition| definition.inheritance.iter().any(|parent| parent == ancestor))
    }
}

impl<'a> ResolvedClass<'a> {
    pub fn definition(&self) -> &'a ClassDefinition {
        self.chain[0]
    }

    pub fn database(&self) -> &'a ApiDatabase {
        self.database
    }

    pub fn functions(&self) -> impl Iterator<Item = (&'a ClassDefinition, &'a Function)> + '_ {
        self.chain
            .iter()
            .flat_map(|owner| owner.functions.iter().map(move |function| (*owner, function)))
    }

    pub fn events(&self) -> impl Iterator<Item = (&'a ClassDefinition, &'a Event)> + '_ {
        self.chain
            .iter()
            .flat_map(|owner| owner.events.iter().map(move |event| (*owner, event)))
    }

    pub fn find_function(&self, name: &str) -> Option<(&'a ClassDefinition, &'a Function)> {
        self.functions().find(|(_, function)| function.name == name)
    }

    pub fn find_event(&self, name: &str) -> Option<(&'a ClassDefinition, &'a Event)> {
        self.events().find(|(_, event)| event.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Authority, TypeExpr};

    #[test]
    fn loads_every_bundled_channel() {
        for channel in ApiChannel::ALL {
            let database = ApiDatabase::bundled(channel).unwrap();
            assert!(database.class("Character").is_some());
            assert!(database.enum_definition("CollisionChannel").is_some());
        }
    }

    #[test]
    fn resolves_inherited_members() {
        let database = ApiDatabase::bundled(ApiChannel::Latest).unwrap();
        let character = database.resolve("Character").unwrap();

        let (owner, _) = character.find_function("SetLocation").unwrap();
        assert_eq!(owner.name, "Actor");

        let (_, set_health) = character.find_function("SetHealth").unwrap();
        assert_eq!(set_health.authority, Some(Authority::Server));

        assert!(database.is_subclass_of("Character", "Pawn"));
        assert!(!database.is_subclass_of("Prop", "Pawn"));
    }

    #[test]
    fn every_type_in_the_api_parses() {
        let database = ApiDatabase::bundled(ApiChannel::Latest).unwrap();
        for (class_name, _, definition) in database.classes() {
            let functions = definition.functions.iter().chain(&definition.static_functions);
            for function in functions {
                let types = function
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.type_name)
                    .chain(function.returns.iter().map(|value| &value.type_name));
                for type_name in types {
                    TypeExpr::parse(type_name).unwrap_or_else(|error| {
                        panic!("{class_name}.{}: {error}", function.name)
                    });
                }
            }
        }
    }
}
