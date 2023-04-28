//! A bulk file renaming utility that uses your editor as its UI.
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use structopt::StructOpt;
use tempfile::NamedTempFile;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "bumv",
    about = "bumv (bulk move) - A bulk file renaming utility that uses your editor as its UI. Invoke the utility, edit the filenames, save the temporary file, close the editor and confirm changes."
)]
struct BumvConfiguration {
    /// Recursively rename files in subdirectories
    #[structopt(short, long)]
    recursive: bool,
    /// Do not observe ignore files
    #[structopt(short, long)]
    no_ignore: bool,
    /// Base path for the operation
    #[structopt(parse(from_os_str))]
    base_path: Option<PathBuf>,
}

/// Deterministically sort paths
fn sort_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| path.to_string_lossy().to_string());
    paths
}

/// Read the files in `base_path` non-recursively, optionally ignoring ignore files
fn read_directory_files(base_path: &Path, no_ignore_files: bool) -> Result<Vec<PathBuf>> {
    Ok(sort_paths(
        WalkBuilder::new(base_path)
            .standard_filters(!no_ignore_files)
            .build()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| path.is_file())
            .filter(|path| path.parent() == Some(base_path))
            .collect(),
    ))
}

/// read the files in `base_path` recursively, optionally ignoring ignore files
fn read_directory_files_recursive(base_path: &Path, no_ignore_files: bool) -> Result<Vec<PathBuf>> {
    Ok(sort_paths(
        WalkBuilder::new(base_path)
            .standard_filters(!no_ignore_files)
            .build()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| path.is_file())
            .collect(),
    ))
}

/// Create the content of the temp file the user will edit
fn create_editable_temp_file_content(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| f.to_string_lossy().to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

/// Write the content of the temp file the user will edit
fn write_editable_temp_file(content: String) -> Result<NamedTempFile> {
    let mut temp_file = NamedTempFile::new()?;
    write!(temp_file, "{}", content)?;
    Ok(temp_file)
}

/// Let the user edit the temp file
fn let_user_edit_temp_file(temp_file: &NamedTempFile) -> Result<()> {
    let editor = std::env::var("EDITOR");
    let temp_path = temp_file
        .path()
        .to_str()
        .context("Failed to convert path to string")?;
    let status = match editor {
        Ok(editor) => Command::new(editor).arg(temp_path).status().unwrap(),
        // the author loves VS Code's multi cursor editing
        Err(_) => Command::new("code")
            .arg("--wait")
            .arg(temp_path)
            .status()
            .unwrap(),
    };
    anyhow::ensure!(status.success(), "Editor exited with an error");
    Ok(())
}

/// Read the temp file the user edited and parse the content
fn read_temp_file(temp_file: &NamedTempFile) -> Result<String> {
    let mut content = String::new();
    File::open(temp_file.path())?.read_to_string(&mut content)?;
    Ok(content)
}

/// Parse the content of the temp file the user edited
fn parse_temp_file_content(content: String) -> Vec<PathBuf> {
    content
        .lines()
        // skip empty lines (usually the last line)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Create a mapping from old to new filenames
fn create_rename_mapping<'a>(
    files: &'a [PathBuf],
    new_files: &'a [PathBuf],
) -> Result<Vec<(&'a PathBuf, &'a PathBuf)>> {
    if files.len() != new_files.len() {
        anyhow::bail!("The number of files in the edited file does not match the original.");
    }

    let unique_new_files: HashSet<&PathBuf> = new_files.iter().collect();
    if unique_new_files.len() != new_files.len() {
        anyhow::bail!("There is a name clash in the edited files.");
    }

    let result: Vec<_> = files
        .iter()
        .zip(new_files.iter())
        .filter(|(old, new)| old != new)
        .collect();

    for (old, new) in &result {
        if old.parent() != new.parent() {
            anyhow::bail!(
                "Renaming directories and moving files to other directories is currently not supported.",
            );
        }
    }

    Ok(result)
}

/// Create a human readable representation of the rename mapping
fn create_human_readable_rename_mapping(rename_mapping: &[(&PathBuf, &PathBuf)]) -> String {
    rename_mapping
        .iter()
        .map(|(old, new)| format!("{} -> {}", old.to_string_lossy(), new.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Perform the actual renaming of the files
fn rename_files(rename_mapping: &Vec<(&PathBuf, &PathBuf)>) -> Result<()> {
    for (old, new) in rename_mapping {
        if new.exists() {
            anyhow::bail!(
                "The file {} already exists. Aborting.",
                new.to_string_lossy()
            );
        }
        fs::rename(old, new)?;
    }
    Ok(())
}

/// Ensure that the files did not change while the user was editing them
fn ensure_files_did_not_change(
    previous_files: &[PathBuf],
    current_files: &[PathBuf],
) -> Result<()> {
    let mut files = previous_files.to_vec();
    let mut new_files = current_files.to_vec();
    files.sort();
    new_files.sort();
    anyhow::ensure!(
        files == new_files,
        "The files in the directory changed while you were editing them."
    );
    Ok(())
}

/// Read the files in `base_path` according to the configuration
fn read_files(config: &BumvConfiguration) -> Result<Vec<PathBuf>> {
    let base_path = config
        .base_path
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    if config.recursive {
        read_directory_files_recursive(base_path, config.no_ignore)
    } else {
        read_directory_files(base_path, config.no_ignore)
    }
}

/// Prompt the user for confirmation
fn prompt_for_confirmation(human_readable_mapping: String) -> bool {
    println!("{}", human_readable_mapping);
    let input = rprompt::prompt_reply("\nRename: [Y/n]? ").unwrap();
    matches!(input.to_lowercase().as_str(), "y" | "")
}

/// Edit the files in a temp file and return the modified content
fn edit_files_in_temp_file(temp_file_content: String) -> Result<String> {
    let temp_file = write_editable_temp_file(temp_file_content)?;
    let_user_edit_temp_file(&temp_file)?;
    read_temp_file(&temp_file)
}

/// Bulk rename files according to the configuration
/// `edit_function` and `prompt_function` are passed as parameters to allow for testing.
fn bulk_rename(
    config: BumvConfiguration,
    edit_function: fn(String) -> Result<String>,
    prompt_function: Box<dyn FnOnce(String) -> bool>,
) -> Result<()> {
    let files = read_files(&config)?;
    let temp_file_content = create_editable_temp_file_content(&files);
    let modified_temp_file_content = edit_function(temp_file_content)?;
    let new_files = parse_temp_file_content(modified_temp_file_content);

    let rename_mapping = create_rename_mapping(&files, &new_files)?;

    if !rename_mapping.is_empty() {
        let human_readable_mapping = create_human_readable_rename_mapping(&rename_mapping);
        if prompt_function(human_readable_mapping) {
            let current_files = read_files(&config)?;
            match ensure_files_did_not_change(&files, &current_files) {
                Ok(_) => {}
                Err(e) => {
                    println!("{}", e);
                    return Err(e);
                }
            };
            rename_files(&rename_mapping)?;
            println!("Files renamed successfully.");
        } else {
            println!("Aborted.")
        }
    } else {
        println!("No files to rename.");
    }
    Ok(())
}
fn main() -> Result<()> {
    let config = BumvConfiguration::from_args();
    bulk_rename(
        config,
        edit_files_in_temp_file,
        Box::new(prompt_for_confirmation),
    )
}

#[cfg(test)]
mod tests;
