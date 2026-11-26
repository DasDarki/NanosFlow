use std::path::{Path, PathBuf};

use nf_project::PROJECT_EXTENSION;

pub fn find_project_argument<I, S>(arguments: I, working_directory: &Path) -> Option<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    arguments
        .into_iter()
        .skip(1)
        .map(|argument| PathBuf::from(argument.as_ref()))
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION))
        })
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                working_directory.join(path)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_relative_and_absolute_project_paths() {
        let working_directory = Path::new("/work");
        let found = find_project_argument(["nanosflow", "--flag", "Game/Game.nfproj"], working_directory);
        assert_eq!(found, Some(PathBuf::from("/work/Game/Game.nfproj")));

        let found = find_project_argument(["nanosflow", "/abs/Game.NFPROJ"], working_directory);
        assert_eq!(found, Some(PathBuf::from("/abs/Game.NFPROJ")));

        assert_eq!(find_project_argument(["nanosflow"], working_directory), None);
    }
}
