// Copy buttons beside commands. A button copies the text of the <code> beside
// it, and says "Copied" only when the copy worked.
(() => {
  for (const button of document.querySelectorAll(".copy")) {
    button.setAttribute("aria-label", "Copy command");
    const code = button.parentElement.querySelector("code");
    const say = (text, cls) => {
      button.textContent = text;
      button.classList.remove("done", "failed");
      if (cls) button.classList.add(cls);
    };
    button.addEventListener("click", async () => {
      let copied = false;
      try {
        await navigator.clipboard.writeText(code.textContent);
        copied = true;
      } catch {
        const range = document.createRange();
        range.selectNodeContents(code);
        const selection = getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        try {
          copied = document.execCommand("copy");
        } catch {
          copied = false;
        }
      }
      if (copied) {
        say("Copied", "done");
        setTimeout(() => say("Copy"), 1600);
      } else {
        say("Copy failed. Select and copy the command.", "failed");
        setTimeout(() => say("Copy"), 4000);
      }
    });
  }
})();
