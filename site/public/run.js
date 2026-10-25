// Run buttons for the site's runnable snippets, on the landing page and in
// the docs. Each snippet is editable in place. Its Run button sits in a
// terminal strip and shows what the program printed on rawr's server, or the
// frame an iced snippet drew. It doesn't run the command the strip shows on
// your machine. Without JS the code stays read-only, the buttons stay hidden
// and the strips keep their static output.
//
// The site's own text goes to POST /play/run, which accepts only the exact
// sources on the site's pages and caches their results. Edited text goes to
// POST /play/render, the playground's endpoint, with the playground's checks:
// its pass cookie or a Turnstile token, and its per-IP limits.
//
// After a run the play icon becomes an ×, which clears the output. A Reset
// button at the code's top-right shows while the code differs from the
// original, and restores it.
(() => {
  const SVG = "http://www.w3.org/2000/svg";
  const icon = (cls, d) => {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 12 12");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    svg.classList.add(cls);
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    path.setAttribute("fill", "currentColor");
    svg.append(path);
    return svg;
  };

  // The playground's browser check, loaded only for an edited run.
  const TURNSTILE_JS = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
  let gate = null;
  const gateInfo = () =>
    (gate ??= fetch("/play/gate", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : { sitekey: null, pass: false }))
      .catch(() => ({ sitekey: null, pass: false })));
  let script = null;
  const loadTurnstile = () =>
    (script ??= new Promise((resolve, reject) => {
      const js = document.createElement("script");
      js.src = TURNSTILE_JS;
      js.async = true;
      js.onload = resolve;
      js.onerror = () => reject(new Error("the browser check could not load"));
      document.head.append(js);
    }));
  // A Turnstile token, or null when no check is needed. The widget stays
  // invisible unless Cloudflare asks for an interaction, shown under `strip`.
  const token = async (strip) => {
    const info = await gateInfo();
    if (!info.sitekey || info.pass) return null;
    await loadTurnstile();
    const box = document.createElement("div");
    box.className = "runcheck";
    strip.after(box);
    try {
      return await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("the browser check timed out")), 60000);
        window.turnstile.render(box, {
          sitekey: info.sitekey,
          appearance: "interaction-only",
          theme: "dark",
          callback: (t) => (clearTimeout(timer), resolve(t)),
          "error-callback": () => (clearTimeout(timer), reject(new Error("the browser check failed"))),
        });
      });
    } finally {
      box.remove();
    }
  };
  // POST an edited source to the playground's endpoint. A refused pass falls
  // back to Turnstile once.
  const render = async (code, strip) => {
    const send = async () =>
      fetch("/play/render", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ code, turnstile: await token(strip) }),
      });
    let response = await send();
    const info = await gateInfo();
    if (response.status === 403 && info.pass && info.sitekey) {
      info.pass = false;
      response = await send();
    }
    if (response.headers.get("x-play-pass") === "set") info.pass = true;
    return response;
  };

  // Plain-text editing where the browser has it. Elsewhere a paste is
  // reduced to its text.
  const probe = document.createElement("div");
  probe.contentEditable = "plaintext-only";
  const PLAIN = probe.contentEditable === "plaintext-only";

  for (const button of document.querySelectorAll(".runbtn")) {
    const out = document.getElementById(button.dataset.out);
    const source = document.getElementById(button.dataset.source);
    if (!out || !source) continue;
    const strip = out.closest(".run");
    const name = source.dataset.run || "this snippet";
    const label = button.getAttribute("aria-label") || `Run ${name} on rawr's server`;
    const original = source.textContent;
    const originalHtml = source.innerHTML;
    const staticOut = [...out.childNodes].map((node) => node.cloneNode(true));
    const staticClass = out.className;

    // The code: an editable text box.
    source.contentEditable = PLAIN ? "plaintext-only" : "true";
    source.spellcheck = false;
    source.setAttribute("autocapitalize", "off");
    source.setAttribute("autocorrect", "off");
    source.setAttribute("role", "textbox");
    source.setAttribute("aria-multiline", "true");
    source.setAttribute("aria-label", `Code of ${name}. Edit it, then run it with Control or Command and Enter.`);
    if (!PLAIN) {
      source.addEventListener("paste", (e) => {
        e.preventDefault();
        document.execCommand("insertText", false, e.clipboardData.getData("text/plain"));
      });
    }
    // innerText reads the line breaks an edit inserts as elements.
    const baseline = source.innerText;
    const text = () => (source.innerText === baseline ? original : source.innerText.replace(/\n$/, ""));
    const edited = () => text() !== original;

    // Reset: shown while the code differs from the original.
    const reset = document.createElement("button");
    reset.type = "button";
    reset.className = "resetbtn";
    reset.textContent = "Reset";
    reset.setAttribute("aria-label", `Restore the original ${name}`);
    reset.title = `Restore the original ${name}`;
    reset.hidden = true;
    // It belongs to the code, so it sits at the code block's top-right corner.
    source.before(reset);

    // While it runs, the icon is three dots: there is nothing to stop. After
    // it, the icon is an ×.
    button.replaceChildren(
      icon("play", "M2 1L11 6L2 11Z"),
      icon("busy", "M1 5H3V7H1ZM5 5H7V7H5ZM9 5H11V7H9Z"),
      icon("clear", "M2.5 1.1L6 4.6L9.5 1.1L10.9 2.5L7.4 6L10.9 9.5L9.5 10.9L6 7.4L2.5 10.9L1.1 9.5L4.6 6L1.1 2.5Z"),
    );
    button.hidden = false;
    strip.classList.add("has-run");
    // The static output keeps its space but stays hidden until Run, so the
    // strip doesn't change height when the same output arrives.
    out.classList.add("pending");

    let state = "idle";
    const setState = (next) => {
      state = next;
      button.dataset.state = next;
      const say = {
        idle: label,
        running: `Running ${name}…`,
        done: `Clear the output of ${name}`,
      }[next];
      button.setAttribute("aria-label", say);
      button.title = say;
      if (next === "running") button.setAttribute("aria-disabled", "true");
      else button.removeAttribute("aria-disabled");
    };
    setState("idle");

    let highlighted = true;
    const highlight = () => {
      if (highlighted) return;
      const code = text();
      source.textContent = code;
      window.rawrHighlight?.(source);
      highlighted = true;
    };
    const restore = () => {
      source.innerHTML = originalHtml;
      highlighted = true;
      reset.hidden = true;
    };
    const clear = () => {
      out.replaceChildren(...staticOut.map((node) => node.cloneNode(true)));
      out.className = staticClass;
      out.classList.add("pending");
    };

    source.addEventListener("input", () => {
      highlighted = false;
      reset.hidden = !edited();
      if (state === "done") setState("idle");
    });
    source.addEventListener("blur", highlight);
    source.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        if (state !== "running") run();
      }
    });
    reset.addEventListener("click", () => {
      restore();
      if (state === "done") setState("idle");
      source.focus();
    });

    const show = (node) => {
      const live = document.createElement("span");
      live.className = "live";
      live.append(node);
      out.replaceChildren(live);
      out.classList.remove("pending", "quiet");
    };
    const fail = (message) => {
      const span = document.createElement("span");
      span.className = "err";
      span.textContent = message;
      show(span);
    };

    const run = async () => {
      setState("running");
      // In a compare slider the output is on the right-hand file, so show it.
      button.closest(".compare")?.querySelector('.tabs button[data-to="0"]')?.click();
      const code = text();
      try {
        const response = edited()
          ? await render(code, strip)
          : await fetch("/play/run", {
              method: "POST",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({ code }),
            });
        const type = response.headers.get("content-type") || "";
        if (response.ok && type.startsWith("application/json")) {
          const result = await response.json();
          const lines = document.createDocumentFragment();
          const add = (value, cls) => {
            const span = document.createElement("span");
            span.textContent = value;
            if (cls) span.className = cls;
            lines.append(span);
          };
          for (const chunk of result.output || []) add(chunk.text, chunk.stream === "stderr" ? "err" : "");
          const last = lines.lastChild;
          if (last) last.textContent = last.textContent.replace(/\n$/, "");
          if (result.status !== 0) add((last ? "\n" : "") + "exit status " + result.status, "exit");
          show(lines);
        } else if (response.ok && type.startsWith("image/")) {
          const img = new Image();
          img.className = "frame";
          img.alt = button.dataset.alt || "The frame the code drew on rawr's server";
          img.src = URL.createObjectURL(await response.blob());
          await img.decode().catch(() => {});
          // Rendered at 2x: show it at its logical size.
          if (img.naturalWidth) img.width = img.naturalWidth / 2;
          show(img);
        } else {
          fail("The code could not run. " + (await response.text()).trim().slice(0, 2000));
        }
      } catch (error) {
        // fetch throws a TypeError when the network fails.
        fail(error instanceof TypeError ? "rawr's server could not be reached. " + error.message : "The code could not run. " + error.message);
      }
      setState("done");
    };

    button.addEventListener("click", () => {
      if (state === "running") return;
      if (state === "done") {
        clear();
        setState("idle");
        return;
      }
      highlight();
      run();
    });
  }
})();
