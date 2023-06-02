# bumv, a Bulk File Renaming Utility

`bumv` (bulk move) lets you use your favorite editor to rename files.
It was created becuase the author found bulk file renaming very tedious compared to editing strings in modern editors. Editors provide powerful search and replace functionality as well as multi cursor editing, all of which are helpful for bulk file renaming.

## Usage

By default, `bumv` will let you rename the files in the current directory non-recursively, respecting git ignore definitions and `.ignore` files.
Invoked on this project directory, it would open the following list of files in `EDITOR` (defaulting to VS Code):

```
./Cargo.lock
./Cargo.toml
./LICENSE.txt
./README.md
```

Assume you edit the file as follows, save it and close your editor:

```
./Cargo.lock
./Cargo.toml
./LICENSE.txt
./README_CAREFULLY.md
```

`bumv` will prompt you for confirmation and then rename `README.md` to `README_CAREFULLY.md`.

### Warning

Race conditions or unforseen edge cases could lead to undesired behavior. Use at your own risk and only on files you have backed up.

### Safety checks

The following safety checks are implemented and tested:

- Inputs are only accepted if they have the same number of lines.
- Inputs that will obviously lead to overwriting of files are rejected right away.
- To avoid overwriting existing of files due to race conditions or renaming order, `bumv` verifies before each renaming operation that a file with the target filename does not exist.
- Before renaming is performed, `bumv` verifies that the file list presented to the user still exactly matches what is present on the file system.

### Notes

- If a part of the parent directory hierarchy of a file is changed when editing the mapping, the file will be moved to the specified location, but empty directories will not be deleted.
- If a renaming would lead to a conflict if done naively, e.g. `file1 <-> file2`, a temporary file will be used to enable the renaming.

### Options

```
-n, --no-ignore    Do not observe ignore files
-r, --recursive    Recursively rename files in subdirectories
```

## Installation

`cargo install bumv`

## About This Project

This project was done as a playground for AI-assisted programming. The original code was written by ChatGPT 4 based on a loose specification and surprisingly both compiled and worked right away.

The generated unit tests were insufficient. Thus, the code had to be refactored for testability, and a lot of tests were written by hand.

Several safeguards and improvements had to be added manually based on outcomes of local testing and thinking carefully about program behavior.

GitHub Copilot was very helpful when extending the code and adding comments.
