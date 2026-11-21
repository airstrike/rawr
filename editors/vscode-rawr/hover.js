// What a debug hover asks `rawr dap` to read: the place under the
// pointer, the name there with the fields, tuple items, indices and
// derefs around it, as `*player.items[turn].0`. VS Code's own word
// stops at `[`, so `names[turn]` would hover as `names` alone. The
// adapter reads it with rawr's hover read, which runs no call.

const word = (character) => character !== undefined && /[\p{L}\p{N}_]/u.test(character);

/** The `[` that the `]` at `close` closes. */
function opening(characters, close) {
  let depth = 0;
  for (let at = close; at >= 0; at -= 1) {
    if (characters[at] === "]") depth += 1;
    if (characters[at] === "[" && --depth === 0) return at;
  }
  return undefined;
}

/** The `]` that closes the `[` at `open`. */
function closing(characters, open) {
  let depth = 0;
  for (let at = open; at < characters.length; at += 1) {
    if (characters[at] === "[") depth += 1;
    if (characters[at] === "]" && --depth === 0) return at;
  }
  return undefined;
}

/** Whether character `column` of `text` is outside its strings and comment. */
function inCode(text, column) {
  let quoted = false;
  let escaped = false;
  for (let at = 0; at < column; at += 1) {
    const character = text[at];
    if (quoted) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quoted = false;
    } else if (character === "/" && text[at + 1] === "/") {
      return false;
    } else if (character === '"') {
      quoted = true;
    }
  }
  return !quoted;
}

/**
 * The characters `[start, end)` of the place `text` has at character
 * `column`, or nothing where no place is.
 */
function placeAt(text, column) {
  // VS Code counts a line's characters in UTF-16 units.
  const characters = text.split("");
  const pointed = characters[column];
  if (pointed === undefined || !inCode(text, column)) return undefined;
  let start;
  let end;
  if (pointed === "[") [start, end] = [column, (closing(characters, column) ?? -2) + 1];
  else if (pointed === "]") [start, end] = [opening(characters, column), column + 1];
  else if (pointed === "." || pointed === "*" || word(pointed)) [start, end] = [column, column + 1];
  else return undefined;
  if (start === undefined || end < 0) return undefined;
  while (start > 0 && word(characters[start - 1])) start -= 1;
  while (word(characters[end])) end += 1;
  while (start >= 2 && characters[start - 1] === "." && (word(characters[start - 2]) || characters[start - 2] === "]")) {
    start -= 1;
    while (start > 0 && characters[start - 1] === "]") {
      start = opening(characters, start - 1);
      if (start === undefined) return undefined;
    }
    while (start > 0 && word(characters[start - 1])) start -= 1;
  }
  while (start > 0 && characters[start - 1] === "*") start -= 1;
  for (;;) {
    if (characters[end] === "." && word(characters[end + 1])) {
      end += 1;
      while (word(characters[end])) end += 1;
    } else if (characters[end] === "[") {
      const close = closing(characters, end);
      if (close === undefined) break;
      end = close + 1;
    } else {
      break;
    }
  }
  return [start, end];
}

module.exports = { placeAt };
