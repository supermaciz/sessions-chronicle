use std::fs;
use std::path::{Path, PathBuf};

pub fn resolve_project_path(cwd: &str) -> String {
    let Some(existing) = nearest_existing_ancestor(Path::new(cwd)) else {
        return cwd.to_string();
    };

    let Ok(canonical_existing) = fs::canonicalize(&existing) else {
        return cwd.to_string();
    };

    find_git_directory_root(&canonical_existing)
        .unwrap_or(canonical_existing)
        .to_string_lossy()
        .into_owned()
}

fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|candidate| candidate.exists())
        .map(Path::to_path_buf)
}

fn find_git_directory_root(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        let git_marker = ancestor.join(".git");
        if git_marker.is_dir() || git_marker.is_file() {
            return Some(ancestor.to_path_buf());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::resolve_project_path;
    use std::fs;
    use tempfile::tempdir;

    fn path_string(path: &std::path::Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn resolves_repo_root_when_git_directory_exists() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let nested = repo.join("src/lib");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();

        assert_eq!(
            resolve_project_path(nested.to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn resolves_repo_root_when_git_file_marker_exists() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let nested = repo.join("src/lib");
        fs::create_dir_all(&nested).unwrap();
        fs::write(repo.join(".git"), "gitdir: /tmp/worktrees/repo").unwrap();

        assert_eq!(
            resolve_project_path(nested.to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn resolves_existing_non_git_directory_to_canonical_path() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("plain/subdir");
        fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            resolve_project_path(dir.to_str().unwrap()),
            path_string(&dir.canonicalize().unwrap())
        );
    }

    #[test]
    fn resolves_missing_leaf_from_nearest_existing_parent() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::create_dir(repo.join(".git")).unwrap();
        let missing_leaf = repo.join("src/generated/output");

        assert_eq!(
            resolve_project_path(missing_leaf.to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }
}
