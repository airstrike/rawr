import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const loadJson = async (path) => JSON.parse(await readFile(new URL(path, root), "utf8"));

const [manifest, configuration, grammar, fixture] = await Promise.all([
  loadJson("package.json"),
  loadJson("language-configuration.json"),
  loadJson("syntaxes/rawr.tmLanguage.json"),
  readFile(new URL("test/fixture.rawr", root), "utf8"),
]);

assert.equal(manifest.contributes.languages[0].id, "rawr");
assert.deepEqual(manifest.contributes.languages[0].extensions, [".rawr"]);
assert.equal(manifest.contributes.grammars[0].scopeName, "source.rawr");
assert.equal(grammar.scopeName, "source.rawr");
assert.equal(grammar.patterns.at(-1).include, "source.rust");
assert.equal(configuration.comments.lineComment, "//");

const regexFields = new Set(["begin", "end", "match", "while"]);
const validateRegexes = (value, path = "grammar") => {
  if (Array.isArray(value)) {
    value.forEach((entry, index) => validateRegexes(entry, `${path}[${index}]`));
    return;
  }
  if (value === null || typeof value !== "object") return;

  for (const [key, entry] of Object.entries(value)) {
    if (regexFields.has(key)) {
      assert.doesNotThrow(() => new RegExp(entry, "u"), `${path}.${key}`);
    } else {
      validateRegexes(entry, `${path}.${key}`);
    }
  }
};
validateRegexes(grammar);

const matches = (repositoryName, patternIndex, source) => {
  const pattern = grammar.repository[repositoryName].patterns[patternIndex].match;
  return new RegExp(pattern, "u").test(source);
};

const attributePatterns = grammar.repository["rawr-attributes"].patterns;
for (const attribute of ["widget", "preview", "derive"]) {
  assert(
    attributePatterns.some((rule) => new RegExp(rule.match ?? rule.begin, "u").test(`#[${attribute}]`)),
    `missing ${attribute} attribute`,
  );
}
assert(matches("module-declarations", 0, fixture), "missing module declaration");
assert(matches("use-declarations", 0, fixture), "missing use declaration");
assert(matches("framework-paths", 0, fixture), "missing framework path");
assert(matches("rawr-keywords", 0, fixture), "missing Rawr control keyword");
assert(matches("rawr-operators", 0, fixture), "missing Rawr operator");
assert(matches("rawr-operators", 1, fixture), "missing Rawr map opener");
assert(fixture.includes("${"), "missing interpolated string fixture");

console.log("Rawr VS Code grammar smoke validation passed.");
