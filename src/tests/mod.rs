use crate::{bulk_rename, create_editable_temp_file_content, BumvConfiguration};
use std::{
    cell::RefCell,
    fs::{self, File},
    io::Write,
    rc::Rc,
};
use tempfile::{tempdir, TempDir};

fn prompt_function(prompt: String) -> bool {
    println!("prompt:\n{}", prompt);
    true
}

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
    let mut file1 = File::create(file1).unwrap();
    write!(file1, "file1_content").unwrap();
    let mut file2 = File::create(file2).unwrap();
    write!(file2, "file2_content").unwrap();
    File::create(ignored).unwrap();
    let mut file3 = File::create(file3).unwrap();
    write!(file3, "file3_content").unwrap();
    File::create(file4).unwrap();
}

fn assert_no_filenames_changed(dir: &TempDir) {
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
            println!("prompt:\n{}", prompt);
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
    // file1.txt -> renamed_file2.txt
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
            println!("prompt:\n{}", prompt);
            // make test robust to unstable topological sort
            let (rename_prompt_1, rename_prompt_2) = {
                let (rename_prompt_a, rename_prompt_b) = prompt.split_once('\n').unwrap();
                if rename_prompt_a.contains("renamed_file1") {
                    (rename_prompt_a, rename_prompt_b)
                } else {
                    (rename_prompt_b, rename_prompt_a)
                }
            };

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
        Box::new(prompt_function),
    )
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "There is a name clash in the edited files."
    );
    assert_no_filenames_changed(&dir);
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

    let err = bulk_rename(
        config,
        |_| Ok("file1".to_string()),
        Box::new(prompt_function),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "The number of files in the edited file does not match the original."
    );
    assert_no_filenames_changed(&dir);
}

/// Verify "directory renaming", i.e. creation of new parent directories
/// Old parent dirs are left empty
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

    bulk_rename(
        config,
        |content| Ok(content.replace("subdir", "superdir")),
        Box::new(prompt_function),
    )
    .unwrap();

    assert!(dir.path().join(".ignore").exists());
    assert!(dir.path().join("file1.txt").exists());
    assert!(dir.path().join("file2.txt").exists());
    assert!(dir.path().join("ignored.txt").exists());
    // files moved from subdir to new superdir
    assert!(!dir.path().join("subdir").join("file3.txt").exists());
    assert!(!dir.path().join("subdir").join("file4.txt").exists());
    assert!(dir.path().join("superdir").join("file3.txt").exists());
    assert!(dir.path().join("superdir").join("file4.txt").exists());
    // old directory remains
    assert!(dir.path().join("subdir").exists());
    assert!(dir.path().join("subdir").exists());
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
        Box::new(move |prompt| {
            println!("prompt:\n{}", prompt);
            // simulate file creation at the worst possible moment
            File::create(path.join("renamed_file1.txt")).unwrap();
            true
        }),
    )
    .unwrap_err();

    assert_eq!(
        err.to_string(),
        "The files in the directory changed while you were editing them."
    );
    assert_no_filenames_changed(&dir);
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
        Box::new(prompt_function),
    )
    .unwrap_err();

    assert!(err.to_string().contains("ignored.txt already exists"));
    assert_no_filenames_changed(&dir);
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
        Box::new(move |prompt| {
            println!("prompt:\n{}", prompt);
            // simulate file creation at the worst possible moment
            File::create(path.join("also_ignored.txt")).unwrap();
            true
        }),
    )
    .unwrap_err();

    assert!(err.to_string().contains("also_ignored.txt already exists"));
}

/// Verify that renaming order is fixed
#[test]
fn scenario_test_detect_fix_renaming_order() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);
    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    bulk_rename(
        config,
        |content| {
            Ok(content
                .replace("file2.txt", "file3.txt")
                .replace("file1.txt", "file2.txt"))
        },
        Box::new(prompt_function),
    )
    .unwrap();

    assert!(dir.path().join(".ignore").exists());
    // file1.txt -> file2.txt
    assert!(!dir.path().join("file1.txt").exists());
    assert!(dir.path().join("file2.txt").exists());
    let new_content_file2 = fs::read_to_string(dir.path().join("file2.txt")).unwrap();
    assert_eq!(new_content_file2, "file1_content");
    // file2.txt -> file3.txt
    assert!(dir.path().join("file3.txt").exists());
    let new_content_file3 = fs::read_to_string(dir.path().join("file3.txt")).unwrap();
    assert_eq!(new_content_file3, "file2_content");
    assert!(dir.path().join("ignored.txt").exists());
    assert!(dir.path().join("subdir").join("file3.txt").exists());
    assert!(dir.path().join("subdir").join("file4.txt").exists());
}

#[test]
fn direct_cycle_test() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let config = BumvConfiguration {
        recursive: false,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    // Create a direct cycle: file1.txt -> file2.txt, file2.txt -> file1.txt
    let _ = bulk_rename(
        config,
        |content| {
            Ok({
                let result = content
                    .replace("file1.txt", "some_temporary_string")
                    .replace("file2.txt", "file1.txt")
                    .replace("some_temporary_string", "file2.txt");
                dbg!(content, &result);
                result
            })
        },
        Box::new(prompt_function),
    )
    .unwrap();

    assert_no_filenames_changed(&dir);
    // Check the file content after renaming
    let new_content_file1 = fs::read_to_string(dir.path().join("file1.txt")).unwrap();
    let new_contents_file2 = fs::read_to_string(dir.path().join("file2.txt")).unwrap();
    assert_eq!(new_content_file1, "file2_content");
    assert_eq!(new_contents_file2, "file1_content");
}

#[test]
fn longer_cycle_test() {
    let dir = tempdir().unwrap();
    create_test_files(&dir);

    let config = BumvConfiguration {
        recursive: true,
        no_ignore: false,
        use_vscode: false,
        base_path: Some(dir.path().to_path_buf()),
    };

    // Create a longer cycle: file1.txt -> file2.txt, file2.txt -> file3.txt, file3.txt -> file1.txt
    let _ = bulk_rename(
        config,
        |content| {
            Ok({
                let result = content
                    .replace("file1.txt", "some_temporary_string")
                    .replace("subdir/file3.txt", "file1.txt")
                    .replace("file2.txt", "subdir/file3.txt")
                    .replace("some_temporary_string", "file2.txt");
                dbg!(content, &result);
                result
            })
        },
        Box::new(prompt_function),
    )
    .unwrap();

    assert_no_filenames_changed(&dir);
    // Check the file content after renaming
    let new_content_file1 = fs::read_to_string(dir.path().join("file1.txt")).unwrap();
    let new_content_file2 = fs::read_to_string(dir.path().join("file2.txt")).unwrap();
    let new_content_file3 =
        fs::read_to_string(dir.path().join("subdir").join("file3.txt")).unwrap();
    assert_eq!(new_content_file1, "file3_content");
    assert_eq!(new_content_file2, "file1_content");
    assert_eq!(new_content_file3, "file2_content");
}
