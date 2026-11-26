use std::path::{Path, PathBuf};
use std::process::Command;

const APPLICATION_ID: &str = "NanosFlow";
const ICON_NAME: &str = "nanosflow";
const MIME_TYPE: &str = "application/x-nanosflow-project";
const MIME_XML: &str = include_str!("../linux/nanosflow-mime.xml");
const ICONS: [(&str, &[u8]); 5] = [
    ("32x32", include_bytes!("../icons/32x32.png")),
    ("64x64", include_bytes!("../icons/64x64.png")),
    ("128x128", include_bytes!("../icons/128x128.png")),
    ("256x256", include_bytes!("../icons/256x256.png")),
    ("512x512", include_bytes!("../icons/icon.png")),
];

enum Installation {
    AppImage(PathBuf),
    Development(PathBuf),
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

fn system_entry_exists() -> bool {
    let directories = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    directories
        .split(':')
        .filter(|directory| !directory.is_empty())
        .any(|directory| Path::new(directory).join("applications").join(format!("{APPLICATION_ID}.desktop")).is_file())
}

fn detect_installation() -> Option<Installation> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(Installation::AppImage(PathBuf::from(appimage)));
    }
    if cfg!(debug_assertions) && !system_entry_exists() {
        return std::env::current_exe().ok().map(Installation::Development);
    }
    None
}

fn quote_exec(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for character in path.to_string_lossy().chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn desktop_entry(executable: &Path, name: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Visual scripting for nanos world\nExec={} %f\nIcon={ICON_NAME}\nTerminal=false\nCategories=Development;\nMimeType={MIME_TYPE};\nStartupWMClass={APPLICATION_ID}\n",
        quote_exec(executable)
    )
}

fn write_if_changed(path: &Path, content: &[u8]) -> std::io::Result<bool> {
    if std::fs::read(path).is_ok_and(|existing| existing == content) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(true)
}

fn run_quietly(program: &str, arguments: &[&Path]) {
    let _ = Command::new(program).args(arguments).output();
}

pub fn apply_application_id() {
    glib::set_prgname(Some(APPLICATION_ID));
}

pub fn register() {
    let Some(installation) = detect_installation() else {
        return;
    };
    let Some(data_home) = data_home() else {
        return;
    };
    let (executable, name) = match &installation {
        Installation::AppImage(path) => (path.as_path(), "NanosFlow"),
        Installation::Development(path) => (path.as_path(), "NanosFlow (Development)"),
    };

    let applications = data_home.join("applications");
    let mime = data_home.join("mime");
    let icons = data_home.join("icons").join("hicolor");

    let mut icons_changed = false;
    for (size, bytes) in ICONS {
        let path = icons.join(size).join("apps").join(format!("{ICON_NAME}.png"));
        icons_changed |= write_if_changed(&path, bytes).unwrap_or(false);
    }
    let entry_changed = write_if_changed(
        &applications.join(format!("{APPLICATION_ID}.desktop")),
        desktop_entry(executable, name).as_bytes(),
    )
    .unwrap_or(false);
    let mime_changed = write_if_changed(&mime.join("packages").join(format!("{ICON_NAME}.xml")), MIME_XML.as_bytes()).unwrap_or(false);

    if mime_changed {
        run_quietly("update-mime-database", &[&mime]);
    }
    if entry_changed {
        run_quietly("update-desktop-database", &[&applications]);
    }
    if icons_changed {
        run_quietly("gtk-update-icon-cache", &[Path::new("-f"), Path::new("-t"), &icons]);
    }
    if entry_changed || icons_changed || mime_changed {
        let _ = Command::new("kbuildsycoca6").output().or_else(|_| Command::new("kbuildsycoca5").output());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_executable_paths_for_desktop_entries() {
        let entry = desktop_entry(Path::new("/home/user/My Apps/$nanos\"flow"), "NanosFlow");
        assert!(entry.contains("Exec=\"/home/user/My Apps/\\$nanos\\\"flow\" %f"));
        assert!(entry.contains("StartupWMClass=NanosFlow"));
    }
}
