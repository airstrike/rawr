# Rawr for VS Code

This extension is for Rawr files: `.rs` files whose first line is `// rawr`.
rust-analyzer already handles their syntax, completion and diagnostics, so the
extension has no grammar of its own.

## Debugging

The extension adds a debugger of type `rawr`. VS Code runs `rawr dap`, which
speaks the Debug Adapter Protocol on stdio as one shell of the run's debugger
session, so `rawr mcp` and a terminal can reach the same run at once.

- **Launch** (`"request": "launch"`) starts `rawr run` on `program` with
  `args`. The program waits at its first statement until VS Code has set
  the breakpoints, so a breakpoint in code that runs at startup is hit. A
  save still reloads the program; the Debug Console says so.
- **Attach** (`"request": "attach"`) reaches a `rawr run` started in a
  terminal for `program`.

Breakpoints on lines and on functions, stepping over, in and out, pause, the
call stack named by Rust paths, variables you can expand field by field, and
evaluation in the paused function all go through rawr's inspector. Press F5
with no `launch.json` to run the crate the active Rawr file belongs to, by
the nearest `Cargo.toml` above it, or the file alone when there is none. An
active `.rs` file that isn't marked runs nothing and says so. With no Rust
file active, F5 runs the workspace folder.

The extension's own tests run with `node --test` in this directory.

```json
{
  "type": "rawr",
  "request": "launch",
  "name": "Rawr: run",
  "program": "${workspaceFolder}"
}
```

The `rawr.path` setting names the `rawr` command, if it isn't on `PATH`.

## The preview pane

The extension will show previews of a Rawr crate in a separate pane, the way
Xcode shows SwiftUI previews. A dropdown in the pane lists the crate's
`#[preview]` items. Picking one renders it beside the code, and the pane
follows edits as Rawr reloads.

**The preview pane is not built yet.** It also has no backend for now. Its
backend is `rawr preview`, which was removed with rawr-iced and comes back on
Rawr's plain runner as open work. While a Rawr file is the active editor, the
`rawr.marked` context key is true.

## Install locally

Package it as a `.vsix` and install that:

```sh
npx @vscode/vsce package --skip-license --out rawr.vsix
code --install-extension rawr.vsix
```

Or, with no build step, link or copy this directory into the extension
directory for your editor, then reload the editor window:

- VS Code: `~/.vscode/extensions/rawr-0.0.4`
- VS Codium: `~/.vscode-oss/extensions/rawr-0.0.4`

For example, from this directory on macOS or Linux:

```sh
ln -s "$(pwd)" ~/.vscode/extensions/rawr-0.0.4
```

Remove the linked or copied directory to uninstall it.
