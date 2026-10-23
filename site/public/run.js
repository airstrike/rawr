// Run buttons for the site's runnable snippets, on the landing page and in
// the docs. Each button sits in a terminal strip. It sends the text of its
// `<code data-run>` element to /play/run, which accepts only the site's own
// snippets. The strip then shows what the program printed, or the frame an
// iced snippet drew. Without JS the buttons stay hidden and the strips keep
// their static output.
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

  for (const button of document.querySelectorAll(".runbtn")) {
    const out = document.getElementById(button.dataset.out);
    const source = document.getElementById(button.dataset.source);
    if (!out || !source) continue;
    const strip = out.closest(".run");
    button.replaceChildren(icon("play", "M2 1L11 6L2 11Z"), icon("stop", "M3 3H9V9H3Z"));
    if (!button.title) button.title = button.getAttribute("aria-label") || "Run";
    button.hidden = false;
    strip.classList.add("has-run");
    // The static output keeps its space but stays hidden until Run, so the
    // strip doesn't change height when the same output arrives.
    out.classList.add("pending");
    let running = false;

    const show = (node) => {
      const live = document.createElement("span");
      live.className = "live";
      live.append(node);
      out.replaceChildren(live);
      out.classList.remove("pending", "quiet");
    };
    const fail = (text) => {
      const span = document.createElement("span");
      span.className = "err";
      span.textContent = text;
      show(span);
    };

    button.addEventListener("click", async () => {
      if (running) return;
      running = true;
      button.setAttribute("aria-disabled", "true");
      // In a compare slider the output is on the right-hand file, so show it.
      button.closest(".compare")?.querySelector('.tabs button[data-to="0"]')?.click();
      try {
        const response = await fetch("/play/run", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ code: source.textContent }),
        });
        const type = response.headers.get("content-type") || "";
        if (response.ok && type.startsWith("application/json")) {
          const result = await response.json();
          const text = document.createDocumentFragment();
          const add = (value, cls) => {
            const span = document.createElement("span");
            span.textContent = value;
            if (cls) span.className = cls;
            text.append(span);
          };
          for (const chunk of result.output || []) add(chunk.text, chunk.stream === "stderr" ? "err" : "");
          const last = text.lastChild;
          if (last) last.textContent = last.textContent.replace(/\n$/, "");
          if (result.status !== 0) add((last ? "\n" : "") + "exit status " + result.status, "exit");
          show(text);
        } else if (response.ok && type.startsWith("image/")) {
          const img = new Image();
          img.className = "frame";
          img.alt = button.dataset.alt || "The first frame, drawn by rawr-iced on the server";
          img.src = URL.createObjectURL(await response.blob());
          await img.decode().catch(() => {});
          // Rendered at 2x: show it at its logical size.
          if (img.naturalWidth) img.width = img.naturalWidth / 2;
          show(img);
        } else {
          fail("Could not run it: " + (await response.text()).trim().slice(0, 200));
        }
      } catch (error) {
        fail("Could not reach the server: " + error.message);
      } finally {
        running = false;
        button.removeAttribute("aria-disabled");
      }
    });
  }
})();
