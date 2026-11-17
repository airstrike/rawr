// The landing page: the subtitle under "rawr" as wide as the word, and the
// cargo run and rawr run sliders.
export function setup(): void {
  const brand = document.querySelector(".hero h1");
  if (brand) {
    const word = brand.querySelector<HTMLElement>(".word")!;
    const sub = brand.querySelector<HTMLElement>(".sub")!;
    const fit = () => {
      sub.style.fontSize = "";
      const ratio = word.getBoundingClientRect().width / sub.getBoundingClientRect().width;
      sub.style.fontSize = parseFloat(getComputedStyle(sub).fontSize) * ratio + "px";
    };
    document.fonts.ready.then(fit);
    addEventListener("resize", fit);
  }

  // Each slider: --pos is where the right-hand file starts covering the left.
  for (const fig of document.querySelectorAll<HTMLElement>(".compare")) {
    const stage = fig.querySelector<HTMLElement>(".stage")!;
    const grip = fig.querySelector<HTMLElement>(".grip")!;
    const tabs = fig.querySelectorAll<HTMLButtonElement>(".tabs button");
    const left = fig.dataset.left, right = fig.dataset.right;
    let pos = 50;
    const set = (value: number, tween: boolean) => {
      pos = Math.max(0, Math.min(100, Math.round(value * 10) / 10));
      fig.classList.remove("intro");
      fig.classList.toggle("tween", tween);
      fig.style.setProperty("--pos", pos + "%");
      const n = Math.round(pos);
      grip.setAttribute("aria-valuenow", String(n));
      grip.setAttribute("aria-valuetext", n >= 100 ? `All ${left}` : n <= 0 ? `All ${right}` : `${n}% ${left}, ${100 - n}% ${right}`);
      tabs[0].setAttribute("aria-pressed", String(n >= 100));
      tabs[1].setAttribute("aria-pressed", String(n <= 0));
    };
    const fromPointer = (e: PointerEvent) => {
      const r = stage.getBoundingClientRect();
      set(((e.clientX - r.left) / r.width) * 100, false);
    };
    for (const handle of [grip, fig.querySelector<HTMLElement>(".hit")!]) {
      handle.addEventListener("pointerdown", (e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        handle.setPointerCapture(e.pointerId);
        document.body.classList.add("dragging");
        grip.focus({ preventScroll: true });
        fromPointer(e);
      });
      handle.addEventListener("pointermove", (e) => {
        if (handle.hasPointerCapture(e.pointerId)) fromPointer(e);
      });
      const end = () => document.body.classList.remove("dragging");
      handle.addEventListener("pointerup", end);
      handle.addEventListener("pointercancel", end);
    }
    grip.addEventListener("keydown", (e) => {
      const step = e.shiftKey ? 25 : 5;
      const next = ({
        ArrowLeft: pos - step, ArrowDown: pos - step,
        ArrowRight: pos + step, ArrowUp: pos + step,
        PageDown: pos - 25, PageUp: pos + 25, Home: 0, End: 100,
      } as Record<string, number>)[e.key];
      if (next === undefined) return;
      e.preventDefault();
      set(next, e.key === "Home" || e.key === "End" || e.key.startsWith("Page"));
    });
    for (const tab of tabs) tab.addEventListener("click", () => set(Number(tab.dataset.to), true));
    // The intro peek animates --pos itself; once it ends, the inline value holds.
    fig.addEventListener("animationend", () => fig.classList.remove("intro"));
  }

  tabAnchors(document, location, history);
}

/** The library tabs' radio inputs by the anchor each one has in the address bar. */
export const TAB_ANCHORS: Record<string, string> = { iced: "lib-iced", ratatui: "lib-ratatui", loop: "lib-std" };

/**
 * The library tabs follow the address bar's anchor: `#iced`, `#ratatui` or
 * `#loop` opens that tab and brings it into view, and choosing a tab writes
 * its anchor without scrolling, so a tab can be linked to.
 */
export function tabAnchors(doc: Document, place: Location, past: History): void {
  const radio = (anchor: string) => {
    const id = TAB_ANCHORS[anchor];
    return id ? doc.getElementById(id) as HTMLInputElement | null : null;
  };
  const open = () => {
    const tab = radio(place.hash.slice(1));
    if (!tab) return;
    tab.checked = true;
    tab.closest("figure")?.scrollIntoView({ block: "start" });
  };
  for (const [anchor, id] of Object.entries(TAB_ANCHORS)) {
    doc.getElementById(id)?.addEventListener("change", () => past.replaceState(null, "", `#${anchor}`));
  }
  doc.defaultView?.addEventListener("hashchange", open);
  open();
}
