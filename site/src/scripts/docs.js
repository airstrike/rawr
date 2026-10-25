(() => {
  // Sections that moved to another page. A link to an old anchor that isn't
  // on this page goes to the section's new home. The Worker redirects moved
  // pages (DOCS_MOVED in its index.ts) and the browser keeps the anchor.
  const MOVED = {"/docs":{"status":"/docs/reference/requirements#status"},"/docs/embedding":{"call-fn":"/docs/embedding#example","lifecycle":"/docs/embedding#reload"},"/docs/how-it-works":{"ship":"/docs/run#release","snippets":"/docs/reference/play#sandbox"},"/docs/iced":{"async":"/docs/reference/iced#async","async-rawr":"/docs/reference/iced#async-rawr","available":"/docs/reference/iced#available","available-widgets":"/docs/reference/iced#available-widgets","blocking":"/docs/reference/iced#blocking","canvas":"/docs/reference/iced#canvas","components":"/docs/reference/iced#components","custom-widgets":"/docs/reference/iced#custom-widgets","diagnostics":"/docs/reference/iced#diagnostics","examples":"/docs/reference/iced#examples","hot-reload":"/docs/iced/reload","main-statements":"/docs/iced/reload#main-statements","not-available":"/docs/reference/iced#not-available","previews":"/docs/iced/previews#previews","rust-crates":"/docs/reference/iced#rust-crates","subscriptions":"/docs/reference/iced#subscriptions","views":"/docs/reference/iced#views"},"/docs/play":{"add-bot":"/docs/play/discord#add-bot","allowance":"/docs/reference/play#allowance","animation-time":"/docs/reference/play#animation-time","clock":"/docs/reference/play#clock","crates":"/docs/reference/play#crates","differences":"/docs/reference/play#differences","discord":"/docs/play/discord#discord","error-replies":"/docs/reference/play#error-replies","frames":"/docs/reference/play#frames","image-notes":"/docs/reference/play#image-notes","images":"/docs/reference/play#images","limits":"/docs/reference/play#limits","loops":"/docs/reference/play#loops","messages":"/docs/reference/play#messages","notes":"/docs/reference/play#notes","options":"/docs/play/discord#options","output":"/docs/reference/play#output","output-notes":"/docs/reference/play#output-notes","prelude":"/docs/reference/play#prelude","sandbox":"/docs/reference/play#sandbox","sippers":"/docs/reference/play#sippers","size":"/docs/reference/play#size","spawn":"/docs/reference/play#spawn","spawn-blocking":"/docs/reference/play#spawn-blocking","tokio":"/docs/reference/play#tokio","tokio-differences":"/docs/reference/play#tokio-differences","wrapping":"/docs/reference/play#wrapping"},"/docs/reference/commands":{"rawr-iced":"/docs/iced/previews#commands","rawr-import":"/docs/imports"},"/docs/reference/overview":{"items":"/docs/reference/items","types":"/docs/reference/types","generics-traits":"/docs/reference/generics","patterns":"/docs/reference/patterns","moves":"/docs/reference/moves","closures":"/docs/reference/closures","async":"/docs/reference/closures#async","macros":"/docs/reference/macros","format":"/docs/reference/macros#format","std":"/docs/reference/std"},"/docs/start":{"first-app":"/docs/iced#new","hot-reload":"/docs/iced/reload","install-crate":"/docs/embedding#add","install-iced":"/docs/iced#install","install-import":"/docs/imports#install"}};
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
  // run.js calls it again on code that was edited.
  const highlight = (el) => {
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
  };
  for (const el of document.querySelectorAll("code[data-lang]")) highlight(el);
  window.rawrHighlight = highlight;

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
