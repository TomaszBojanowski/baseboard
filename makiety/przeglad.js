// SPDX-License-Identifier: GPL-3.0-or-later

// The Overview of D67, "a card for each part of the computer": the status
// sentence on top; the cards Processor, Graphics card and Board
// (karta.js); the tiles Fans (with a bar for each fan), Memory and Disks;
// the row "All sensors". The content grows with the window up to 1320 px
// and folds into one column in a narrow window.
// ?stan=spoczynek (default), obciazenie (processor 85 °C, the fans speed
// up), goraco (processor 93 °C, 5 °C or less to its limit of 95 °C: the
// status warns), napiecie (+12 V at 11.20 V), stoi (the CPU fan stopped,
// D65), bezprofilu (a board without a profile), bezgpu (no graphics card);
// ?theme=, ?width=narrow, ?w=, ?h=, ?bare=1. Values as in the test tree of
// the app (przeglad-dane.js); the histories are made up, ending at the
// value shown.

(function () {
  const { NB, whole, history, processor, card } = window.Karta;
  const P = window.PRZEGLAD;
  const params = new URLSearchParams(location.search);
  const stateName = params.get("stan") || "spoczynek";
  const winWidth = parseInt(params.get("w") || "0", 10);
  const fixedHeight = parseInt(params.get("h") || "0", 10);
  if (params.get("bare") === "1") document.documentElement.dataset.bare = "1";
  window.afterBuild = window.afterBuild || [];

  const base = {
    cpu: P.tctlN, gpu: P.gpuN, board: P.systemN, mem: P.dimm2, disk: P.nvme0,
    cpuFan: 890, sysFans: [948, 777, 753, 1013], gpuPower: 12,
    highest: { cpu: 78, gpu: 34, board: 39 },
    rail: null, stopped: false, profile: true, hasGpu: true, load: false, hot: false,
  };
  const STATES = {
    spoczynek: {},
    obciazenie: { cpu: 85.3, board: 39.0, gpu: 31.0, cpuFan: 1520, sysFans: [1480, 1390, 753, 1013], load: true,
      highest: { cpu: 86, gpu: 34, board: 40 } },
    goraco: { cpu: 93.2, board: 41.0, gpu: 31.0, cpuFan: 1880, sysFans: [1920, 1860, 753, 1013], load: true, hot: true,
      highest: { cpu: 93, gpu: 34, board: 41 } },
    napiecie: { rail: { name: `+12${NB}V`, value: `11,20${NB}V`, range: `11,40–12,60${NB}V` } },
    stoi: { cpu: 69.6, cpuFan: 0, stopped: true },
    bezprofilu: { profile: false },
    bezgpu: { hasGpu: false },
  };
  const S = { ...base, ...STATES[stateName] };
  const quantize = (v, step) => Math.round(v / step) * step;
  const SERIES = {
    cpu: processor(S.cpu, S),
    // Whole degrees from amdgpu, half degrees from the board.
    gpu: history(11, S.gpu, (i, n) => 30.1 + 0.55 * Math.sin(i / 70) + 0.08 * n + (S.load && i > 200 ? (i - 200) / 100 : 0)).map(v => quantize(v, 1)),
    board: history(13, S.board, (i, n) => 37.6 + 0.45 * Math.sin(i / 90 + 2) + 0.05 * n + (S.load && i > 200 ? 1.5 * (1 - Math.exp(-(i - 200) / 40)) : 0)).map(v => quantize(v, 0.5)),
  };

  // The status sentence, by the rule of D67: a stopped CPU fan, a
  // temperature 5 °C or less to its limit, a rail outside its tolerance;
  // without a profile "readings work"; else all fine, and the fans speed
  // up when a duty cycle rose.
  function status() {
    if (S.stopped) {
      return { kind: "warn", icon: "warning", t: "Wentylator procesora stoi",
        s: `Kręcił się wcześniej, a od co najmniej 10${NB}s ma 0${NB}rpm.` };
    }
    if (S.hot) {
      return { kind: "warn", icon: "warning", t: `Procesor jest bardzo gorący: ${whole(S.cpu)}${NB}°C`,
        s: `Przy 95${NB}°C zaczyna zwalniać, żeby się nie przegrzać. Wentylatory pracują na pełnych obrotach.` };
    }
    if (S.rail) {
      return { kind: "warn", icon: "warning", t: `Napięcie ${S.rail.name} poniżej normy: ${S.rail.value}`,
        s: `Norma to ${S.rail.range}. Odczyt z płyty bywa niedokładny; jeśli komputer jest niestabilny, warto sprawdzić zasilacz.` };
    }
    if (!S.profile) {
      return { kind: "info", icon: "info", t: "Odczyty działają",
        s: "Ta płyta nie ma jeszcze profilu, więc część odczytów jest bez nazw i bez oceny." };
    }
    return { kind: "ok", icon: "ok", t: "Wszystko w normie",
      s: S.load ? "Wentylatory przyspieszają." : "Temperatury, wentylatory i zasilanie bez zastrzeżeń." };
  }
  const st = status();
  const statusRow = `<div class="o-status ${st.kind}" role="${st.kind === "warn" ? "alert" : "status"}">
      <i data-ic="${st.icon}"></i>
      <div class="grow"><div class="t">${st.t}</div><div class="s">${st.s}</div></div>
      ${st.kind === "warn" ? `<i data-ic="chevron" class="chev"></i>` : ""}
    </div>`;

  // Only the cards there are data for: without a profile the app does not
  // know the board's temperature nor which fan is the CPU fan.
  const cards = [
    { name: "Procesor", icon: "cpu", key: "cpu", hot: S.hot,
      facts: S.profile ? [["Wentylator procesora", S.stopped ? "Stoi" : `${S.cpuFan}${NB}rpm`, S.stopped]] : [] },
    S.hasGpu && { name: "Karta graficzna", icon: "gpu", key: "gpu", facts: [["Pobór mocy", `${S.gpuPower}${NB}W`]] },
    S.profile && { name: "Płyta", icon: "board", key: "board",
      facts: [S.rail ? ["Zasilanie", `${S.rail.name}: ${S.rail.value}`, true] : ["Zasilanie", "W normie"]] },
  ].filter(Boolean);

  // A bar for each fan, the CPU fan first; together as wide as six bars of
  // 10 px, however many fans there are.
  const fans = [S.cpuFan, 0, ...S.sysFans];
  const spinning = fans.filter(rpm => rpm > 0).length;
  const bars = `<div class="bars" aria-hidden="true">${fans.map((rpm, i) =>
    `<span class="${rpm === 0 ? (i === 0 && S.stopped ? "warn" : "off") : ""}" style="height:${Math.max(10, rpm / 1600 * 100).toFixed(0)}%"></span>`).join("")}</div>`;
  const tiles = [
    S.profile && { icon: "fan", t: "Wentylatory", v: `${spinning}${NB}${spinning > 1 && spinning < 5 ? "kręcą się" : "kręci się"}`,
      s: S.stopped ? "Wentylator procesora stoi" : S.load ? "Przyspieszają" : `Od 750 do 1010${NB}rpm`, warn: S.stopped, extra: bars },
    { icon: "memory", t: "Pamięć", v: `${S.mem}${NB}°C`, s: "Najcieplejszy z 2 modułów" },
    { icon: "drive", t: "Dyski", v: `${S.disk}${NB}°C`, s: "Najcieplejszy z 2 dysków" },
  ].filter(Boolean);

  document.getElementById("content").innerHTML = `<div class="o-page">
      ${statusRow}
      <div class="o-cards n${cards.length}">${cards.map(c => card({
        ...c, value: S[c.key], values: SERIES[c.key], highest: S.highest[c.key], opens: true,
      })).join("")}</div>
      <div class="o-tiles n${tiles.length}">${tiles.map(i => `
        <div class="tile${i.warn ? " warn" : ""}" role="button">
          <span class="badge${i.warn ? " warn" : ""}"><i data-ic="${i.warn ? "warning" : i.icon}"></i></span>
          <div class="grow"><div class="tile-t">${i.t}</div><div class="tile-v">${i.v}</div><div class="tile-s">${i.s}</div></div>
          ${i.extra || ""}
        </div>`).join("")}</div>
      <div class="boxed-list o-all"><div class="n-row" role="button">
        <i data-ic="list"></i>
        <div class="grow"><div class="t">Wszystkie czujniki</div><div class="s">Każdy odczyt ze źródłem i zakresem od uruchomienia aplikacji</div></div>
        <i data-ic="chevron" class="chev"></i></div></div>
    </div>`;

  window.afterBuild.unshift(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
    if (!S.profile) {
      const footer = win.querySelector(".sidebar-footer .lines div");
      if (footer) footer.textContent = "PRO B650M-A WIFI (MS-7D77)";
    }
  });
})();
