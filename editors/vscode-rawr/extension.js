// The Rawr extension. A Rawr file is a `.rs` file whose first line is
// `// rawr`; rust-analyzer handles its syntax. The extension tells such
// files apart (the `rawr.marked` context key is true while one is the
// active editor), which the preview pane will build on, and runs the
// `rawr` debugger: VS Code starts `rawr dap`, which speaks the Debug
// Adapter Protocol on stdio, for each debug session of type `rawr`.

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

/** `rawr dap`, with the `rawr` command the settings name. */
const adapter = {
  createDebugAdapterDescriptor(session) {
    const command = vscode.workspace.getConfiguration("rawr").get("path") || "rawr";
    const folder = session.workspaceFolder;
    const options = folder === undefined ? undefined : { cwd: folder.uri.fsPath };
    return new vscode.DebugAdapterExecutable(command, ["dap"], options);
  },
};

/**
 * A launch with no configuration, as F5 without a launch.json starts:
 * the active file's program, else the workspace folder's.
 */
const configurations = {
  resolveDebugConfiguration(folder, configuration) {
    if (configuration.type === undefined && configuration.request === undefined) {
      const editor = vscode.window.activeTextEditor;
      const program =
        editor !== undefined && isMarked(editor.document)
          ? editor.document.uri.fsPath
          : folder?.uri.fsPath;
      if (program === undefined) {
        return undefined;
      }
      return { type: "rawr", request: "launch", name: "Rawr: run", program };
    }
    return configuration;
  },
};

function activate(context) {
  update(vscode.window.activeTextEditor);
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("rawr", adapter),
    vscode.debug.registerDebugConfigurationProvider("rawr", configurations),
  );
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
