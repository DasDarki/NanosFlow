use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use nf_project::{Id, Project, ProjectError};
use serde::Serialize;

use crate::emit::WATCH_PREFIX;
use crate::package::{SourceMap, locate};

pub const DEBUG_DIRECTORY: &str = "debug";
const INITIAL_TAIL_BYTES: u64 = 64 * 1024;
const MAXIMUM_READ_BYTES: u64 = 1024 * 1024;
const MAXIMUM_SUGGESTIONS: usize = 6;
const CONTEXT_DIRECTORIES: [&str; 3] = ["Server", "Client", "Shared"];

pub type PackageMaps = BTreeMap<String, SourceMap>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogSeverity {
    Error,
    Warning,
    Info,
    Watch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLocation {
    pub package: Option<String>,
    pub file: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchOutput {
    pub graph: Id,
    pub node: Id,
    pub values: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub severity: LogSeverity,
    pub text: String,
    pub locations: Vec<LogLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<WatchOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub entries: Vec<LogEntry>,
    pub offset: u64,
    pub restarted: bool,
}

fn debug_directory(project_root: &Path) -> PathBuf {
    project_root.join(Project::LOCAL_DIRECTORY).join(DEBUG_DIRECTORY)
}

fn map_file_name(package_folder: &str) -> String {
    format!("{package_folder}.json")
}

pub fn save_source_map(project_root: &Path, package_folder: &str, map: &SourceMap) -> Result<(), ProjectError> {
    let directory = debug_directory(project_root);
    fs::create_dir_all(&directory).map_err(ProjectError::io(&directory))?;
    let path = directory.join(map_file_name(package_folder));
    let content = serde_json::to_string(map).map_err(|error| ProjectError::io(&path)(std::io::Error::other(error)))?;
    fs::write(&path, content).map_err(ProjectError::io(&path))
}

pub fn load_source_maps(project: &Project) -> PackageMaps {
    let directory = debug_directory(project.root());
    project
        .packages()
        .iter()
        .filter_map(|package| {
            let content = fs::read_to_string(directory.join(map_file_name(&package.folder))).ok()?;
            let map: SourceMap = serde_json::from_str(&content).ok()?;
            Some((package.folder.clone(), map))
        })
        .collect()
}

fn is_path_character(character: char) -> bool {
    !(character.is_whitespace() || matches!(character, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | '`' | ','))
}

fn locations_in(text: &str, maps: &PackageMaps) -> Vec<LogLocation> {
    let mut locations: Vec<LogLocation> = Vec::new();
    let mut search_from = 0;
    while let Some(found) = text[search_from..].find(".lua") {
        let extension_end = search_from + found + ".lua".len();
        let after = &text[extension_end..];
        let separator = if after.starts_with(':') {
            1
        } else if after.starts_with("\"]:") {
            3
        } else if after.starts_with("]:") {
            2
        } else {
            search_from = extension_end;
            continue;
        };
        search_from = extension_end + separator;
        let digits: String = text[search_from..].chars().take_while(char::is_ascii_digit).collect();
        let Ok(line) = digits.parse::<usize>() else {
            continue;
        };
        let start = text[..extension_end]
            .char_indices()
            .rev()
            .find(|(_, character)| !is_path_character(*character))
            .map(|(index, character)| index + character.len_utf8())
            .unwrap_or(0);
        let path = text[start..extension_end].replace('\\', "/");
        let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
        let Some(context_index) = segments.iter().rposition(|segment| CONTEXT_DIRECTORIES.contains(segment)) else {
            continue;
        };
        let file = segments[context_index..].join("/");
        let named_package = context_index.checked_sub(1).map(|index| segments[index].to_owned());
        let package = match named_package {
            Some(name) if maps.contains_key(&name) => Some(name),
            Some(name) if maps.len() != 1 => Some(name),
            _ => maps.keys().next().filter(|_| maps.len() == 1).cloned(),
        };
        let span = package
            .as_ref()
            .and_then(|package| maps.get(package))
            .and_then(|map| map.get(&file))
            .and_then(|spans| locate(spans, line));
        let location = LogLocation {
            package,
            file,
            line,
            graph: span.map(|span| span.graph),
            node: span.map(|span| span.node),
        };
        push_location(&mut locations, location);
    }
    locations
}

fn push_location(locations: &mut Vec<LogLocation>, location: LogLocation) {
    let duplicate = locations
        .iter()
        .any(|existing| *existing == location || (location.node.is_some() && existing.node == location.node));
    if !duplicate {
        locations.push(location);
    }
}

fn watch_in(text: &str) -> Option<WatchOutput> {
    let start = text.find(WATCH_PREFIX)? + WATCH_PREFIX.len();
    let rest = &text[start..];
    let close = rest.find(']')?;
    let (graph, node) = rest[..close].split_once('/')?;
    Some(WatchOutput {
        graph: graph.parse().ok()?,
        node: node.parse().ok()?,
        values: rest[close + 1..].trim().to_owned(),
    })
}

fn severity_of(text: &str) -> LogSeverity {
    let lower = text.to_lowercase();
    if lower.contains("error") || lower.contains("attempt to ") || lower.contains("stack traceback") {
        LogSeverity::Error
    } else if lower.contains("warn") {
        LogSeverity::Warning
    } else {
        LogSeverity::Info
    }
}

fn is_continuation(line: &str) -> bool {
    line.starts_with(['\t', ' ']) || line.trim_start().starts_with("stack traceback") || line.trim_start().starts_with("[C]")
}

pub fn parse_log(text: &str, maps: &PackageMaps) -> Vec<LogEntry> {
    let mut entries: Vec<LogEntry> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let locations = locations_in(line, maps);
        if is_continuation(line)
            && let Some(previous) = entries.last_mut()
            && previous.severity != LogSeverity::Watch
        {
            previous.text.push('\n');
            previous.text.push_str(line);
            for location in locations {
                push_location(&mut previous.locations, location);
            }
            continue;
        }
        let watch = watch_in(line);
        let severity = if watch.is_some() { LogSeverity::Watch } else { severity_of(line) };
        entries.push(LogEntry {
            severity,
            text: line.to_owned(),
            locations,
            watch,
        });
    }
    entries
}

pub fn read_log(path: &Path, offset: Option<u64>, maps: &PackageMaps) -> Result<LogChunk, ProjectError> {
    let mut file = fs::File::open(path).map_err(ProjectError::io(path))?;
    let length = file.metadata().map_err(ProjectError::io(path))?.len();
    let (start, restarted) = match offset {
        Some(offset) if offset <= length => (offset, false),
        Some(_) => (0, true),
        None => (length.saturating_sub(INITIAL_TAIL_BYTES), false),
    };
    let end = length.min(start + MAXIMUM_READ_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(ProjectError::io(path))?;
    let mut bytes = Vec::with_capacity((end - start) as usize);
    file.take(end - start).read_to_end(&mut bytes).map_err(ProjectError::io(path))?;
    let complete = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        None if end == length && offset.is_some() => 0,
        None => bytes.len(),
    };
    let mut text = String::from_utf8_lossy(&bytes[..complete]).into_owned();
    if offset.is_none() && start > 0 {
        text = text.split_once('\n').map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    }
    Ok(LogChunk {
        entries: parse_log(&text, maps),
        offset: start + complete as u64,
        restarted,
    })
}

fn newest_logs(directory: &Path, found: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_log = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("log"));
        if is_log && let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) {
            found.push((modified, path));
        }
    }
}

