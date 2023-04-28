# bumv, a Bulk File Renaming Utility

`bumv` (bulk move) lets you use your favorite editor to rename files.
It was created becuase the author found bulk file renaming very tedious compared to editing strings in modern editors. Editorsh provide powerful search and replace functionality as well as multi cursor editing, all of which are helpful for bulk file renaming.

# Usage

By default, `bumv` will let you rename the files in the current directory non-recursively.
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

## Warning

While `bumv` checks that no files will be overwritten and that the filenames have not changed between listing the files and doing the rename, race conditions or unforseen edge cases could lead to undesired behavior. Use at your own risk and only on files you have backed up.

# Installation

`cargo install bumv`

# Colophonium

This project was done as a playground for AI-assisted programming. The original code was written by ChatGPT 4 based on a loose specification and surprisingly both compiled and worked right away.

The generated unit tests only covered insignificant parts of the code. Thus, the code had to be refactored for testability, and a lot of tests were written by hand.

Several safeguards and improvements had to be added manually based on outcomes of local testing and thinking carefully about program behavior.

GitHub Copilot was very helpful when extending the code and adding comments.
