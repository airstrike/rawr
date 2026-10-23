// The Rawr extension. A Rawr file is a `.rs` file whose first line is
// `// rawr`; rust-analyzer handles its syntax. For now the extension only
// tells such files apart: the `rawr.marked` context key is true while one
// is the active editor. The preview pane will build on it.

const vscode = require("vscode");

const MARKER = "// rawr";

/** Whether `document` is a `.rs` file whose first line is `// rawr`. */
function isMarked(document) {
  if (document.languageId !== "rust" || document.lineCount === 0) {
    return false;
  }
  const first = document.lineAt(0).text.replace(/^﻿/, "");
  return first.trimEnd() === MARKER;
}

function update(editor) {
  const marked = editor !== undefined && isMarked(editor.document);
  vscode.commands.executeCommand("setContext", "rawr.marked", marked);
}

function activate(context) {
  update(vscode.window.activeTextEditor);
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(update),
    vscode.workspace.onDidChangeTextDocument((event) => {
      const editor = vscode.window.activeTextEditor;
      if (editor !== undefined && event.document === editor.document) {
        update(editor);
      }
    }),
  );
}

function deactivate() {}

module.exports = { activate, deactivate, isMarked };
