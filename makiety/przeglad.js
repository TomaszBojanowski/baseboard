// SPDX-License-Identifier: GPL-3.0-or-later

// The Overview of D64 (D63, approach b): in one column as wide as the list
// of the Fans page (600 px), a warning about a rail outside its tolerance,
// three large numbers without frames in the style of the Fans page, a
// short list (power, fans, the warmest module and drive) and "All
// Sensors" (czujniki.html), which a number or a row opens on its group.
// ?stan=spoczynek (default), napiecie (+12 V at 11.20 V), stoi (the CPU fan
// stopped, D65), fabryczny (driver of the Fedora kernel: the same page);
// ?theme=, ?width=narrow, ?w=, ?h=, ?bare=1.

(function () {
  const NB = " ";
  const params = new URLSearchParams(location.search);
  const state = params.get("stan") || "spoczynek";
  const winWidth = parseInt(params.get("w") || "0", 10);
  const fixedHeight = parseInt(params.get("h") || "0", 10);
  if (params.get("bare") === "1") document.documentElement.dataset.bare = "1";
  window.afterBuild = window.afterBuild || [];
  const V = window.PRZEGLAD;
  const low12 = state === "napiecie";
  const stopped = state === "stoi";

  const row = (t, v, o = {}) => `<div class="n-row reading${v ? " prop" : ""}" role="button">
      ${o.icon ? `<i data-ic="${o.icon}" class="${o.iconCls || ""}"></i>` : ""}
      <div class="grow"><div class="t">${t}</div>${o.s ? `<div class="s">${o.s}</div>` : ""}</div>
      ${v ? `<div class="v">${v}</div>` : ""}
      <i data-ic="chevron" class="chev"></i>
    </div>`;
  const C = v => `${v}${NB}°C`;
  const hero = (label, value) => `<div class="n-hero-item" role="button">
      <div class="l">${label}</div>
      <div class="n">${value}<span class="u">°C</span></div>
    </div>`;

  // The warning on top: what the board reads, the normal range, and advice
  // that does not overstate a reading of the board.
  const banner = low12 ? `<div class="n-banner" role="alert"><i data-ic="warning"></i>
      <div class="grow"><div class="t">Napięcie +12${NB}V poniżej normy: 11,20${NB}V</div>
      <div class="s">Norma to 11,40–12,60${NB}V. Odczyt z płyty bywa niedokładny; jeśli komputer jest niestabilny, warto sprawdzić zasilacz.</div></div></div>` : "";
  const power = low12
    ? row("Zasilanie", `11,20${NB}V`, { icon: "warning", iconCls: "warn", s: `Napięcie +12${NB}V poniżej normy` })
    : row("Zasilanie", "W normie", { icon: "ok", iconCls: "ok" });
  const fans = stopped
    ? row("Wentylatory", `4${NB}pracuje`, { icon: "warning", iconCls: "warn", s: "Wentylator procesora stoi" })
    : row("Wentylatory", `${V.fansTurning}${NB}pracuje`);

  document.getElementById("content").innerHTML = `<div class="n-page">
      <div class="n-clamp">
        ${banner}
        <div class="n-hero">
          ${hero("Procesor", Math.round(V.tctlN))}
          ${hero("Karta graficzna", Math.round(V.gpuN))}
          ${hero("Płyta", Math.round(V.systemN))}
        </div>
        <section class="n-group"><div class="boxed-list">
          ${power}
          ${fans}
          ${row("Pamięć", C(V.dimm2), { s: "Najcieplejszy z 2 modułów" })}
          ${row("Dyski", C(V.nvme0), { s: "Najcieplejszy z 2 dysków" })}
        </div></section>
        <section class="n-group"><div class="boxed-list">
          ${row("Wszystkie czujniki", "", { s: "Każdy odczyt ze źródłem i zakresem od uruchomienia aplikacji" })}
        </div></section>
      </div>
    </div>`;

  window.afterBuild.unshift(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
  });
})();
