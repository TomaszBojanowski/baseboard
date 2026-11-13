// SPDX-License-Identifier: GPL-3.0-or-later

// Shared scaffolding for the HTML mockups: theme switch, icons, window frame.
// Open a page with ?theme=dark or ?theme=light to force a color scheme, and
// with ?width=narrow for a narrow window (collapsed sidebar, as in the app
// below 600 sp). A page can push functions to window.afterBuild; they run
// once the window exists, e.g. to draw charts at their final size.

(function () {
  const params = new URLSearchParams(location.search);
  const theme = params.get("theme");
  if (theme === "dark" || theme === "light") {
    document.documentElement.dataset.theme = theme;
  }
  const narrow = params.get("width") === "narrow";
  if (narrow) document.documentElement.dataset.width = "narrow";
  window.afterBuild = window.afterBuild || [];

  // ?accent=purple and so on: the accent colors of libadwaita, to check that
  // a page follows the accent the user picked in the system settings.
  const ACCENTS = {
    blue: "#3584e4", teal: "#2190a4", green: "#3a944a", yellow: "#c88800", orange: "#ed5b00",
    red: "#e62d42", pink: "#d56199", purple: "#9141ac", slate: "#6f8396",
  };
  const accent = ACCENTS[params.get("accent")];
  if (accent) document.documentElement.style.setProperty("--accent-bg", accent);

  // Simple 16x16 symbolic icons drawn for these mockups.
  const F = 'style="fill:currentColor;stroke:none"';
  const ICONS = {
    gauge: `<path d="M2.8 12 A6 6 0 1 1 13.2 12"/><path d="M8 9 L10.9 5.6"/><circle cx="8" cy="9" r="1.5" ${F}/>`,
    fan: [0, 120, 240].map(a =>
      `<path d="M8 8 C6.1 6.9 5.4 3.9 6.9 2.1 C8.3 0.5 11.3 1.1 11.4 3.2 C11.5 5.2 9.6 6.6 8 8 Z" transform="rotate(${a} 8 8)" ${F}/>`).join("") +
      `<circle cx="8" cy="8" r="1.4" ${F}/>`,
    rgb: `<circle cx="8" cy="5.6" r="3.6"/><circle cx="5.7" cy="9.6" r="3.6"/><circle cx="10.3" cy="9.6" r="3.6"/>`,
    bolt: `<path d="M9.2 1.2 L3.4 9.2 H7.6 L6.8 14.8 L12.6 6.6 H8.4 Z" ${F}/>`,
    chip: `<rect x="3.5" y="3.5" width="9" height="9" rx="1.5"/><rect x="6" y="6" width="4" height="4" rx=".5" ${F}/>` +
      `<path d="M6 1.2v2.3M10 1.2v2.3M6 12.5v2.3M10 12.5v2.3M1.2 6h2.3M1.2 10h2.3M12.5 6h2.3M12.5 10h2.3"/>`,
    menu: `<path d="M2.5 4h11M2.5 8h11M2.5 12h11"/>`,
    close: `<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>`,
    warning: `<path d="M8 1.9 L14.8 13.8 H1.2 Z"/><path d="M8 6.2v3.4"/><circle cx="8" cy="11.7" r=".9" ${F}/>`,
    ok: `<circle cx="8" cy="8" r="6.5"/><path d="M5.1 8.2 L7.1 10.2 L10.9 6.1"/>`,
    info: `<circle cx="8" cy="8" r="6.5"/><path d="M8 7.3v4"/><circle cx="8" cy="4.9" r=".9" ${F}/>`,
    next: `<path d="M6 3.5 L10.5 8 L6 12.5"/>`,
    back: `<path d="M10 3.5 L5.5 8 L10 12.5"/>`,
    down: `<path d="M3.5 6 L8 10.5 L12.5 6"/>`,
    plus: `<path d="M8 3v10M3 8h10"/>`,
    minus: `<path d="M3 8h10"/>`,
    copy: `<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.2 V3 A1.5 1.5 0 0 0 9 1.5 H4 A1.5 1.5 0 0 0 2.5 3 V8 A1.5 1.5 0 0 0 4 9.5 H3.5"/>`,
    restore: `<path d="M13.3 8 A5.3 5.3 0 1 1 11.6 4.1"/><path d="M12.2 1.4 V4.6 H9"/>`,
    external: `<path d="M9.5 2.5 H13.5 V6.5M13.5 2.5 L7.5 8.5"/><path d="M12 9.5 V13.5 H2.5 V4 H6.5"/>`,
    drop: `<path d="M8 1.8 C8 1.8 3.6 6.9 3.6 9.9 A4.4 4.4 0 0 0 12.4 9.9 C12.4 6.9 8 1.8 8 1.8Z"/>`,
    trash: `<path d="M2.5 4h11M6 4V2.5h4V4M4 4l.8 9.5h6.4L12 4"/>`,
    thermo: `<path d="M6.3 9.3V3a1.7 1.7 0 0 1 3.4 0v6.3a3.2 3.2 0 1 1-3.4 0Z"/><circle cx="8" cy="11.7" r="1.3" ${F}/>`,
    leaf: `<path d="M13.5 2.5 C6 2.5 2.5 6 2.5 13.5 C10 13.5 13.5 10 13.5 2.5Z"/><path d="M2.5 13.5 L9 7"/>`,
    balance: `<circle cx="8" cy="8" r="6.5"/><path d="M8 1.5 A6.5 6.5 0 0 1 8 14.5 Z" ${F}/>`,
    reboot: `<path d="M4 3.5 A6 6 0 1 0 8 2"/><path d="M8 2 L5.6 0.6M8 2 L6.4 4.4"/>`,
    lock: `<rect x="3" y="7" width="10" height="7.5" rx="1.5"/><path d="M5.2 7V5a2.8 2.8 0 0 1 5.6 0v2"/>`,
    pencil: `<path d="M10.5 2.5 L13.5 5.5 L5.5 13.5 H2.5 V10.5 Z"/>`,
    error: `<circle cx="8" cy="8" r="6.5"/><path d="M5.7 5.7l4.6 4.6M10.3 5.7l-4.6 4.6"/>`,
    curve: `<path d="M1.5 13.5 H14.5M1.5 13.5 V2"/><path d="M3 11.5 L6.5 10 L9.5 6.5 L13.5 3"/>`,
    hand: `<path d="M5 8.5V3.6a1.1 1.1 0 0 1 2.2 0V7.5M7.2 7V2.6a1.1 1.1 0 0 1 2.2 0V7M9.4 7V3.4a1.1 1.1 0 0 1 2.2 0V9.5c0 2.8-1.8 4.8-4.4 4.8-2 0-3-1-4-2.6L1.9 9.6a1.1 1.1 0 0 1 1.8-1.2L5 9.8"/>`,
    package: `<path d="M8 1.6 L14 4.6 V11.4 L8 14.4 L2 11.4 V4.6 Z"/><path d="M2 4.6 L8 7.6 L14 4.6M8 7.6 V14.4"/>`,
    keyboard: `<rect x="1.5" y="4" width="13" height="8.5" rx="1.5"/><path d="M4 6.8h.1M6.6 6.8h.1M9.3 6.8h.1M12 6.8h.1M5 9.8h6"/>`,
    // As in the app (D64): the board, a frame with parts in it, and the
    // three buttons of the window.
    board: `<rect x="1.8" y="1.8" width="12.4" height="12.4" rx="1.5"/><rect x="4.5" y="4.5" width="4" height="4" rx=".6" ${F}/><path d="M10.5 5h1.5M10.5 7.5h1.5M4.5 11h7"/>`,
    minimize: `<path d="M4.5 10.5h7"/>`,
    maximize: `<rect x="4.5" y="4.5" width="7" height="7" rx="1"/>`,
    chevron: `<path d="M6 3.5 L10.5 8 L6 12.5"/>`,
    // The parts of the computer on the cards and in the list of the System
    // page (D67): a disk, a memory module, a graphics card, a processor,
    // and the list of every sensor.
    drive: `<rect x="1.8" y="4" width="12.4" height="8" rx="1.6"/><circle cx="11.2" cy="8" r=".9" ${F}/><path d="M4 8h4"/>`,
    memory: `<rect x="1.5" y="4.5" width="13" height="6" rx="1"/><path d="M4 7h1.5M7.2 7h1.5M10.5 7h1.5M3 10.5v1.8M6 10.5v1.8M9 10.5v1.8M12 10.5v1.8"/>`,
    gpu: `<rect x="1.5" y="3.5" width="13" height="8" rx="1.3"/><circle cx="6" cy="7.5" r="2.3"/><path d="M10 6h2.5M10 9h2.5M3 11.5v2"/>`,
    cpu: `<rect x="3.5" y="3.5" width="9" height="9" rx="1.5"/><rect x="6" y="6" width="4" height="4" rx=".5" ${F}/>` +
      `<path d="M6 1.2v2.3M10 1.2v2.3M6 12.5v2.3M10 12.5v2.3M1.2 6h2.3M1.2 10h2.3M12.5 6h2.3M12.5 10h2.3"/>`,
    list: `<path d="M5.5 4h8M5.5 8h8M5.5 12h8"/><circle cx="2.8" cy="4" r=".9" ${F}/><circle cx="2.8" cy="8" r=".9" ${F}/><circle cx="2.8" cy="12" r=".9" ${F}/>`,
  };

  window.iconPaths = ICONS;

  window.icon = function (name, extra = "") {
    return `<svg class="ic ${extra}" viewBox="0 0 16 16" aria-hidden="true">${ICONS[name] || ""}</svg>`;
  };

  // Only the pages the app has (M1) or gets next (M2); RGB and Performance
  // join the sidebar with their milestones.
  const PAGES = [
    ["przeglad", "Przegląd", "gauge"],
    ["wentylatory", "Wentylatory", "fan"],
    ["system", "System", "board"],
  ];

  function replaceIcons() {
    // Replace <i data-ic="name"> placeholders with icons.
    document.querySelectorAll("i[data-ic]").forEach(el => {
      el.outerHTML = icon(el.dataset.ic, el.className);
    });
  }

  function finish() {
    window.afterBuild.forEach(f => f());
    // Let the render script know how tall the page is.
    document.documentElement.dataset.height =
      Math.ceil(document.documentElement.getBoundingClientRect().height);
  }

  function build() {
    const body = document.body;
    const page = body.dataset.nav || body.dataset.page;
    const content = document.getElementById("content");
    const headEnd = document.getElementById("header-end");
    const banner = document.getElementById("banner");
    const bottom = document.getElementById("bottom-bar");

    // A sheet of components, without the window around it.
    if (body.dataset.sheet) {
      replaceIcons();
      finish();
      return;
    }

    const nav = PAGES.map(([id, label, ic]) =>
      `<div class="nav-row${id === page ? " selected" : ""}" role="link">${icon(ic)}<span>${label}</span></div>`
    ).join("");

    const win = document.createElement("div");
    win.className = narrow ? "window narrow" : "window";
    win.innerHTML = `
      ${narrow ? "" : `<aside class="sidebar-pane" aria-label="Pasek boczny">
        <div class="headerbar">
          <div class="start"></div>
          <div class="wtitle"><div class="t">Baseboard</div></div>
          <div class="end"><span class="btn flat image" title="Menu główne">${icon("menu")}</span></div>
        </div>
        <nav class="nav">${nav}</nav>
        <div class="sidebar-footer">
          <div class="lines caption dim">
            <div>MAG B850M MORTAR WIFI</div>
          </div>
        </div>
      </aside>`}
      <section class="content-pane">
        <div class="headerbar">
          <div class="start">${body.dataset.back || narrow ? `<span class="btn flat image" title="Wstecz">${icon("back")}</span>` : ""}</div>
          <div class="wtitle">
            <div class="t">${body.dataset.title}</div>
            ${body.dataset.subtitle ? `<div class="s">${body.dataset.subtitle}</div>` : ""}
          </div>
          <div class="end">
            ${headEnd ? headEnd.innerHTML : ""}
            <div class="windowcontrols">
              <div class="wbtn" title="Minimalizuj"><span>${icon("minimize")}</span></div>
              <div class="wbtn" title="Maksymalizuj"><span>${icon("maximize")}</span></div>
              <div class="wbtn" title="Zamknij"><span>${icon("close")}</span></div>
            </div>
          </div>
        </div>
      </section>`;

    const pane = win.querySelector(".content-pane");
    if (banner) {
      const b = document.createElement("div");
      b.className = "banner";
      b.innerHTML = banner.innerHTML;
      pane.appendChild(b);
    }
    pane.appendChild(content);
    if (bottom) {
      const b = document.createElement("div");
      b.className = "bottombar";
      b.innerHTML = bottom.innerHTML;
      pane.appendChild(b);
      bottom.remove();
    }
    if (headEnd) headEnd.remove();
    if (banner) banner.remove();
    body.prepend(win);

    replaceIcons();
    finish();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", build);
  } else {
    build();
  }
})();
