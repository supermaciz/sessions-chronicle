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
        .find(|candidate| candidate.exists() && !is_filesystem_root(candidate))
        .map(Path::to_path_buf)
}

fn is_filesystem_root(path: &Path) -> bool {
    path.parent().is_none()
}

fn find_git_directory_root(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        let dot_git = ancestor.join(".git");

        if dot_git.is_dir() {
            return Some(ancestor.to_path_buf());
        }

        if dot_git.is_file() {
            return resolve_gitfile_root(&dot_git).or_else(|| Some(ancestor.to_path_buf()));
        }
    }

    None
}

fn resolve_gitfile_root(gitfile_path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(gitfile_path).ok()?;
    let gitdir_raw = contents.strip_prefix("gitdir:")?.trim();
    let gitdir_path = Path::new(gitdir_raw);

    let resolved_gitdir = if gitdir_path.is_absolute() {
        gitdir_path.to_path_buf()
    } else {
        gitfile_path.parent()?.join(gitdir_path)
    };

    let canonical_gitdir = fs::canonicalize(resolved_gitdir).ok()?;
    repo_root_from_gitdir(&canonical_gitdir)
}

fn repo_root_from_gitdir(gitdir: &Path) -> Option<PathBuf> {
    if gitdir.file_name()? == ".git" {
        return Some(gitdir.parent()?.to_path_buf());
    }

    if gitdir.parent()?.file_name()? == "worktrees" {
        let common_git_dir = gitdir.parent()?.parent()?;
        if common_git_dir.file_name()? == ".git" {
            return Some(common_git_dir.parent()?.to_path_buf());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::resolve_project_path;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn path_string(path: &std::path::Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn write_gitfile(path: &Path, gitdir: &Path) {
        fs::write(path, format!("gitdir: {}\n", gitdir.display())).unwrap();
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

    #[test]
    fn resolves_worktree_gitfile_to_main_repo_root() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("repo-worktree");
        let worktree_gitdir = repo.join(".git/worktrees/repo-worktree");

        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&worktree_gitdir).unwrap();
        fs::create_dir_all(worktree.join("src")).unwrap();
        write_gitfile(&worktree.join(".git"), &worktree_gitdir);

        assert_eq!(
            resolve_project_path(worktree.join("src").to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn resolves_relative_gitfile_paths() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let worktree = temp.path().join("repo-worktree");
        let worktree_gitdir = repo.join(".git/worktrees/repo-worktree");

        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&worktree_gitdir).unwrap();
        fs::create_dir_all(worktree.join("src")).unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: ../repo/.git/worktrees/{}\n", "repo-worktree"),
        )
        .unwrap();

        assert_eq!(
            resolve_project_path(worktree.join("src").to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn falls_back_to_gitfile_ancestor_when_mapping_is_unrecognized() {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("repo");
        let nested = repo.join("src/lib");

        fs::create_dir_all(&nested).unwrap();
        fs::write(repo.join(".git"), "gitdir: not-a-recognized-layout\n").unwrap();

        assert_eq!(
            resolve_project_path(nested.to_str().unwrap()),
            path_string(&repo.canonicalize().unwrap())
        );
    }

    #[test]
    fn returns_raw_path_when_no_existing_ancestor_exists() {
        let raw = "/definitely/missing/project/root/subdir";
        assert_eq!(resolve_project_path(raw), raw);
    }

    #[cfg(unix)]
    #[test]
    fn canonicalizes_symlinked_paths_before_returning_repo_root() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let real_repo = temp.path().join("real-repo");
        let linked_repo = temp.path().join("linked-repo");
        fs::create_dir_all(real_repo.join("src")).unwrap();
        fs::create_dir(real_repo.join(".git")).unwrap();
        symlink(&real_repo, &linked_repo).unwrap();

        assert_eq!(
            resolve_project_path(linked_repo.join("src").to_str().unwrap()),
            path_string(&real_repo.canonicalize().unwrap())
        );
    }
}
