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
    about = "A bulk file renaming utility that uses your editor as its UI. Invoke the utility, edit the filenames, save the temporary file, close the editor and confirm changes."
)]
struct Opt {
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

fn sort_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| path.to_string_lossy().to_string());
    paths
}

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

fn create_temp_file_content(files: &[PathBuf]) -> String {
    files
        .into_iter()
        .map(|f| f.to_string_lossy().to_string())
        .collect::<Vec<String>>()
        .join("\n")
}

fn write_temp_file(files: &[PathBuf]) -> Result<NamedTempFile> {
    let mut temp_file = NamedTempFile::new()?;
    writeln!(temp_file, "{}", create_temp_file_content(files))?;
    Ok(temp_file)
}

fn edit_temp_file(temp_file: &NamedTempFile) -> Result<()> {
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

fn read_temp_file(temp_file: &NamedTempFile) -> Result<Vec<PathBuf>> {
    let mut content = String::new();
    File::open(temp_file.path())?.read_to_string(&mut content)?;
    Ok(parse_temp_file_content(content))
}

fn parse_temp_file_content(content: String) -> Vec<PathBuf> {
    content
        .lines()
        // skip empty lines (usually the last line)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn create_rename_mapping<'a>(
    files: &'a [PathBuf],
    new_files: &'a [PathBuf],
) -> Result<Vec<(&'a PathBuf, &'a PathBuf)>> {
    if files.len() != new_files.len() {
        anyhow::bail!("The number of files in the edited file does not match the original.");
    }

    let unique_new_files: HashSet<&PathBuf> = new_files.iter().collect();
    if unique_new_files.len() != new_files.len() {
        anyhow::bail!("There is a name clash in the edited file.");
    }

    Ok(files
        .iter()
        .zip(new_files.iter())
        .filter(|(old, new)| old != new)
        .collect())
}

fn create_rename_prompt(rename_mapping: &Vec<(&PathBuf, &PathBuf)>) -> String {
    rename_mapping
        .iter()
        .map(|(old, new)| format!("{} -> {}", old.to_string_lossy(), new.to_string_lossy()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rename_files(rename_mapping: &Vec<(&PathBuf, &PathBuf)>) -> Result<()> {
    for (old, new) in rename_mapping {
        fs::rename(old, new)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    let base_path = opt
        .base_path
        .as_ref()
        .map(PathBuf::as_path)
        .unwrap_or_else(|| Path::new("."));

    let files = if opt.recursive {
        read_directory_files_recursive(base_path, opt.no_ignore)?
    } else {
        read_directory_files(base_path, opt.no_ignore)?
    };

    let temp_file = write_temp_file(&files)?;
    edit_temp_file(&temp_file)?;
    let new_files = read_temp_file(&temp_file)?;

    let rename_mapping = create_rename_mapping(&files, &new_files)?;

    if !rename_mapping.is_empty() {
        println!("{}", create_rename_prompt(&rename_mapping));
        let input = rprompt::prompt_reply("\nRename: [Y/n]? ").unwrap();
        if input.to_lowercase() != "n" {
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