pub fn suggest_log_files(project: &Project) -> Vec<PathBuf> {
    let output = project.output_directory();
    let mut roots = vec![output.clone()];
    if let Some(server) = output.parent() {
        roots.push(server.to_path_buf());
    }
    let mut found = Vec::new();
    for root in roots {
        newest_logs(&root, &mut found);
        newest_logs(&root.join("Logs"), &mut found);
        newest_logs(&root.join("logs"), &mut found);
    }
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let mut paths: Vec<PathBuf> = Vec::new();
    for (_, path) in found {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.truncate(MAXIMUM_SUGGESTIONS);
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::SourceSpan;

    #[test]
    fn maps_lua_locations_and_watch_lines_back_to_nodes() {
        let graph = Id::generate();
        let first = Id::generate();
        let second = Id::generate();
        let map: SourceMap = [(
            "Server/Main.lua".to_owned(),
            vec![SourceSpan { line: 3, graph, node: first }, SourceSpan { line: 8, graph, node: second }],
        )]
        .into_iter()
        .collect();
        let maps: PackageMaps = [("arena".to_owned(), map)].into_iter().collect();
        let log = format!(
            "[12:00:01] [info] Server started\r\n\
             [12:00:02] [error] Lua Error: Packages\\arena\\Server\\Main.lua:9: attempt to index a nil value (local 'player')\n\
             stack traceback:\n\
             \t[C]: in ?\n\
             \tPackages/arena/Server/Main.lua:4: in function <Packages/arena/Server/Main.lua:3>\n\
             [12:00:03] [info] [NanosFlow:{graph}/{first}] Value = 5, Result = 6\n\
             [12:00:04] [warning] something in Client/Other.lua:2 happened\n"
        );
        let entries = parse_log(&log, &maps);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].severity, LogSeverity::Info);
        let error = &entries[1];
        assert_eq!(error.severity, LogSeverity::Error);
        assert!(error.text.contains("stack traceback"));
        let nodes: Vec<Option<Id>> = error.locations.iter().map(|location| location.node).collect();
        assert_eq!(nodes, [Some(second), Some(first)]);
        assert_eq!(error.locations[0].line, 9);
        assert_eq!(error.locations[0].package.as_deref(), Some("arena"));
        let watch = entries[2].watch.as_ref().unwrap();
        assert_eq!((watch.graph, watch.node, watch.values.as_str()), (graph, first, "Value = 5, Result = 6"));
        assert_eq!(entries[3].severity, LogSeverity::Warning);
        assert_eq!(entries[3].locations[0].file, "Client/Other.lua");
        assert_eq!(entries[3].locations[0].node, None);
    }

    #[test]
    fn reads_the_error_format_of_a_real_nanos_world_server() {
        let graph = Id::generate();
        let node = Id::generate();
        let map: SourceMap = [("Server/Main.lua".to_owned(), vec![SourceSpan { line: 6, graph, node }])].into_iter().collect();
        let maps: PackageMaps = [("arena".to_owned(), map), ("core".to_owned(), SourceMap::new())].into_iter().collect();
        let log = "2026-11-25 15:33:42  SCRIPT  [arena] ready\n\
                   2026-11-25 15:33:42   ERROR  Lua Error on Class 'arena' Event 'Load':\n\
                   \t\t- [1] [arena/Server/Main.lua]:8: attempt to index a nil value (local 'broken')\n\
                   \t\t- [2] [arena/Server/Main.lua]:8: in upvalue 'callback'\n\
                   2026-11-25 15:33:42 SUCCESS  Server started!\n";
        let entries = parse_log(log, &maps);
        assert_eq!(entries.len(), 3, "{entries:#?}");
        let error = &entries[1];
        assert_eq!(error.severity, LogSeverity::Error);
        assert!(error.text.contains("in upvalue 'callback'"));
        assert_eq!(error.locations.len(), 1);
        assert_eq!(error.locations[0].package.as_deref(), Some("arena"));
        assert_eq!((error.locations[0].line, error.locations[0].node), (8, Some(node)));
        assert_eq!(entries[2].severity, LogSeverity::Info);
    }

    #[test]
    fn follows_a_growing_log_file() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("server.log");
        fs::write(&path, "first\nsecond\npartial").unwrap();
        let maps = PackageMaps::new();
        let chunk = read_log(&path, None, &maps).unwrap();
        assert_eq!(chunk.entries.iter().map(|entry| entry.text.as_str()).collect::<Vec<_>>(), ["first", "second"]);
        fs::write(&path, "first\nsecond\npartial line\nthird\n").unwrap();
        let next = read_log(&path, Some(chunk.offset), &maps).unwrap();
        assert_eq!(next.entries.iter().map(|entry| entry.text.as_str()).collect::<Vec<_>>(), ["partial line", "third"]);
        fs::write(&path, "fresh\n").unwrap();
        let rotated = read_log(&path, Some(next.offset), &maps).unwrap();
        assert!(rotated.restarted);
        assert_eq!(rotated.entries[0].text, "fresh");
    }
}
