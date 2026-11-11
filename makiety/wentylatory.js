// SPDX-License-Identifier: GPL-3.0-or-later

// Data and building blocks shared by the M2 fan mockups.
//
// Real readings from MS-7E61 on 2026-11-11 (module with the v1 series, the
// owner's BIOS fan settings): speeds, duty cycles, modes, curves of all six
// channels, temp1 46,0 °C. Illustrative, because nothing was written: the
// own curves (USER_CURVE, OLD_CURVE), manual 45 % on SYS_FAN2 and the speeds
// that would follow from them, and every error state.
//
// No text is repeated on one screen: a card says the minimum in its subtitle
// and has no labels on its chart; the source and its threshold stand once,
// in the row above the cards; "header not tested" stands once per card.

(function () {
  const { NB, pct, f1, dutyAt } = window.fanChart;
  window.afterBuild = window.afterBuild || [];

  const params = new URLSearchParams(location.search);
  const narrow = params.get("width") === "narrow";
  const state = params.get("stan") || "";

  const flat = (temps, d) => temps.map(t => [t, d]);
  const fill7 = pts => pts.concat(Array(7 - pts.length).fill(pts[pts.length - 1]));

  const SOURCE = "Procesor (czujnik płyty)";
  const TEMP1 = 46.0;
  const THRESHOLD = 95;
  const MINI_H = narrow ? 136 : 150;

  const CH = {
    cpu1: {
      key: "cpu1", role: "cpu", name: "Wentylator procesora 1", header: "CPU_FAN1", icon: "fan",
      min: 30, rpm: 886, duty: 124, needsFull: true,
      curve: fill7([[0, 122], [75, 124], [80, 191], [85, 255]]),
    },
    pump1: {
      key: "pump1", role: "pump", name: "Pompa 1", header: "PUMP_SYS1", icon: "drop",
      min: 50, rpm: 0, duty: 255, needsFull: true, untested: true,
      curve: fill7([[35, 255], [40, 255], [70, 255], [90, 255]]),
    },
    sys1: {
      key: "sys1", role: "sys", name: "Wentylator systemowy 1", header: "SYS_FAN1", icon: "fan",
      min: 0, rpm: 931, duty: 98, curve: fill7([[0, 94], [5, 96], [84, 99], [85, 255]]),
    },
    sys2: {
      key: "sys2", role: "sys", name: "Wentylator systemowy 2", header: "SYS_FAN2", icon: "fan",
      min: 0, rpm: 777, duty: 109, curve: fill7([[0, 104], [5, 107], [84, 109], [85, 255]]),
    },
    sys3: {
      key: "sys3", role: "sys", name: "Wentylator systemowy 3", header: "SYS_FAN3", icon: "fan",
      min: 0, rpm: 756, duty: 76, curve: fill7(flat([40, 55, 70, 85], 76)),
    },
    sys4: {
      key: "sys4", role: "sys", name: "Wentylator systemowy 4", header: "SYS_FAN4", icon: "fan",
      min: 0, rpm: 1024, duty: 102, curve: fill7(flat([40, 55, 70, 85], 102)),
    },
    ezconn: {
      key: "ezconn", role: "aux", name: "Wentylator dodatkowy 1", header: "JAF_1", icon: "fan",
      min: 0, rpm: 0, duty: 153, untested: true,
    },
  };
  // Default CPU curve of the BIOS (§6.3): point 1 is below the minimum (B5).
  const BIOS_DEFAULT_CPU = fill7([[0, 51], [45, 89], [65, 178], [80, 255]]);
  // Illustrative own curves: one applied earlier, one being edited.
  const OLD_CURVE = [[30, 51], [50, 76], [60, 115], [70, 153], [80, 204], [85, 255], [85, 255]];
  const USER_CURVE = [[35, 51], [50, 64], [60, 89], [68, 127], [78, 191], [85, 255], [85, 255]];

  // ---------- formatting ----------

  const P = v => `${v}${NB}%`;
  const C = v => `${v}${NB}°C`;

  // ---------- charts: placeholders filled once the window has its size ----------

  const charts = [];
  function chartBox(o, cls = "fc-chart") {
    charts.push(o);
    return `<div class="${cls}" data-chart="${charts.length - 1}"></div>`;
  }
  window.afterBuild.push(() => {
    document.querySelectorAll("[data-chart]").forEach(el => {
      const o = charts[el.dataset.chart];
      const w = Math.floor(el.clientWidth);
      const h = typeof o.h === "function" ? o.h(w) : o.h;
      const conf = { ...o, w, h, uid: `c${el.dataset.chart}` };
      el.innerHTML = window.fanChart.chart(conf);
      o.nowShown = conf.nowShown;
    });
  });

  function chartLabel(c, points) {
    const p = points.map(([t, d], i) => `punkt ${i + 1}: ${C(t)}, ${P(pct(d))}`).join("; ");
    return `Krzywa: ${c.name}, wypełnienie według temperatury ${SOURCE.toLowerCase()}. ${p}. Teraz ${C(f1(TEMP1))}.`;
  }

  // ---------- pieces ----------

  // Mode switch; insensitive in the preview, where it only shows the mode.
  function toggles(n, mode, insensitive = false) {
    if (!n) return "";
    const all = [["bios", "Automat BIOS"], ["manual", "Ręczny"], ["curve", "Krzywa"]].slice(0, n);
    return `<div class="toggle-group full${insensitive ? " insensitive" : ""}" role="radiogroup" aria-label="Tryb"${insensitive ? ' aria-disabled="true"' : ""}>${all.map(([m, l]) =>
      `<span class="toggle${m === mode ? " checked" : ""}" role="radio">${l}</span>`).join("")}</div>`;
  }

  // "złącze nietestowane" only where no other line of the card says it; the
  // minimum only where something can be controlled or a chart shows it.
  function subtitle(c, { untested = c.untested, min = true } = {}) {
    const parts = [c.header];
    if (min && c.min > 0) parts.push(`minimum ${P(c.min)}`);
    if (untested) parts.push("złącze nietestowane");
    return parts.join(" · ");
  }

  function head(c, o = {}) {
    const rpm = o.rpm != null ? o.rpm : c.rpm;
    const duty = o.duty != null ? o.duty : c.duty;
    const value = o.value || `<div class="rpm${rpm === 0 ? " dim" : ""}"><span class="n">${rpm}</span><span class="u">${NB}rpm</span></div>`;
    const dutyLine = o.dutyLine !== undefined ? o.dutyLine : (duty != null ? `wypełnienie ${P(pct(duty))}` : "");
    return `<div class="fc-head">
      <div class="fc-ic"><i data-ic="${c.icon}"></i></div>
      <div class="tb"><div class="heading">${c.name}</div><div class="caption soft">${o.sub || subtitle(c)}</div></div>
      <div class="fc-val">${value}${dutyLine ? `<div class="caption soft">${dutyLine}</div>` : ""}</div>
    </div>`;
  }

  function miniChart(c, o = {}) {
    const points = o.points || c.curve;
    return chartBox({
      mini: true, h: MINI_H, points, min: c.min, threshold: THRESHOLD, needsFull: c.needsFull,
      now: { t: TEMP1, d: o.duty != null ? o.duty : c.duty, target: o.target },
      label: chartLabel(c, points),
    });
  }

  // A footer: an optional note on the left and the button that opens the
  // curve subpage.
  function foot(left, button) {
    return `<div class="fc-foot">${left}<span class="spacer"></span>${button ? `<span class="btn flat small">${button}<i data-ic="next"></i></span>` : ""}</div>`;
  }


  function scale(c, duty) {
    const p = pct(duty);
    const floor = c.min > 0 ? `<div class="floor" style="width:${c.min}%"></div>` : "";
    return `<div class="scale" role="slider" aria-label="Wypełnienie: ${c.name}" aria-valuenow="${p}">
        <div class="trough"></div>${floor}<div class="fill" style="width:${p}%"></div><div class="knob" style="left:${p}%"></div>
      </div>`;
  }

  // Manual mode: the duty cycle large, the slider under it, in a box as high
  // as the chart of the other cards, so that the grid stays even.
  function manualPanel(c, duty) {
    return `<div class="fc-manual" style="height:${MINI_H}px">
      <div class="mv"><span class="n">${pct(duty)}</span><span class="u">${NB}%</span></div>
      <div class="caption soft">wypełnienie</div>
      ${scale(c, duty)}
    </div>`;
  }

  function manualFoot(c) {
    return foot(c.min > 0 ? "" : `<span class="caption soft">0${NB}% zatrzymuje wentylator</span>`, "");
  }

  // In place of the mode switch, when the header cannot be controlled.
  function lockRow(iconName, text, sub) {
    return `<div class="fc-lock"><i data-ic="${iconName}"></i><div class="grow"><span class="caption">${text}</span>${sub ? `<span class="caption soft">${sub}</span>` : ""}</div></div>`;
  }

  function infoLine(iconName, text) {
    return `<div class="fc-line info"><i data-ic="${iconName}"></i><span class="caption">${text}</span></div>`;
  }

  const EMPTY = "Brak obrotów od startu aplikacji: złącze może być puste.";

  function notice(kind, iconName, title, text, button) {
    return `<div class="notice ${kind}"><i data-ic="${iconName}" class="${kind === "info" || kind === "accent" ? "" : kind}"></i>
      <div class="grow"><div${text ? ' class="heading"' : ""}>${title}</div>${text ? `<div class="caption">${text}</div>` : ""}</div>
      ${button ? `<span class="btn small">${button}</span>` : ""}</div>`;
  }

  function card(parts, cls = "") {
    return `<div class="card fan-card ${cls}">${parts.join("")}</div>`;
  }

  // ---------- rows shared with the Overview ----------

  // The one row with the source of the curves, above the cards: value,
  // min–max range and emergency threshold. A card names a source only when
  // it differs from this one (not on MS-7E61, where all curves use temp1).
  function sourceGroup() {
    return `<section class="group">
      <div class="group-header"><div class="labels">
        <div class="heading">Źródło temperatury</div>
        <div class="desc">Według tej temperatury kontroler płyty liczy wypełnienie z krzywych.</div>
      </div></div>
      <div class="boxed-list">
        <div class="row src"><div class="prefix"><i data-ic="thermo"></i></div>
          <div class="tb"><div class="t">${SOURCE}</div><div class="sub">nct6687 · temp1 · próg awaryjny ${C(THRESHOLD)}</div></div>
          <div class="suffix"><span class="v-range">41,0–48,0${NB}°C</span><span class="v-now">${C(f1(TEMP1))}</span></div></div>
      </div>
    </section>`;
  }

  function readOnlyGroup() {
    const c = CH.ezconn;
    return `<section class="group">
      <div class="group-header"><div class="labels">
        <div class="heading">Tylko odczyt</div>
        <div class="desc">Sterownik jądra nie obsługuje sterowania tym złączem.</div>
      </div></div>
      <div class="boxed-list">
        <div class="row"><div class="prefix"><i data-ic="fan" class="dim"></i></div>
          <div class="tb"><div class="t">${c.name}</div><div class="sub">${c.header} · wypełnienie ${P(pct(c.duty))} · złącze nietestowane</div></div>
          <div class="suffix"><span class="v-now muted">0${NB}rpm</span></div></div>
      </div>
    </section>`;
  }

  function statusRow(iconName, iconCls, title, sub, button) {
    return `<div class="boxed-list"><div class="row">
      <div class="prefix"><i data-ic="${iconName}" class="${iconCls}"></i></div>
      <div class="tb"><div class="t">${title}</div>${sub ? `<div class="sub">${sub}</div>` : ""}</div>
      ${button ? `<div class="suffix">${button}</div>` : ""}
    </div></div>`;
  }

  window.FANS = {
    NB, pct, f1, dutyAt, P, C, narrow, state, MINI_H,
    SOURCE, TEMP1, THRESHOLD, CH, BIOS_DEFAULT_CPU, OLD_CURVE, USER_CURVE, EMPTY,
    chartBox, chartLabel, toggles, subtitle, head, miniChart, foot, scale, manualPanel, manualFoot,
    lockRow, infoLine, notice, card, sourceGroup, readOnlyGroup, statusRow,
  };
})();
