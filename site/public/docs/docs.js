(() => {
  // Sections that moved to another page. A link to an old anchor that isn't
  // on this page goes to the section's new home. Generated with the page split.
  const MOVED = {"/docs": {"status": "/docs/requirements#status"}, "/docs/cli": {"rawr-iced": "/docs/iced/previews#commands", "rawr-import": "/docs/imports"}, "/docs/how-it-works": {"ship": "/docs/run#release", "snippets": "/docs/play/reference#sandbox"}, "/docs/iced": {"async": "/docs/iced/reference#async", "async-rawr": "/docs/iced/reference#async-rawr", "available": "/docs/iced/reference#available", "available-widgets": "/docs/iced/reference#available-widgets", "blocking": "/docs/iced/reference#blocking", "canvas": "/docs/iced/reference#canvas", "components": "/docs/iced/reference#components", "custom-widgets": "/docs/iced/reference#custom-widgets", "diagnostics": "/docs/iced/reference#diagnostics", "examples": "/docs/iced/reference#examples", "hot-reload": "/docs/iced/reload", "main-statements": "/docs/iced/reload#main-statements", "not-available": "/docs/iced/reference#not-available", "previews": "/docs/iced/previews#previews", "rust-crates": "/docs/iced/reference#rust-crates", "subscriptions": "/docs/iced/reference#subscriptions", "views": "/docs/iced/reference#views"}, "/docs/play": {"add-bot": "/docs/play/discord#add-bot", "allowance": "/docs/play/reference#allowance", "animation-time": "/docs/play/reference#animation-time", "clock": "/docs/play/reference#clock", "crates": "/docs/play/reference#crates", "differences": "/docs/play/reference#differences", "discord": "/docs/play/discord#discord", "error-replies": "/docs/play/reference#error-replies", "frames": "/docs/play/reference#frames", "image-notes": "/docs/play/reference#image-notes", "images": "/docs/play/reference#images", "limits": "/docs/play/reference#limits", "loops": "/docs/play/reference#loops", "messages": "/docs/play/reference#messages", "notes": "/docs/play/reference#notes", "options": "/docs/play/discord#options", "output": "/docs/play/reference#output", "output-notes": "/docs/play/reference#output-notes", "prelude": "/docs/play/reference#prelude", "sandbox": "/docs/play/reference#sandbox", "sippers": "/docs/play/reference#sippers", "size": "/docs/play/reference#size", "spawn": "/docs/play/reference#spawn", "spawn-blocking": "/docs/play/reference#spawn-blocking", "tokio": "/docs/play/reference#tokio", "tokio-differences": "/docs/play/reference#tokio-differences", "wrapping": "/docs/play/reference#wrapping"}, "/docs/start": {"first-app": "/docs/iced#new", "hot-reload": "/docs/iced/reload", "install-crate": "/docs/embedding#add", "install-iced": "/docs/iced#install", "install-import": "/docs/imports#install"}};
  const here = location.pathname.replace(/\.html$/, "").replace(/\/index$/, "").replace(/\/$/, "") || "/";
  const id = decodeURIComponent(location.hash.slice(1));
  if (id && !document.getElementById(id) && MOVED[here] && MOVED[here][id]) {
    location.replace(MOVED[here][id]);
    return;
  }

  // Highlight Rust and TOML by hand, as the landing page does: one regex pass, no library.
  const KW = /^(as|break|const|continue|crate|dyn|else|enum|false|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|self|Self|static|struct|super|trait|true|type|use|where|while)$/;
  const TOKEN = /(\/\/[^\n]*)|("(?:\\.|[^"\\])*")|(#\[[^\]\n]*\])|(\b\d[\d_]*(?:\.\d+)?\b)|(\b[A-Za-z_][A-Za-z0-9_]*!)|(\b[A-Za-z_][A-Za-z0-9_]*\b)|(\[[^\]\n]*\](?=\n|$))/g;
  const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const span = (c, s) => `<span class="${c}">${esc(s)}</span>`;
  for (const el of document.querySelectorAll("code[data-lang]")) {
    const src = el.textContent, toml = el.dataset.lang === "toml";
    let out = "", last = 0;
    src.replace(TOKEN, (m, com, str, attr, num, mac, word, table, at) => {
      out += esc(src.slice(last, at));
      last = at + m.length;
      if (com) out += span("c", m);
      else if (str) out += span("s", m);
      else if (attr) out += span("a", m);
      else if (num) out += span("n", m);
      else if (mac) out += span("m", m);
      else if (table && toml) out += span("k", m);
      else if (word && !toml && KW.test(m)) out += span("k", m);
      else if (word && !toml && /^[A-Z]/.test(m)) out += span("t", m);
      else out += esc(m);
      return m;
    });
    el.innerHTML = out + esc(src.slice(last));
  }

  // A "#" link before each section heading that has an id.
  for (const h of document.querySelectorAll("article h2[id], article h3[id]")) {
    const a = document.createElement("a");
    a.className = "anchor";
    a.href = "#" + h.id;
    a.textContent = "#";
    a.setAttribute("aria-label", "Link to this section");
    h.prepend(a);
  }

  // "On this page": the page's sections, shown on narrow screens.
  const sections = [...document.querySelectorAll("article h2[id]")];
  const lede = document.querySelector("article .lede");
  if (sections.length > 2 && lede) {
    const toc = document.createElement("details");
    toc.className = "toc";
    const summary = document.createElement("summary");
    summary.textContent = "On this page";
    const list = document.createElement("ol");
    for (const h of sections) {
      const item = document.createElement("li");
      const link = document.createElement("a");
      link.href = "#" + h.id;
      link.textContent = h.textContent.replace(/^#/, "");
      item.append(link);
      list.append(item);
    }
    toc.append(summary, list);
    lede.after(toc);
  }
})();
