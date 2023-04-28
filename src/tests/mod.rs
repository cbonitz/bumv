use crate::{
    create_rename_mapping, create_rename_prompt, create_temp_file_content, parse_temp_file_content,
    read_directory_files, read_directory_files_recursive, rename_files,
};
use std::fs::File;
use tempfile::tempdir;

fn create_test_files(dir: &tempfile::TempDir) {
    let file1 = dir.path().join("file1.txt");
    let file2 = dir.path().join("file2.txt");
    let file3 = dir.path().join("subdir").join("file3.txt");
    let file4 = dir.path().join("subdir").join("file4.txt");

    let subdir = dir.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();

    File::create(&file1).unwrap();
    File::create(&file2).unwrap();
    File::create(&file3).unwrap();
    File::create(&file4).unwrap();
}

/// Validate non-recursive reading of files
#[test]
fn test_read_directory_files_nonrecursive() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = read_directory_files(dir.path(), false).unwrap();

    assert_eq!(files.len(), 2);
    assert!(files[0].file_name().unwrap() == "file1.txt");
    assert!(files[1].file_name().unwrap() == "file2.txt");
}

/// Validate recursive reading of files
#[test]
fn test_read_directory_files_recursive() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = read_directory_files_recursive(dir.path(), false).unwrap();

    assert_eq!(files.len(), 4);
    // assertions take into account temp dir prefixes
    assert_eq!(files[0].file_name().unwrap(), "file1.txt");
    assert_eq!(files[1].file_name().unwrap(), "file2.txt");
    assert_eq!(files[2].file_name().unwrap(), "file3.txt");
    assert_eq!(files[3].file_name().unwrap(), "file4.txt");
}

/// Validate the content of the temporary file.
#[test]
fn test_create_temp_file_content() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let files = read_directory_files_recursive(dir.path(), false).unwrap();

    let content = create_temp_file_content(&files);

    let lines: Vec<_> = content.split("\n").collect();
    // assertions take into account temp dir prefixes
    assert!(lines[0].ends_with("/file1.txt"));
    assert!(lines[1].ends_with("/file2.txt"));
    assert!(lines[2].ends_with("/subdir/file3.txt"));
    assert!(lines[3].ends_with("/subdir/file4.txt"));
}

/// Validate renaming a file in the current directory
/// ```
/// file1.txt
/// file2.txt
/// ```
/// to
/// ```
/// file2.txt
/// renamed_file1.txt
/// ```
#[test]
fn scenario_test_rename_files() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = read_directory_files(dir.path(), false).unwrap();
    let content = create_temp_file_content(&files);

    // simulate file editing
    let new_files = parse_temp_file_content(content.replace("file1.txt", "renamed_file1.txt"));

    let rename_mapping = create_rename_mapping(&files, &new_files).unwrap();

    // verify rename prompt format
    let rename_prompt = create_rename_prompt(&rename_mapping);
    let (from, to) = rename_prompt.split_once(" -> ").unwrap();
    // assertions take into account temp dir prefixes
    assert!(from.ends_with("file1.txt"));
    assert!(to.ends_with("renamed_file1.txt"));

    rename_files(&rename_mapping).unwrap();

    // validate files after renaming
    let files_after_rename = read_directory_files(dir.path(), false).unwrap();
    assert_eq!(files_after_rename.len(), 2);
    // sorted alphabetically
    assert_eq!(files_after_rename[0].file_name().unwrap(), "file2.txt");
    assert_eq!(
        files_after_rename[1].file_name().unwrap(),
        "renamed_file1.txt"
    );
}

/// Validate renaming a file each in the current directory and in a subdirectory.
/// ```
/// file1.txt
/// file2.txt
/// subdir/file3.txt
/// subdir/file4.txt
/// ```
/// to
/// ```
/// file2.txt
/// renamed_file1.txt
/// subdir/file4.txt
/// subdir/renamed_file3.txt
/// ```
#[test]
fn scenario_test_rename_files_recursive() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = read_directory_files_recursive(dir.path(), false).unwrap();
    let content = create_temp_file_content(&files);

    // simulate file editing
    let new_files = parse_temp_file_content(
        content
            .replace("file1.txt", "renamed_file1.txt")
            .replace("/subdir/file3.txt", "/subdir/renamed_file3.txt"),
    );

    let rename_mapping = create_rename_mapping(&files, &new_files).unwrap();

    // verify rename prompt format
    let rename_prompt = create_rename_prompt(&rename_mapping);
    let (rename_prompt_1, rename_prompt_2) = rename_prompt.split_once("\n").unwrap();
    let (from, to) = rename_prompt_1.split_once(" -> ").unwrap();
    // assertions take into account temp dir prefixes
    assert!(from.ends_with("file1.txt"));
    assert!(to.ends_with("renamed_file1.txt"));
    let (from, to) = rename_prompt_2.split_once(" -> ").unwrap();
    assert!(from.ends_with("/subdir/file3.txt"));
    assert!(to.ends_with("/subdir/renamed_file3.txt"));

    rename_files(&rename_mapping).unwrap();

    // validate files after renaming
    let files_after_rename = read_directory_files_recursive(dir.path(), false).unwrap();
    assert_eq!(files_after_rename.len(), 4);
    // sorted alphabetically
    assert_eq!(files_after_rename[0].file_name().unwrap(), "file2.txt");
    assert_eq!(
        files_after_rename[1].file_name().unwrap(),
        "renamed_file1.txt"
    );
    assert_eq!(files_after_rename[2].file_name().unwrap(), "file4.txt");
    assert_eq!(
        files_after_rename[3].file_name().unwrap(),
        "renamed_file3.txt"
    );
}
