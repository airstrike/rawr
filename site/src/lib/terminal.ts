// A terminal's screen, drawn from what a program writes, as a terminal
// draws it: text goes at the cursor, a line feed moves to the next line's
// start, and the escapes a program redraws itself with move the cursor and
// erase (CSI H, J, K and the cursor moves A to D). Colours and modes (CSI m,
// h, l) are read and dropped. An escape split across writes waits for its
// end.

/** The most rows kept: a program that only appends scrolls the oldest away. */
export const ROWS = 500;

/** ESC [ parameters final, or a lone ESC and the character after it. */
const ESCAPE = /^\x1b(?:\[([0-?]*)[ -/]*([@-~])|[^[])/;

export class Screen {
  private rows: string[][] = [[]];
  private row = 0;
  private column = 0;
  /** The start of an escape the last write ended inside. */
  private pending = "";

  /** Draw `text` at the cursor. */
  write(text: string): void {
    let rest = this.pending + text;
    this.pending = "";
    while (rest.length > 0) {
      const at = rest.search(/[\x00-\x1f]/);
      if (at !== 0) {
        const plain = at < 0 ? rest : rest.slice(0, at);
        for (const char of plain) this.put(char);
        rest = rest.slice(plain.length);
        continue;
      }
      const control = rest[0];
      if (control === "\x1b") {
        const escape = ESCAPE.exec(rest);
        if (!escape) {
          // An escape cut short: the next write ends it.
          if (/^\x1b(\[[0-?]*[ -/]*)?$/.test(rest)) {
            this.pending = rest;
            return;
          }
          rest = rest.slice(1);
          continue;
        }
        if (escape[2]) this.csi(escape[1], escape[2]);
        rest = rest.slice(escape[0].length);
        continue;
      }
      if (control === "\n") this.feed();
      else if (control === "\r") this.column = 0;
      else if (control === "\b") this.column = Math.max(0, this.column - 1);
      else if (control === "\t") this.column = (Math.floor(this.column / 8) + 1) * 8;
      rest = rest.slice(1);
    }
  }

  /** The screen's text, each row without its trailing spaces, and without empty rows at its end. */
  text(): string {
    return this.rows
      .map((cells) => cells.join("").trimEnd())
      .join("\n")
      .replace(/\n+$/, "");
  }

  private put(char: string): void {
    const cells = this.rows[this.row];
    while (cells.length < this.column) cells.push(" ");
    cells[this.column] = char;
    this.column += 1;
  }

  private feed(): void {
    this.column = 0;
    this.row += 1;
    while (this.rows.length <= this.row) this.rows.push([]);
    if (this.rows.length > ROWS) {
      this.rows.splice(0, this.rows.length - ROWS);
      this.row = ROWS - 1;
    }
  }

  /** A control sequence: `final` with its `parameters`, numbers split by `;`. */
  private csi(parameters: string, final: string): void {
    const numbers = parameters.split(";").map((part) => Number.parseInt(part, 10));
    const first = Number.isNaN(numbers[0]) ? 0 : numbers[0];
    const count = Math.max(1, first);
    switch (final) {
      case "H":
      case "f": {
        const column = Number.isNaN(numbers[1]) || numbers[1] === undefined ? 1 : numbers[1];
        this.moveTo(Math.max(1, first) - 1, Math.max(1, column) - 1);
        break;
      }
      case "A":
        this.moveTo(this.row - count, this.column);
        break;
      case "B":
        this.moveTo(this.row + count, this.column);
        break;
      case "C":
        this.column += count;
        break;
      case "D":
        this.column = Math.max(0, this.column - count);
        break;
      case "J":
        if (first === 0) {
          this.eraseLine(0);
          this.rows.length = this.row + 1;
        } else if (first === 1) {
          for (let row = 0; row < this.row; row++) this.rows[row] = [];
          this.eraseLine(1);
        } else {
          this.rows = this.rows.map(() => []);
        }
        break;
      case "K":
        this.eraseLine(first);
        break;
    }
  }

  private moveTo(row: number, column: number): void {
    this.row = Math.min(Math.max(0, row), ROWS - 1);
    this.column = column;
    while (this.rows.length <= this.row) this.rows.push([]);
  }

  /** Erase the cursor's row: from the cursor on (0), up to it (1), or all of it. */
  private eraseLine(mode: number): void {
    const cells = this.rows[this.row];
    if (mode === 0) cells.length = Math.min(cells.length, this.column);
    else if (mode === 1) for (let at = 0; at <= this.column && at < cells.length; at++) cells[at] = " ";
    else this.rows[this.row] = [];
  }
}
