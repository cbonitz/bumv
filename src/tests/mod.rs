use crate::{bulk_rename, create_editable_temp_file_content, BumvConfiguration};
use std::{
    cell::RefCell,
    fs::{self, File},
    io::Write,
    rc::Rc,
};
use tempfile::{tempdir, TempDir};

fn create_test_files(dir: &tempfile::TempDir) {
    let ignore = dir.path().join(".ignore");
    let file1 = dir.path().join("file1.txt");
    let file2 = dir.path().join("file2.txt");
    let ignored = dir.path().join("ignored.txt");
    let file3 = dir.path().join("subdir").join("file3.txt");
    let file4 = dir.path().join("subdir").join("file4.txt");

    let subdir = dir.path().join("subdir");
    std::fs::create_dir_all(subdir).unwrap();

    let mut ignore = File::create(ignore).unwrap();
    ignore
        .write_all("ignored.txt\nalso_ignored.txt".as_bytes())
        .unwrap();
    ignore.flush().unwrap();
    File::create(file1).unwrap();
    File::create(file2).unwrap();
    File::create(ignored).unwrap();
    File::create(file3).unwrap();
    File::create(file4).unwrap();
}

fn assert_no_files_changed(dir: &TempDir) {
    assert!(dir.path().join(".ignore").exists());
    assert!(dir.path().join("file1.txt").exists());
    assert!(dir.path().join("file2.txt").exists());
    assert!(dir.path().join("ignored.txt").exists());
    assert!(dir.path().join("subdir").join("file3.txt").exists());
    assert!(dir.path().join("subdir").join("file4.txt").exists());
}

/// Validate non-recursive reading of files
#[test]
fn test_read_directory_files_nonrecursive() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.into_path()),
    }
    .file_list();

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].file_name().unwrap(), "file1.txt");
    assert_eq!(files[1].file_name().unwrap(), "file2.txt");
}

/// Validate non-recursive reading of files ignoring ignore files
#[test]
fn test_read_directory_files_nonrecursive_no_ignore() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = BumvConfiguration {
        recursive: false,
        no_ignore: true,
        use_vscode: false,
        base_path: Some(dir.into_path()),
    }
    .file_list();

    assert_eq!(files.len(), 4);
    assert_eq!(files[0].file_name().unwrap(), ".ignore");
    assert_eq!(files[1].file_name().unwrap(), "file1.txt");
    assert_eq!(files[2].file_name().unwrap(), "file2.txt");
    assert_eq!(files[3].file_name().unwrap(), "ignored.txt");
}

/// Validate recursive reading of files
#[test]
fn test_read_directory_files_recursive() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = BumvConfiguration {
        recursive: true,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.into_path()),
    }
    .file_list();

    assert_eq!(files.len(), 4);
    // assertions take into account temp dir prefixes
    assert_eq!(files[0].file_name().unwrap(), "file1.txt");
    assert_eq!(files[1].file_name().unwrap(), "file2.txt");
    assert_eq!(files[2].file_name().unwrap(), "file3.txt");
    assert_eq!(files[3].file_name().unwrap(), "file4.txt");
}

/// Validate recursive reading of files
#[test]
fn test_read_directory_files_recursive_no_ignore() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = BumvConfiguration {
        recursive: true,
        no_ignore: true,
        use_vscode: false,
        base_path: Some(dir.into_path()),
    }
    .file_list();

    assert_eq!(files.len(), 6);
    // assertions take into account temp dir prefixes
    assert_eq!(files[0].file_name().unwrap(), ".ignore");
    assert_eq!(files[1].file_name().unwrap(), "file1.txt");
    assert_eq!(files[2].file_name().unwrap(), "file2.txt");
    assert_eq!(files[3].file_name().unwrap(), "ignored.txt");
    assert_eq!(files[4].file_name().unwrap(), "file3.txt");
    assert_eq!(files[5].file_name().unwrap(), "file4.txt");
}

