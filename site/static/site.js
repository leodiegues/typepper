// docs.typepper.com: the theme switch, diagrams, copy buttons, h2 rules.
const root = document.documentElement;
const main = document.querySelector("main");
const css = (v) => getComputedStyle(root).getPropertyValue(v).trim();

// theme: remembered, like the game's setting
document.getElementById("theme").addEventListener("click", () => {
  root.dataset.theme = root.dataset.theme === "light" ? "dark" : "light";
  localStorage.setItem("typepper-docs.theme", root.dataset.theme);
  drawDiagrams();
});

// h2 rules draw out once they scroll in
const seen = new IntersectionObserver((es) => es.forEach((e) => e.isIntersecting && e.target.classList.add("seen")));
main.querySelectorAll("h2").forEach((h) => seen.observe(h));

// copy buttons on code blocks
main.querySelectorAll("pre").forEach((pre) => {
  const wrap = Object.assign(document.createElement("div"), { className: "term" });
  const btn = Object.assign(document.createElement("button"), { className: "chip", type: "button", textContent: "copy" });
  btn.addEventListener("click", async () => {
    await navigator.clipboard.writeText(pre.innerText);
    btn.textContent = "copied";
    setTimeout(() => (btn.textContent = "copy"), 1200);
  });
  pre.replaceWith(wrap);
  wrap.append(pre, btn);
});

// ```mermaid blocks become diagrams, in the theme's colors and the game's font
const diagrams = [...main.querySelectorAll("code[data-lang=mermaid]")].map((code) => {
  const div = Object.assign(document.createElement("div"), { className: "mermaid" });
  div.dataset.src = code.textContent;
  code.closest(".term").replaceWith(div);
  return div;
});
let mermaid;
async function drawDiagrams() {
  if (!diagrams.length) return;
  mermaid ??= (await import("https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs")).default;
  mermaid.initialize({
    startOnLoad: false, theme: "base",
    themeVariables: {
      fontFamily: '"Departure Mono", monospace', fontSize: "13px",
      background: css("--bg"), primaryColor: css("--chip"), primaryTextColor: css("--ink"),
      primaryBorderColor: css("--gold"), lineColor: css("--dim"), secondaryColor: css("--chip"),
      tertiaryColor: css("--bg"), clusterBkg: "transparent", clusterBorder: css("--line"),
      edgeLabelBackground: css("--bg"), actorBorder: css("--gold"), actorBkg: css("--chip"),
      noteBkgColor: css("--chip"), noteTextColor: css("--ink"), noteBorderColor: css("--line"),
    },
  });
  diagrams.forEach((d) => { d.removeAttribute("data-processed"); d.textContent = d.dataset.src; });
  await document.fonts.load('13px "Departure Mono"'); // measure labels in the real font
  await mermaid.run({ nodes: diagrams });
}
drawDiagrams();
