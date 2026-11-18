import assert from "node:assert/strict";
import test from "node:test";

import { ROWS, Screen } from "../src/lib/terminal.ts";

test("a frame drawn after home and clear replaces the one before", () => {
  const screen = new Screen();
  screen.write("\x1b[H\x1b[2Jo   \n----\n");
  screen.write("\x1b[H\x1b[2J  o\n----\n");
  assert.equal(screen.text(), "  o\n----");
});

test("an escape split across writes waits for its end", () => {
  const screen = new Screen();
  screen.write("first\n\x1b[");
  screen.write("1;3Hx");
  assert.equal(screen.text(), "fixst");
});

test("erasing a line, moving the cursor, and colours dropped", () => {
  const screen = new Screen();
  screen.write("abcdef\r\x1b[3C\x1b[K\x1b[31mZ\x1b[0m\n");
  screen.write("\x1b[?25lline two\x1b[A\x1b[2K");
  assert.equal(screen.text(), "\nline two");
});

test("a program that only appends keeps its last rows", () => {
  const screen = new Screen();
  for (let line = 0; line < ROWS + 10; line++) screen.write(`${line}\n`);
  const rows = screen.text().split("\n");
  assert.equal(rows.length, ROWS - 1);
  assert.equal(rows.at(-1), String(ROWS + 9));
});
