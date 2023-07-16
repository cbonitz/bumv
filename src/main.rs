//! A bulk file renaming utility that uses your editor as its UI.

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use petgraph::algo::toposort;
use petgraph::graph::Graph;
use petgraph::prelude::*;
use petgraph::Directed;
use std::collections::{HashMap, HashSet};
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

struct RenamingPlan {
    request: RenamingRequest,
    steps: Vec<(PathBuf, PathBuf)>,
}

fn break_cycles(renames: HashMap<PathBuf, PathBuf>) -> Vec<(PathBuf, PathBuf)> {
    let mut graph = Graph::<PathBuf, (), Directed>::new();
    let mut nodes = HashMap::<PathBuf, NodeIndex>::new();
    let mut temp_file_counter = 0;
    let mut deferred_steps = Vec::new();

    for (old, new) in renames {
        let node_old = *nodes
            .entry(old.clone())
            .or_insert_with(|| graph.add_node(old.clone()));
        let node_new = *nodes
            .entry(new.clone())
            .or_insert_with(|| graph.add_node(new.clone()));
        graph.add_edge(node_old, node_new, ());
    }

    while let Err(cycle) = toposort(&graph, None) {
        let node_idx = cycle.node_id();
        let old_path = graph[node_idx].clone();
        let mut temp_file;
        loop {
            temp_file = old_path.with_file_name(format!(
                "{}.n{}.tmp",
                old_path.file_name().unwrap().to_str().unwrap(),
                temp_file_counter
            ));
            temp_file_counter += 1;
            if !temp_file.exists() {
                break;
            }
        }
        let edges: Vec<_> = graph.edges(node_idx).collect();
        let edge_causing_cycle = edges[0];
        let target = edge_causing_cycle.target();
        let target_path = graph[target].clone();
        println!(
            "Breaking cycle temporarily renaming {:?} to {:?}:",
            old_path, temp_file
        );
        deferred_steps.push((temp_file.clone(), target_path));
        graph.remove_edge(edge_causing_cycle.id());
        let temp_file_node = graph.add_node(temp_file.clone());
        graph.update_edge(node_idx, temp_file_node, ());
    }

    let sorted_indices = match toposort(&graph, None) {
        Ok(sorted_indices) => sorted_indices,
        Err(e) => panic!("Cycle detected even after breaking all cycles: {:?}", e),
    };

    let mut steps: Vec<_> = sorted_indices
        .into_iter()
        .filter_map(|idx| {
            let edges: Vec<_> = graph.edges(idx).collect();
            if !edges.is_empty() {
                Some((graph[idx].clone(), graph[edges[0].target()].clone()))
            } else {
                None
            }
        })
        .collect();
    steps.reverse();
    steps.append(&mut deferred_steps);

    steps
}

impl RenamingPlan {
    fn try_new(request: RenamingRequest) -> Result<Self> {
        // Using HashMap to store renaming requests
        let renames: HashMap<PathBuf, PathBuf> = request.mapping.iter().cloned().collect();

        let steps = break_cycles(renames);

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

    fn execute(&self) -> Result<String> {
        self.request.ensure_files_did_not_change()?;
        rename_files(&self.steps)?;
        Ok("Files renamed successfully.".to_string())
    }
}

/// Perform the actual renaming of the files
fn rename_files(rename_mapping: &Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for (old, new) in rename_mapping {
        if let Some(parent) = new.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
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

/// Create the content of the temp file the user will edit
fn create_editable_temp_file_content(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|f| f.to_string_lossy().to_string())
        .collect::<Vec<String>>()
        .join("\n")
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

struct RenamingRequest {
    config: BumvConfiguration,
    all_files_at_creation_time: Vec<PathBuf>,
    mapping: Vec<(PathBuf, PathBuf)>,
}

impl RenamingRequest {
    fn try_new<F: FnOnce(String) -> Result<String>>(
        config: BumvConfiguration,
        edit_function: F,
    ) -> Result<Self> {
        let original_filenames = config.file_list();
        let temp_file_content = create_editable_temp_file_content(&original_filenames);
        let modified_temp_file_content = edit_function(temp_file_content)?;
        let edited_filenames = parse_temp_file_content(modified_temp_file_content);
        if original_filenames.len() != edited_filenames.len() {
            anyhow::bail!("The number of files in the edited file does not match the original.");
        }
        let unique_new_filenames: HashSet<&PathBuf> = edited_filenames.iter().collect();
        if unique_new_filenames.len() != edited_filenames.len() {
            anyhow::bail!("There is a name clash in the edited files.");
        }

        let mapping: Vec<(PathBuf, PathBuf)> = original_filenames
            .iter()
            .zip(edited_filenames.iter())
            .filter(|(old, new)| old != new)
            .map(|(old, new)| (old.clone(), new.clone()))
            .collect();
        Ok(Self {
            config,
            all_files_at_creation_time: original_filenames,
            mapping,
        })
    }

    fn is_empty(&self) -> bool {
        self.mapping.is_empty()
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

struct TempFileEditor {
    editor_name: String,
}

impl TempFileEditor {
    /// Write the content of the temp file the user will edit
    fn write_editable_temp_file(content: String) -> Result<NamedTempFile> {
        let mut temp_file = NamedTempFile::new()?;
        write!(temp_file, "{}", content)?;
        Ok(temp_file)
    }

    /// Let the user edit the temp file
    fn let_user_edit_temp_file(&self, temp_file: &NamedTempFile) -> Result<()> {
        let temp_path = temp_file
            .path()
            .to_str()
            .context("Failed to convert path to string")?;
        let mut command = Command::new(&self.editor_name);
        // VS code needs the --wait flag to wait for the user to close the editor
        if self.editor_name == "code" {
            command.arg("--wait");
        }
        let status = command.arg(temp_path).status()?;
        anyhow::ensure!(status.success(), "Editor exited with an error");
        Ok(())
    }

    /// Read the temp file the user edited and parse the content
    fn read_temp_file(temp_file: &NamedTempFile) -> Result<String> {
        let mut content = String::new();
        File::open(temp_file.path())?.read_to_string(&mut content)?;
        Ok(content)
    }

    fn edit(&self, content: String) -> Result<String> {
        let temp_file = Self::write_editable_temp_file(content)?;
        self.let_user_edit_temp_file(&temp_file)?;
        Self::read_temp_file(&temp_file)
    }
}

/// Bulk rename files according to the configuration
/// `edit_function` and `prompt_function` are passed as parameters to allow for testing.
fn bulk_rename(
    config: BumvConfiguration,
    edit_function: impl Fn(String) -> Result<String>,
    prompt_function: impl FnOnce(String) -> bool,
) -> Result<()> {
    let request = RenamingRequest::try_new(config, edit_function)?;

    let plan = RenamingPlan::try_new(request)?;

    if !plan.is_empty() {
        let human_readable_mapping = plan.human_readable_rename_mapping();
        if prompt_function(human_readable_mapping) {
            println!("{}", plan.execute()?);
        } else {
            println!("Aborted.")
        }
    } else {
        println!("No files to rename.");
    }
    Ok(())
}

/// Prompt the user for confirmation
fn prompt_for_confirmation(human_readable_mapping: String) -> bool {
    println!("{}", human_readable_mapping);
    let input: String = rprompt::prompt_reply("\nRename: [Y/n]? ").unwrap();
    matches!(input.to_lowercase().as_str(), "y" | "")
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

    let editor = TempFileEditor { editor_name };

    bulk_rename(
        config,
        move |content| editor.edit(content),
        prompt_for_confirmation,
    )
}

#[cfg(test)]
mod tests;