/// Validate the content of the temporary file.
#[test]
fn test_create_temp_file_content() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let files = BumvConfiguration {
        recursive: true,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.into_path()),
    }
    .file_list();

    let content = create_editable_temp_file_content(&files);

    let lines: Vec<_> = content.split('\n').collect();
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
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let prompted = Rc::new(RefCell::new(false));
    let prompted_clone = prompted.clone();

    bulk_rename(
        config,
        |content| Ok(content.replace("file1.txt", "renamed_file1.txt")),
        Box::new(move |prompt: String| {
            let (from, to) = prompt.split_once(" -> ").unwrap();
            // assertions take into account temp dir prefixes
            assert!(from.ends_with("file1.txt"));
            assert!(to.ends_with("renamed_file1.txt"));
            *prompted_clone.borrow_mut() = true;
            true
        }),
    )
    .unwrap();

    assert!(*prompted.borrow());

    // verify renaming
    assert!(dir.path().join(".ignore").exists());
    assert!(!dir.path().join("file1.txt").exists());
    assert!(dir.path().join("renamed_file1.txt").exists());
    assert!(dir.path().join("file2.txt").exists());
    assert!(dir.path().join("ignored.txt").exists());
    assert!(dir.path().join("subdir").join("file3.txt").exists());
    assert!(dir.path().join("subdir").join("file4.txt").exists());
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

    let config = BumvConfiguration {
        recursive: true,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let prompted = Rc::new(RefCell::new(false));
    let prompted_clone = prompted.clone();

    bulk_rename(
        config,
        |content| {
            Ok(content
                .replace("file1.txt", "renamed_file1.txt")
                .replace("/subdir/file3.txt", "/subdir/renamed_file3.txt"))
        },
        Box::new(move |prompt: String| {
            let (rename_prompt_1, rename_prompt_2) = prompt.split_once('\n').unwrap();
            let (from, to) = rename_prompt_1.split_once(" -> ").unwrap();
            // assertions take into account temp dir prefixes
            assert!(from.ends_with("file1.txt"));
            assert!(to.ends_with("renamed_file1.txt"));
            let (from, to) = rename_prompt_2.split_once(" -> ").unwrap();
            assert!(from.ends_with("/subdir/file3.txt"));
            assert!(to.ends_with("/subdir/renamed_file3.txt"));
            *prompted_clone.borrow_mut() = true;
            true
        }),
    )
    .unwrap();

    assert!(*prompted.borrow());

    // verify renaming
    assert!(dir.path().join(".ignore").exists());
    assert!(!dir.path().join("file1.txt").exists());
    assert!(dir.path().join("renamed_file1.txt").exists());
    assert!(dir.path().join("file2.txt").exists());
    assert!(dir.path().join("ignored.txt").exists());
    assert!(!dir.path().join("subdir").join("file3.txt").exists());
    assert!(dir.path().join("subdir").join("renamed_file3.txt").exists());
    assert!(dir.path().join("subdir").join("file4.txt").exists());
}

/// Verify detection of duplicated file names in mapping
#[test]
fn scenario_test_detect_duplicate_target_names() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let err = bulk_rename(
        config,
        |content| Ok(content.replace("file1.txt", "file2.txt")),
        Box::new(move |_| true),
    )
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "There is a name clash in the edited files."
    );
    assert_no_files_changed(&dir);
}

/// Verify detection of invalid editing (nubmer of lines changed)
#[test]
fn scenario_test_detect_invalid_editing() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let err =
        bulk_rename(config, |_| Ok("file1".to_string()), Box::new(move |_| true)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "The number of files in the edited file does not match the original."
    );
    assert_no_files_changed(&dir);
}

/// Verify detection of directory renaming (not supported at this time)
#[test]
fn scenario_test_detect_directory_renaming() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: true,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let err = bulk_rename(
        config,
        |content| Ok(content.replace("subdir", "superdir")),
        Box::new(|_| true),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Renaming directories and moving files to other directories is currently not supported."
    );
    assert_no_files_changed(&dir);
}

/// Verify detection of a new file appearing in the directory while the program is running
#[test]
fn scenario_test_detect_changed_files() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };
    let path = dir.path().to_path_buf();

    let err = bulk_rename(
        config,
        |content| Ok(content.replace("file1.txt", "renamed_file1.txt")),
        Box::new(move |_| {
            // simulate file creation at possible moment
            File::create(path.join("renamed_file1.txt")).unwrap();
            true
        }),
    )
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "The files in the directory changed while you were editing them."
    );
    assert_no_files_changed(&dir);
}

/// Verify prevention of overwring a file that is not part of the listing (e.g. due to an .ignore file)
#[test]
fn scenario_test_detect_overwrite_of_file_not_part_of_listing() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let err = bulk_rename(
        config,
        |content| Ok(content.replace("file1.txt", "ignored.txt")),
        Box::new(|_| true),
    )
    .unwrap_err();

    assert!(err.to_string().contains("ignored.txt already exists"));
    assert_no_files_changed(&dir);
}

