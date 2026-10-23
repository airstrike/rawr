(() => {
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
})();
