// SPDX-License-Identifier: GPL-3.0-or-later

// The System page of D64, after "About" in GNOME Settings: one status row
// on top that opens "Diagnostics", a short list of the computer with names
// and values side by side, and every technical detail one click further.
// The values of this machine (2026-11-11), as before D64.
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

  // The warnings of the readings from the Overview.
  const alerts = {
    napiecie: [`Napięcie +12${NB}V poniżej normy`],
    stoi: ["Wentylator procesora stoi"],
  }[state] || [];

  const row = (t, v, o = {}) => `<div class="n-row${v ? " prop" : ""}${o.cls ? " " + o.cls : ""}">
      ${o.icon ? `<i data-ic="${o.icon}" class="${o.iconCls || ""}"></i>` : ""}
      <div class="grow"><div class="t">${t}</div>${o.s ? `<div class="s${o.mono ? " mono" : ""}">${o.s}</div>` : ""}</div>
      ${v ? `<div class="v">${v}</div>` : ""}
      ${o.chev ? `<i data-ic="chevron" class="chev"></i>` : ""}
    </div>`;
  const group = (title, rows, desc) => `<section class="n-group">
      ${title ? `<div class="heading">${title}</div>` : ""}${desc ? `<div class="desc">${desc}</div>` : ""}
      <div class="boxed-list">${rows.join("")}</div></section>`;

  function main() {
    const status = alerts.length
      ? row("Wymaga uwagi", "", { cls: "status", icon: "warning", iconCls: "warn", s: alerts[0], chev: true })
      : factory
        ? row("Czujniki działają", "", { cls: "status", icon: "info", iconCls: "info", s: "Ten system na razie tylko odczytuje wentylatory", chev: true })
        : row("Wszystko działa", "", { cls: "status", icon: "ok", iconCls: "ok", s: "Czujniki, sterownik i konfiguracja w porządku", chev: true });
    return `
      ${group("", [status])}
      ${group("Ten komputer", [
        row("Płyta", "MAG B850M MORTAR WIFI"),
        row("BIOS", `1.A68 z${NB}07.09.2026`),
        row("Procesor", "AMD Ryzen 7 9700X"),
        row("Pamięć", `32,0${NB}GiB DDR5 · 6000${NB}MT/s`),
        row("Karta graficzna", "Radeon RX 9050 / 9060 XT"),
      ])}
      ${group("Dyski", [
        row("WD_BLACK SN7100 2TB", `2,0${NB}TB`),
        row("CT2000P3PSSD8", `2,0${NB}TB`),
        row("SanDisk 3.2Gen1", `61,5${NB}GB · wymienny nośnik`),
      ])}
      ${group("Sieć", [
        row("RTL8126 5GbE Controller", ""),
        row("FastConnect 7800", ""),
      ])}
      ${group("", [row("System", "Fedora Linux 45 (Workstation Edition)")])}
      ${group("", [row("Szczegóły techniczne", "", { s: "Kody, wersje, sterowniki i identyfikatory do zgłoszenia błędu", chev: true })])}`;
  }

  function diagnostics() {
    const rows = factory ? [
      row("Monitor sprzętu", "", { icon: "ok", iconCls: "ok", s: "Sterownik nct6683 załadowany · moduł z jądra" }),
      row("Sterowanie wentylatorami niedostępne", "", { s: "Sterownik w tym jądrze tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa." }),
    ] : [
      row("Monitor sprzętu", "", { icon: "ok", iconCls: "ok", s: "Sterownik nct6683 załadowany · moduł spoza jądra" }),
      row("Sterowanie wentylatorami", "", { icon: "ok", iconCls: "ok", s: "Krzywe w EC" }),
    ];
    const readings = alerts.length
      ? group("Odczyty", alerts.map(a => row(a, "", { icon: "warning", iconCls: "warn" })))
      : "";
    return `
      ${readings}
      ${group("Czujniki i wentylatory", [
        ...rows,
        row("Biblioteka lm-sensors", "", { icon: "ok", iconCls: "ok", s: "Wersja 3.6.2: odczyty z konfiguracją lm-sensors" }),
        row("Konfiguracja lm-sensors", "", { icon: "ok", iconCls: "ok", s: "/etc/sensors.d/local.conf<br>/etc/sensors.d/msi-b850m-mortar.conf", mono: true }),
        row("Brak innych programów sterujących wentylatorami", "", { icon: "ok", iconCls: "ok", s: "Sprawdzono: fancontrol, coolercontrold, fan2go, thinkfan" }),
      ])}
      ${group("Profil płyty", [
        row("Sprawdzony dla tej rewizji PCB", "", { icon: "ok", iconCls: "ok", s: "Profil płyty: ms-7e61.toml" }),
      ])}`;
  }

  function details() {
    return `
      ${group("Płyta", [
        row("Model", "MAG B850M MORTAR WIFI (MS-7E61)"),
        row("Producent", "Micro-Star International Co., Ltd."),
        row("Kod płyty i rewizja PCB", "MS-7E61, rewizja 1"),
      ])}
      ${group("BIOS", [
        row("Wersja", "1.A68 z 07.09.2026"),
        row("Producent", "American Megatrends International, LLC."),
      ])}
      ${group("Procesor i system", [
        row("Procesor", "AMD Ryzen 7 9700X"),
        row("Sterownik częstotliwości procesora", "amd-pstate-epp"),
        row("System", "Fedora Linux 45 (Workstation Edition)"),
        row("Jądro", "7.3.3-300.fc45.x86_64"),
      ])}
      ${group("Pamięć", [
        row("DIMMA1", "Puste"),
        row("DIMMA2", "16,0 GiB DDR5 · 6000 MT/s · IR-6000D564L30S/16G"),
        row("DIMMB1", "Puste"),
        row("DIMMB2", "16,0 GiB DDR5 · 6000 MT/s · IR-6000D564L30S/16G"),
      ])}
      ${group("Karta graficzna", [
        row("VBIOS", "113-EXT118704-100"),
        row("Sterownik", "amdgpu"),
        row("Identyfikatory PCI", "1002:7590, podsystem 148c:2437"),
      ], "Radeon RX 9050 / 9060 XT")}`;
  }

  const titles = { glowny: "System", diagnostyka: "Diagnostyka", szczegoly: "Szczegóły techniczne" };
  document.body.dataset.title = titles[view];
  if (view !== "glowny") document.body.dataset.back = "1";
  const body = { glowny: main, diagnostyka: diagnostics, szczegoly: details }[view]();
  document.getElementById("content").innerHTML = `<div class="n-page"><div class="n-clamp">${body}</div></div>`;

  window.afterBuild.unshift(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
  });
})();
