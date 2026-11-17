import { test } from "node:test";
import assert from "node:assert/strict";

test("the library tabs open from the address bar's anchor and write theirs when chosen", async () => {
  const { tabAnchors } = await import("../src/scripts/landing.ts");
  const listeners = new Map<string, () => void>();
  const radios = new Map<string, { checked: boolean; scrolled: boolean; addEventListener(type: string, f: () => void): void; closest(): { scrollIntoView(): void } }>();
  for (const id of ["lib-iced", "lib-ratatui", "lib-std"]) {
    const radio = {
      checked: id === "lib-iced",
      scrolled: false,
      addEventListener: (type: string, f: () => void) => listeners.set(`${id}:${type}`, f),
      closest: () => ({ scrollIntoView: () => (radio.scrolled = true) }),
    };
    radios.set(id, radio);
  }
  let onHash = () => {};
  const doc = {
    getElementById: (id: string) => radios.get(id) ?? null,
    defaultView: { addEventListener: (_: string, f: () => void) => (onHash = f) },
  } as unknown as Document;
  const place = { hash: "#ratatui" } as Location;
  const written: string[] = [];
  const past = { replaceState: (_: unknown, __: string, url: string) => written.push(url) } as unknown as History;

  tabAnchors(doc, place, past);
  assert.ok(radios.get("lib-ratatui")!.checked && radios.get("lib-ratatui")!.scrolled, "#ratatui opens the ratatui tab in view");

  listeners.get("lib-std:change")!();
  assert.deepEqual(written, ["#loop"], "choosing the plain loop writes #loop");

  place.hash = "#iced";
  onHash();
  assert.ok(radios.get("lib-iced")!.checked, "a later #iced opens the iced tab");

  place.hash = "#nothing";
  radios.get("lib-std")!.checked = false;
  onHash();
  assert.ok(!radios.get("lib-std")!.checked, "an anchor that names no tab changes nothing");
});
