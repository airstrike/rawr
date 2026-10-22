# Rawr syntax highlighting for VS Code

This local extension associates `.rawr` files with the Rawr language and layers Rawr-specific TextMate scopes over VS Code's built-in Rust grammar. It covers `#[widget]`, `#[preview]`, `#[derive]`, package-source `mod` and `use` declarations, legacy definition-file `module` declarations, `iced::`/`rawr::` paths, Rawr-only control words and operators, maps, and backtick interpolation.

## Install locally

No npm install or build step is required. Link or copy this directory into the extension directory for your editor, then reload the editor window:

- VS Code: `~/.vscode/extensions/rawr-language-0.0.1`
- VS Codium: `~/.vscode-oss/extensions/rawr-language-0.0.1`

For example, from this directory on macOS or Linux:

```sh
ln -s "$(pwd)" ~/.vscode/extensions/rawr-language-0.0.1
```

Open a `.rawr` file and confirm that the language indicator reads **Rawr**. Remove the linked or copied directory to uninstall it.

## Validate

Run the dependency-free smoke check with Node.js:

```sh
node test/validate.mjs
```

The check parses all extension JSON, compiles the grammar's regular expressions with the local JavaScript engine, verifies the Rust grammar include, and checks representative Rawr syntax in `test/fixture.rawr`.

## Scope

This extension provides syntax highlighting, comments, brackets, and quote pairing only. It does not provide parsing, diagnostics, completion, formatting, preview integration, or language-server features. Highlighting inherits most behavior from the built-in `source.rust` grammar, so exact token colors depend on the active theme and the editor's bundled Rust grammar.
