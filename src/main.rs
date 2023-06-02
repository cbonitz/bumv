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

#[derive(StructOpt, Debug, Clone)]
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
    /// Use VS Code as editor
    #[structopt(short = "c", long)]
    use_vscode: bool,
    /// Base path for the operation
    #[structopt(parse(from_os_str))]
    base_path: Option<PathBuf>,
}

impl BumvConfiguration {
    fn file_list(&self) -> Vec<PathBuf> {
        let base_path = self.base_path.as_deref().unwrap_or_else(|| Path::new("."));
        let builder = WalkBuilder::new(base_path)
            .standard_filters(!self.no_ignore)
            .build()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .filter(|path| path.is_file());
        let mut result: Vec<_> = if !self.recursive {
            // non-recursive mode: only include files in the base path
            builder
                .filter(|path| path.parent() == Some(base_path))
                .collect()
        } else {
            builder.collect()
        };
        // ensure deterministic order
        result.sort_by_key(|path| path.to_string_lossy().to_string());
        result
    }
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
fn let_user_edit_temp_file(temp_file: &NamedTempFile, editor_name: String) -> Result<()> {
    let temp_path = temp_file
        .path()
        .to_str()
        .context("Failed to convert path to string")?;
    let mut command = Command::new(editor_name.clone());
    // VS code needs the --wait flag to wait for the user to close the editor
    if editor_name == "code" {
        command.arg("--wait");
    }
    let status = command.arg(temp_path).status().unwrap();
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

struct RenamingPlan {
    request: RenamingRequest,
    steps: Vec<(PathBuf, PathBuf)>,
}

impl RenamingPlan {
    fn try_new(request: RenamingRequest) -> Result<Self> {
        for (old, new) in request.mapping.iter() {
            if old.parent() != new.parent() {
                anyhow::bail!(
                    "Renaming directories and moving files to other directories is currently not supported.",
                );
            }
        }
        let steps: Vec<(PathBuf, PathBuf)> = request
            .mapping
            .iter()
            .map(|(f, t)| (f.clone(), t.clone()))
            .collect();

        Ok(RenamingPlan { request, steps })
    }

    fn is_empty(&self) -> bool {
        self.request.is_empty()
    }

    /// Create a human readable representation of the rename mapping
    fn human_readable_rename_mapping(&self) -> String {
        self.steps
            .iter()
            .map(|(old, new)| format!("{} -> {}", old.to_string_lossy(), new.to_string_lossy()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn execute(&self) -> Result<()> {
        self.request.ensure_files_did_not_change()?;
        rename_files(&self.steps)?;
        println!("Files renamed successfully.");
        Ok(())
    }
}

/// Perform the actual renaming of the files
fn rename_files(rename_mapping: &Vec<(PathBuf, PathBuf)>) -> Result<()> {
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

/// Prompt the user for confirmation
fn prompt_for_confirmation(human_readable_mapping: String) -> bool {
    println!("{}", human_readable_mapping);
    let input = rprompt::prompt_reply("\nRename: [Y/n]? ").unwrap();
    matches!(input.to_lowercase().as_str(), "y" | "")
}

/// Edit the files in a temp file and return the modified content
fn edit_files_in_temp_file(temp_file_content: String, editor_name: String) -> Result<String> {
    let temp_file = write_editable_temp_file(temp_file_content)?;
    let_user_edit_temp_file(&temp_file, editor_name)?;
    read_temp_file(&temp_file)
}

struct RenamingRequest {
    config: BumvConfiguration,
    all_files_at_creation_time: Vec<PathBuf>,
    mapping: Vec<(PathBuf, PathBuf)>,
}

impl RenamingRequest {
    fn try_new(
        config: BumvConfiguration,
        editor_name: String,
        edit_function: fn(String, String) -> Result<String>,
    ) -> Result<Self> {
        let from = config.file_list();
        let temp_file_content = create_editable_temp_file_content(&from);
        let modified_temp_file_content = edit_function(temp_file_content, editor_name)?;
        let to = parse_temp_file_content(modified_temp_file_content);
        if from.len() != to.len() {
            anyhow::bail!("The number of files in the edited file does not match the original.");
        }
        let unique_new_files: HashSet<&PathBuf> = to.iter().collect();
        if unique_new_files.len() != to.len() {
            anyhow::bail!("There is a name clash in the edited files.");
        }

        let mapping: Vec<(PathBuf, PathBuf)> = from
            .iter()
            .zip(to.iter())
            .filter(|(old, new)| old != new)
            .map(|(old, new)| (old.clone(), new.clone()))
            .collect();
        Ok(Self {
            config,
            all_files_at_creation_time: from,
            mapping,
        })
    }

    fn is_empty(&self) -> bool {
        return self.mapping.is_empty();
    }

    /// Ensure that the files have not changed since this request was created
    fn ensure_files_did_not_change(&self) -> Result<()> {
        anyhow::ensure!(
            self.all_files_at_creation_time == self.config.file_list(),
            "The files in the directory changed while you were editing them."
        );
        Ok(())
    }
}

/// Bulk rename files according to the configuration
/// `edit_function` and `prompt_function` are passed as parameters to allow for testing.
fn bulk_rename(
    config: BumvConfiguration,
    editor_name: String,
    edit_function: fn(String, String) -> Result<String>,
    prompt_function: Box<dyn FnOnce(String) -> bool>,
) -> Result<()> {
    let request = RenamingRequest::try_new(config, editor_name, edit_function)?;

    let plan = RenamingPlan::try_new(request)?;

    if !plan.is_empty() {
        let human_readable_mapping = plan.human_readable_rename_mapping();
        if prompt_function(human_readable_mapping) {
            plan.execute()?;
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
    let editor_var = std::env::var("EDITOR");
    let editor_name = match (config.use_vscode, editor_var) {
        (true, _) => "code".to_string(),
        (false, Ok(editor)) => editor,
        // default to VS code
        (false, Err(_)) => "code".to_string(),
    };

    bulk_rename(
        config,
        editor_name,
        edit_files_in_temp_file,
        Box::new(prompt_for_confirmation),
    )
}

#[cfg(test)]
mod tests;