/// Verify prevention of overwring a file that is created during editing and would not be
/// part of the listing (e.g. due to an .ignore file)
#[test]
fn scenario_test_detect_overwrite_of_new_file_not_part_of_listing() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };
    let path = dir.path().to_path_buf();

    let err = bulk_rename(
        config,
        |content| Ok(content.replace("file1.txt", "also_ignored.txt")),
        Box::new(move |_| {
            // simulate file creation at possible moment
            File::create(path.join("also_ignored.txt")).unwrap();
            true
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("also_ignored.txt already exists"));
}

/// Verify prevention of overwring a file due to renaming order
#[test]
fn scenario_test_detect_overwrite_due_to_renaming_order() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    let err = bulk_rename(
        config,
        |content| {
            // results in illegal renaming order
            // file1.txt -> file2.tx
            // file2.txt -> file3.txt
            Ok(content
                .replace("file2.txt", "file3.txt")
                .replace("file1.txt", "file2.txt"))
        },
        Box::new(|_| true),
    )
    .unwrap_err();

    assert!(err.to_string().contains("file2.txt already exists"));
    assert_no_files_changed(&dir);
}

#[test]
fn direct_cycle_test() {
    let dir = tempdir().unwrap();

    // Create test files "a" and "b" with content "a" and "b"
    let mut file_a =
        File::create(dir.path().join("a_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let mut file_b =
        File::create(dir.path().join("b_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    writeln!(file_a, "a").unwrap();
    writeln!(file_b, "b").unwrap();

    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    // Create a direct cycle: a -> b, b -> a
    let _ = bulk_rename(
        config,
        |content| {
            Ok({
                let result = content
                    .replace(
                        "a_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "tmp_c15e4958-db22-4c10-a987-78c2a3a25562",
                    )
                    .replace(
                        "b_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "a_c15e4958-db22-4c10-a987-78c2a3a25562",
                    )
                    .replace(
                        "tmp_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "b_c15e4958-db22-4c10-a987-78c2a3a25562",
                    );
                dbg!(content, &result);
                result
            })
        },
        Box::new(|_| true),
    )
    .unwrap();

    // Check the file content after renaming
    let new_content_a =
        fs::read_to_string(dir.path().join("a_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let new_content_b =
        fs::read_to_string(dir.path().join("b_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    assert_eq!(new_content_a, "b\n");
    assert_eq!(new_content_b, "a\n");
}

#[test]
fn longer_cycle_test() {
    let dir = tempdir().unwrap();

    // Create test files "a", "b" and "c" with content "a", "b" and "c"
    let mut file_a =
        File::create(dir.path().join("a_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let mut file_b =
        File::create(dir.path().join("b_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let mut file_c =
        File::create(dir.path().join("c_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    writeln!(file_a, "a").unwrap();
    writeln!(file_b, "b").unwrap();
    writeln!(file_c, "c").unwrap();

    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    // Create a longer cycle: a -> b, b -> c, c -> a
    let _ = bulk_rename(
        config,
        |content| {
            Ok({
                let result = content
                    .replace(
                        "a_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "tmp_c15e4958-db22-4c10-a987-78c2a3a25562",
                    )
                    .replace(
                        "c_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "a_c15e4958-db22-4c10-a987-78c2a3a25562",
                    )
                    .replace(
                        "b_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "c_c15e4958-db22-4c10-a987-78c2a3a25562",
                    )
                    .replace(
                        "tmp_c15e4958-db22-4c10-a987-78c2a3a25562",
                        "b_c15e4958-db22-4c10-a987-78c2a3a25562",
                    );
                dbg!(content, &result);
                result
            })
        },
        Box::new(|_| true),
    )
    .unwrap();

    // Check the file content after renaming
    let new_content_a =
        fs::read_to_string(dir.path().join("a_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let new_content_b =
        fs::read_to_string(dir.path().join("b_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    let new_content_c =
        fs::read_to_string(dir.path().join("c_c15e4958-db22-4c10-a987-78c2a3a25562")).unwrap();
    assert_eq!(new_content_a, "c\n");
    assert_eq!(new_content_b, "a\n");
    assert_eq!(new_content_c, "b\n");
}
