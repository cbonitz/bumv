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

#[cfg(target_os = "windows")]
const VS_CODE: &str = "code.cmd";

#[cfg(not(target_os = "windows"))]
const VS_CODE: &str = "code";

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
    /// Do not write a log file
    #[structopt(long)]
    no_log: bool,
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

/// Break cycles in the rename mapping by temporarily renaming files if necessary,
/// and finds a conflict-free ordering of the renaming steps.
fn break_cycles_and_fix_ordering(renames: HashMap<PathBuf, PathBuf>) -> Vec<(PathBuf, PathBuf)> {
    // The algorithm views the renaming mappings as a directed graph.
    // It then tries to create a topological ordering of the graph.
    // If a cycle is found, it temporarily renames one of the files in the cycle.
    // This is repeated until the graph is cycle free.
    // The resulting topological ordering is then reversed to get the correct order of the renaming steps.
    // Then, the missing renames of temporary files are added to the end of the list.

    // For example a -> b, b -> a is a cycle. Therefore, Topological ordering will fail.
    // The algorithm will choose one of the files in the cycle, for example a.
    // It will remove the edge a -> b and add the edge a -> a.tmp instead.
    // It will remember new renaming step of a.tmp -> b by storing it in a list of deferred steps.
    // Now the remaining graph b -> a, a -> a.tmp is cycle free.
    // The reversed topological ordering as per the `petrgraph` library is a -> a.tmp, b -> a,
    // which is exactly the order that will work for the renaming process.
    // To complete the list of renamings, the deferred step a.tmp -> b is added to the end of the list,
    // resulting in a -> a.tmp, b -> a, a.tmp -> b.

    let mut graph = Graph::<PathBuf, (), Directed>::new();
    let mut nodes = HashMap::<PathBuf, NodeIndex>::new();
    let mut temp_file_counter = 0;
    let mut deferred_steps = Vec::new();

    // Create the initial graph
    for (old, new) in renames {
        let node_old = *nodes
            .entry(old.clone())
            .or_insert_with(|| graph.add_node(old.clone()));
        let node_new = *nodes
            .entry(new.clone())
            .or_insert_with(|| graph.add_node(new.clone()));
        graph.add_edge(node_old, node_new, ());
    }

    // Attempt topological sorting
    while let Err(cycle) = toposort(&graph, None) {
        let node_idx = cycle.node_id();
        let source_file = graph[node_idx].clone();
        // Create a temp file name that makes sense to a human if renaming fails at any point
        // and which is deterministic for testing.
        let mut temp_file;
        loop {
            temp_file = source_file.with_file_name(format!(
                "{}.n{}.tmp",
                source_file.file_name().unwrap().to_str().unwrap(),
                temp_file_counter
            ));
            temp_file_counter += 1;
            if !temp_file.exists() {
                break;
            }
        }
        // Remove the original renaming, add the renaming of the source file to the temporary file
        // and defer the renaming of the temporary file to its target.
        let edges: Vec<_> = graph.edges(node_idx).collect();
        let edge_causing_cycle = edges[0];
        let target = edge_causing_cycle.target();
        let target_path = graph[target].clone();
        println!(
            "Breaking cycle temporarily renaming {:?} to {:?}:",
            source_file, temp_file
        );
        graph.remove_edge(edge_causing_cycle.id());
        let temp_file_node = graph.add_node(temp_file.clone());
        graph.update_edge(node_idx, temp_file_node, ());
        deferred_steps.push((temp_file.clone(), target_path));
    }

    // Topological sorting succeeded, so the graph must be cycle free.
    let sorted_indices = match toposort(&graph, None) {
        Ok(sorted_indices) => sorted_indices,
        Err(e) => panic!("Cycle detected even after breaking all cycles: {:?}", e),
    };

    // Turn graph back into a list of renaming steps
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
    // Reverse the ordering to get the correct ordering for executing the renamings.
    steps.reverse();
    // Now add the deferred steps. Their relative order does not matter.
    steps.append(&mut deferred_steps);

    steps
}

impl RenamingPlan {
    fn try_new(request: RenamingRequest) -> Result<Self> {
        // Using HashMap to store renaming requests
        let renames: HashMap<PathBuf, PathBuf> = request.mapping.iter().cloned().collect();

        let steps = break_cycles_and_fix_ordering(renames);

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
        if !self.request.config.no_log {
            self.request.write_renaming_log_file();
        }
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

    // Create a logfile called bumv_{timestamp}.log in the base path of the renaming request containing
    // the requested renaming mapping.
    // The log file is based on the request, because the user is not interested in the temporary files
    // created in the planning phase.
    fn write_renaming_log_file(&self) {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let log_file_name = format!("bumv_{}.log", timestamp);
        // set the log file path to the base path of the renaming request
        // or the current directory if none is specified.
        let log_file_path = self
            .config
            .base_path
            .clone()
            .unwrap_or_else(|| Path::new(".").to_path_buf())
            .join(log_file_name);
        let mut log_file = File::create(log_file_path).unwrap();
        // format the rename mapping to be tab separated, with nicely aligned columns
        // first compute the longest lenght of the old filenames, then use this information
        // for indentation
        let max_old_filename_length = self
            .mapping
            .iter()
            .map(|(old, _)| old.to_string_lossy().len())
            .max()
            .unwrap();
        // create the log content
        let log_content = self
            .mapping
            .iter()
            .map(|(old, new)| {
                format!(
                    "{:width$}\t{}",
                    old.to_string_lossy(),
                    new.to_string_lossy(),
                    width = max_old_filename_length
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        log_file.write_all(log_content.as_bytes()).unwrap();
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
        if self.editor_name == VS_CODE {
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
        (true, _) => VS_CODE.to_string(),
        (false, Ok(editor)) => editor,
        // default to VS code
        (false, Err(_)) => VS_CODE.to_string(),
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
