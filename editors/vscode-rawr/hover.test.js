// `node --test` in this directory.

const assert = require("node:assert");
const test = require("node:test");

const { placeAt } = require("./hover");

test("a hover takes the place under the pointer through its fields, indices and derefs", () => {
  const line = "    total += *player.items[turn].0 + count; // player.score";
  const at = (column) => {
    const range = placeAt(line, column);
    return range === undefined ? undefined : line.slice(range[0], range[1]);
  };
  assert.strictEqual(at(20), "*player.items[turn].0");
  assert.strictEqual(at(26), "*player.items[turn].0");
  assert.strictEqual(at(28), "turn");
  assert.strictEqual(at(39), "count");
  assert.strictEqual(at(10), undefined);
  assert.strictEqual(at(50), undefined);
});
