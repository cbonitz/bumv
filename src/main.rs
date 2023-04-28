//! A bulk file renaming utility that uses your editor as its UI.
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
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
            .into_iter()
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
            .into_iter()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| path.is_file())
            .collect(),
    ))
}

/// Create the content of the temp file the user will edit
fn create_editable_temp_file_content(files: &[PathBuf]) -> String {
    files
        .into_iter()
        .map(|f| f.to_string_lossy().to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

/// Write the content of the temp file the user will edit
fn write_editable_temp_file(files: &[PathBuf]) -> Result<NamedTempFile> {
    let mut temp_file = NamedTempFile::new()?;
    write!(temp_file, "{}", create_editable_temp_file_content(files))?;
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
fn read_temp_file(temp_file: &NamedTempFile) -> Result<Vec<PathBuf>> {
    let mut content = String::new();
    File::open(temp_file.path())?.read_to_string(&mut content)?;
    Ok(parse_temp_file_content(content))
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

    Ok(files
        .iter()
        .zip(new_files.iter())
        .filter(|(old, new)| old != new)
        .collect())
}

/// Create a human readable representation of the rename mapping
fn create_human_readable_rename_mapping(rename_mapping: &Vec<(&PathBuf, &PathBuf)>) -> String {
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
        "The files changed while you were editing them."
    );
    Ok(())
}

/// Read the files in `base_path` according to the configuration
fn read_files(opt: &BumvConfiguration) -> Result<Vec<PathBuf>> {
    let base_path = opt
        .base_path
        .as_ref()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| Path::new("."));
    if opt.recursive {
        read_directory_files_recursive(base_path, opt.no_ignore)
    } else {
        read_directory_files(base_path, opt.no_ignore)
    }
}
fn main() -> Result<()> {
    let opt = BumvConfiguration::from_args();

    let files = read_files(&opt)?;

    let temp_file = write_editable_temp_file(&files)?;
    let_user_edit_temp_file(&temp_file)?;
    let new_files = read_temp_file(&temp_file)?;

    let rename_mapping = create_rename_mapping(&files, &new_files)?;

    if !rename_mapping.is_empty() {
        println!("{}", create_human_readable_rename_mapping(&rename_mapping));
        let input = rprompt::prompt_reply("\nRename: [Y/n]? ").unwrap();
        if input.to_lowercase() != "n" {
            let current_files = read_files(&opt)?;
            match ensure_files_did_not_change(&files, &current_files) {
                Ok(_) => {}
                Err(e) => {
                    println!("{}", e);
                    exit(1);
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

#[cfg(test)]
mod tests;
