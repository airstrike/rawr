# Rawr for VS Code

This extension is for Rawr files: `.rs` files whose first line is `// rawr`.
rust-analyzer already handles their syntax, completion and diagnostics, so the
extension has no grammar of its own.

## What it is for

The extension will show previews of a Rawr crate in a separate pane, the way
Xcode shows SwiftUI previews. A dropdown in the pane lists the crate's
`#[preview]` items. Picking one renders it beside the code, and the pane
follows edits as Rawr reloads.

**The preview pane is not built yet.** Today the extension only recognizes
Rawr files: while one is the active editor, the `rawr.marked` context key is
true.

## Install locally

No npm install or build step is required. Link or copy this directory into
the extension directory for your editor, then reload the editor window:

- VS Code: `~/.vscode/extensions/rawr-0.0.2`
- VS Codium: `~/.vscode-oss/extensions/rawr-0.0.2`

For example, from this directory on macOS or Linux:

```sh
ln -s "$(pwd)" ~/.vscode/extensions/rawr-0.0.2
```

Remove the linked or copied directory to uninstall it.
