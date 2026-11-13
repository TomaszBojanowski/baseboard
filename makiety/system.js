// SPDX-License-Identifier: GPL-3.0-or-later

// The System page of D64 and D67, after "About" in GNOME Settings: one
// status row on top in the words of the Overview's status sentence, which
// opens "Diagnostics"; a short list of the computer with names and values
// side by side, each part with its icon; every technical detail one click
// further. From 880 px of content the groups stand in two columns: the
// computer and the system on the left, disks, network and the technical
// details on the right; on the two views each group goes to the column
// with fewer rows so far. The values of this machine (2026-11-11).
// ?stan=spoczynek (default), napiecie (+12 V outside its tolerance), stoi
// (the CPU fan stopped, D65), fabryczny (driver of the Fedora kernel);
// ?widok=glowny|diagnostyka|szczegoly; ?theme=, ?width=narrow, ?w=, ?h=,
// ?bare=1.

(function () {
  const NB = " ";
  const params = new URLSearchParams(location.search);
  const state = params.get("stan") || "spoczynek";
  const factory = state === "fabryczny";
  const view = params.get("widok") || "glowny";
  const winWidth = parseInt(params.get("w") || "0", 10);
  const fixedHeight = parseInt(params.get("h") || "0", 10);
  if (params.get("bare") === "1") document.documentElement.dataset.bare = "1";
  window.afterBuild = window.afterBuild || [];

  // The problems of the readings from the Overview, the most important
  // first, as its status sentence names them.
  const alerts = {
    napiecie: [`Napięcie +12${NB}V poniżej normy: 11,20${NB}V`],
    stoi: ["Wentylator procesora stoi"],
  }[state] || [];
  const readOnly = "Ten system na razie tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa.";

  const row = (t, v, o = {}) => `<div class="n-row${v ? " prop" : ""}${o.cls ? " " + o.cls : ""}">
      ${o.badge ? `<span class="badge small"><i data-ic="${o.badge}"></i></span>` : ""}
      ${o.icon ? `<i data-ic="${o.icon}" class="${o.iconCls || ""}"></i>` : ""}
      <div class="grow"><div class="t">${t}</div>${o.s ? `<div class="s${o.mono ? " mono" : ""}">${o.s}</div>` : ""}</div>
      ${v ? `<div class="v">${v}</div>` : ""}
      ${o.chev ? `<i data-ic="chevron" class="chev"></i>` : ""}
    </div>`;
  // A group, with the rows it counts for the column with fewer rows (its
  // title counts as one).
  const group = (title, rows, desc) => ({
    rows: rows.length + 1,
    html: `<section class="n-group">
      ${title ? `<div class="heading">${title}</div>` : ""}${desc ? `<div class="desc">${desc}</div>` : ""}
      <div class="boxed-list">${rows.join("")}</div></section>`,
  });
  // One column in the order of the groups; two side by side from 880 px:
  // the groups with a side on it, the others in the column with fewer rows.
  function columns(groups) {
    const left = [], right = [];
    let leftRows = 0, rightRows = 0;
    for (const g of groups) {
      const toRight = g.side ? g.side === "right" : rightRows < leftRows;
      if (toRight) { right.push(g.html); rightRows += g.rows; } else { left.push(g.html); leftRows += g.rows; }
    }
    return `<div class="n-cols"><div>${left.join("")}</div><div>${right.join("")}</div></div>`;
  }
  const at = (side, g) => ({ ...g, side });

  function main() {
    const [kind, icon, t, s] = alerts.length
      ? ["warn", "warning", alerts[0], alerts.length > 1 ? `I jeszcze ${alerts.length - 1} problem` : ""]
      : factory
        ? ["info", "info", "Odczyty działają", readOnly]
        : ["ok", "ok", "Wszystko w normie", "Czujniki, sterownik i konfiguracja w porządku"];
    const status = `<div class="o-status ${kind}" role="button">
        <i data-ic="${icon}"></i>
        <div class="grow"><div class="t">${t}</div>${s ? `<div class="s">${s}</div>` : ""}</div>
        <i data-ic="chevron" class="chev"></i></div>`;
    // In one column: the computer, disks, network, the system, the details.
    return `${status}${columns([
      at("left", group("Ten komputer", [
        row("Płyta", "MAG B850M MORTAR WIFI", { badge: "board" }),
        row("BIOS", `1.A68 z${NB}7 września 2026`, { badge: "package" }),
        row("Procesor", "AMD Ryzen 7 9700X", { badge: "cpu" }),
        row("Pamięć", `32,0${NB}GiB DDR5 · 6000${NB}MT/s`, { badge: "memory" }),
        row("Karta graficzna", "Radeon RX 9050 / 9060 XT", { badge: "gpu" }),
      ])),
      at("right", group("Dyski", [
        row("WD_BLACK SN7100 2TB", `2,0${NB}TB`),
        row("CT2000P3PSSD8", `2,0${NB}TB`),
        row("SanDisk 3.2Gen1", `61,5${NB}GB · wymienny nośnik`),
      ])),
      at("right", group("Sieć", [
        row("RTL8126 5GbE Controller", ""),
        row("FastConnect 7800", ""),
      ])),
      at("left", group("", [row("System", "Fedora Linux 45 (Workstation Edition)")])),
      at("right", group("", [row("Szczegóły techniczne", "", { s: "Kody, wersje, sterowniki i identyfikatory do zgłoszenia błędu", chev: true })])),
    ])}`;
  }

  function diagnostics() {
    const rows = factory ? [
      row("Monitor sprzętu", "", { icon: "ok", iconCls: "ok", s: "Sterownik nct6683 załadowany · moduł z jądra" }),
      row("Sterowanie wentylatorami niedostępne", "", { icon: "info", iconCls: "info", s: readOnly }),
    ] : [
      row("Monitor sprzętu", "", { icon: "ok", iconCls: "ok", s: "Sterownik nct6683 załadowany · moduł spoza jądra" }),
      row("Sterowanie wentylatorami", "", { icon: "ok", iconCls: "ok", s: "Krzywe w kontrolerze płyty" }),
    ];
    const readings = alerts.length
      ? [group("Odczyty", alerts.map(a => row(a, "", { icon: "warning", iconCls: "warn" })))]
      : [];
    return columns([
      ...readings,
      group("Czujniki i wentylatory", [
        ...rows,
        row("Biblioteka lm-sensors", "", { icon: "ok", iconCls: "ok", s: "Wersja 3.6.2: odczyty z konfiguracją lm-sensors" }),
        row("Konfiguracja lm-sensors", "", { icon: "ok", iconCls: "ok", s: "/etc/sensors.d/local.conf<br>/etc/sensors.d/msi-b850m-mortar.conf", mono: true }),
        row("Brak innych programów sterujących wentylatorami", "", { icon: "ok", iconCls: "ok", s: "Sprawdzono: fancontrol, coolercontrold, fan2go, thinkfan" }),
      ]),
      group("Profil płyty", [
        row("Sprawdzony dla tej rewizji PCB", "", { icon: "ok", iconCls: "ok", s: "Profil płyty: ms-7e61.toml" }),
      ]),
    ]);
  }

  function details() {
    return columns([
      group("Płyta", [
        row("Model", "MAG B850M MORTAR WIFI (MS-7E61)"),
        row("Producent", "Micro-Star International Co., Ltd."),
        row("Kod płyty i rewizja PCB", "MS-7E61, rewizja 1"),
      ]),
      group("BIOS", [
        row("Wersja", "1.A68 z 7 września 2026"),
        row("Producent", "American Megatrends International, LLC."),
      ]),
      group("Procesor i system", [
        row("Procesor", "AMD Ryzen 7 9700X"),
        row("Sterownik częstotliwości procesora", "amd-pstate-epp"),
        row("System", "Fedora Linux 45 (Workstation Edition)"),
        row("Jądro", "7.3.3-300.fc45.x86_64"),
      ]),
      group("Pamięć", [
        row("DIMMA1", "Puste"),
        row("DIMMA2", "16,0 GiB DDR5 · 6000 MT/s · IR-6000D564L30S/16G"),
        row("DIMMB1", "Puste"),
        row("DIMMB2", "16,0 GiB DDR5 · 6000 MT/s · IR-6000D564L30S/16G"),
      ]),
      group("Karta graficzna", [
        row("VBIOS", "113-EXT118704-100"),
        row("Sterownik", "amdgpu"),
        row("Identyfikatory PCI", "1002:7590, podsystem 148c:2437"),
      ], "Radeon RX 9050 / 9060 XT"),
    ]);
  }

  const titles = { glowny: "System", diagnostyka: "Diagnostyka", szczegoly: "Szczegóły techniczne" };
  document.body.dataset.title = titles[view];
  if (view !== "glowny") document.body.dataset.back = "1";
  const body = { glowny: main, diagnostyka: diagnostics, szczegoly: details }[view]();
  document.getElementById("content").innerHTML = `<div class="n-page"><div class="n-clamp wide">${body}</div></div>`;

  window.afterBuild.unshift(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
  });
})();
