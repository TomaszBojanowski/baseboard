// SPDX-License-Identifier: GPL-3.0-or-later

// The "Curve Details" page of D62: as in 0.2.0 before it, plus what the
// list of fans no longer shows: the mode, the minimum, the legend of the
// chart and the notes of a header nobody checked or that does not turn.
// The header label stays in the subtitle, so no row repeats it.
// ?wentylator=cpu1|pump1, ?stan=spoczynek|85, ?w=, ?h=, ?bare=1.

(function () {
  const { NB, pct, chart, dutyAt } = window.fanChart;
  const params = new URLSearchParams(location.search);
  const key = params.get("wentylator") || "cpu1";
  const hot = params.get("stan") === "85";
  const winWidth = parseInt(params.get("w") || "0", 10);
  const fixedHeight = parseInt(params.get("h") || "0", 10);
  if (params.get("bare") === "1") document.documentElement.dataset.bare = "1";
  window.afterBuild = window.afterBuild || [];
  const fill7 = pts => pts.concat(Array(7 - pts.length).fill(pts[pts.length - 1]));

  const FANS = {
    cpu1: {
      name: "Wentylator procesora 1", header: "CPU_FAN1", min: 30, needsFull: true,
      curve: fill7([[0, 122], [75, 124], [80, 191], [85, 255]]),
      duty: hot ? 200 : 124,
    },
    pump1: {
      name: "Pompa 1", header: "PUMP_SYS1", min: 50, needsFull: true, unchecked: true, empty: true,
      curve: fill7([[35, 255], [40, 255], [70, 255], [90, 255]]),
      duty: 255,
    },
  };
  const f = FANS[key];
  const temp = hot ? 85 : 50;
  const target = Math.ceil(dutyAt(f.curve, temp));
  document.body.dataset.title = f.name;
  document.body.dataset.subtitle = `${f.header} · podgląd`;

  // Runs of equal points as one row, "4–7".
  const runs = [];
  f.curve.forEach((p, i) => {
    const last = runs[runs.length - 1];
    if (last && last.p[0] === p[0] && last.p[1] === p[1]) last.to = i;
    else runs.push({ from: i, to: i, p });
  });
  const points = runs.map(r => `<div class="row pt-row">
      <span class="pt-n${r.to > r.from ? " wide" : ""}">${r.to > r.from ? `${r.from + 1}–${r.to + 1}` : r.from + 1}</span>
      <div class="pt-ro"><span>${r.p[0]}${NB}°C</span><span>${pct(r.p[1])}${NB}%</span></div>
    </div>`).join("");

  const swatch = cls => `<span class="sw-${cls}"></span>`;
  const legend = `<div class="d-legend" role="group" aria-label="Legenda wykresu">
      <span class="li">${swatch("hatch")}poniżej minimum złącza</span>
      <span class="li">${swatch("threshold")}próg awaryjny</span>
      <span class="li">${swatch("now")}teraz: temperatura i wypełnienie</span>
      <span class="li">${swatch("ends")}przedłużenie krzywej</span>
    </div>`;

  const header = `<section class="group">
      <div class="group-header"><div class="labels"><div class="heading">Złącze</div></div></div>
      <div class="boxed-list">
        <div class="row"><div class="tb"><div class="t">Tryb</div></div><div class="suffix"><span class="v">Automat BIOS</span></div></div>
        <div class="row"><div class="tb"><div class="t">Minimum</div><div class="sub">Najniższe wypełnienie, jakie Baseboard dopuści</div></div><div class="suffix"><span class="v">${f.min}${NB}%</span></div></div>
        ${f.unchecked ? `<div class="row"><div class="prefix"><i data-ic="info"></i></div><div class="tb"><div class="t">Złącza nie sprawdzono na tej płycie</div><div class="sub">Nikt jeszcze nie potwierdził, że ten odczyt należy do tego złącza.</div></div></div>` : ""}
        ${f.empty ? `<div class="row"><div class="prefix"><i data-ic="info"></i></div><div class="tb"><div class="t">Nie kręci się od uruchomienia aplikacji</div><div class="sub">Złącze może być puste.</div></div></div>` : ""}
      </div>
    </section>`;

  const source = `<section class="group">
      <div class="group-header"><div class="labels"><div class="heading">Źródło temperatury</div><div class="desc">Według niej kontroler liczy wypełnienie.</div></div></div>
      <div class="boxed-list"><div class="row">
        <div class="prefix"><i data-ic="thermo"></i></div>
        <div class="tb"><div class="t">Procesor (czujnik płyty)</div><div class="sub">nct6687 · temp1</div></div>
        <div class="suffix"><i data-ic="lock" class="dim"></i></div>
      </div></div>
    </section>`;

  const how = `<section class="group">
      <div class="group-header"><div class="labels"><div class="heading">Jak działa krzywa</div></div></div>
      <div class="boxed-list">
        <div class="row"><div class="prefix"><i data-ic="curve"></i></div><div class="tb"><div class="t">Między punktami wypełnienie zmienia się liniowo</div><div class="sub">Poniżej pierwszego punktu i powyżej ostatniego obowiązuje ich wypełnienie.</div></div></div>
        <div class="row"><div class="prefix"><i data-ic="chip"></i></div><div class="tb"><div class="t">Krzywą wykonuje kontroler płyty</div><div class="sub">Aplikacja ani pomocnik nie muszą przy tym działać.</div></div></div>
      </div>
    </section>`;

  document.getElementById("content").innerHTML = `<div class="clamp wide d-page">
      <div class="boxed-list"><div class="row"><div class="prefix"><i data-ic="info"></i></div>
        <div class="tb"><div class="t">Ta wersja Baseboard pokazuje ustawienia wentylatorów bez możliwości zmiany.</div></div></div></div>
      <div class="card d-chart"><div data-big></div>${legend}</div>
      <div class="d-cols">
        <section class="group">
          <div class="group-header"><div class="labels"><div class="heading">Punkty</div><div class="desc">Wartości odczytane z kontrolera płyty.</div></div></div>
          <div class="boxed-list">${points}</div>
        </section>
        <div class="d-side">${header}${source}${how}</div>
      </div>
    </div>`;

  window.afterBuild.unshift(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
    const box = document.querySelector("[data-big]");
    const w = Math.floor(box.clientWidth);
    box.innerHTML = chart({
      w, h: w < 500 ? 250 : 330, uid: "big", points: f.curve, min: f.min, threshold: 95,
      needsFull: f.needsFull, source: "Procesor (czujnik płyty)",
      now: { t: temp, d: f.duty, target: pct(target) !== pct(f.duty) ? target : null },
      label: `Krzywa: ${f.name}`,
    });
  });
})();
