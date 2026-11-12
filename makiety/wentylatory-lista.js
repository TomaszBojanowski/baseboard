// SPDX-License-Identifier: GPL-3.0-or-later

// The Fans page of D62: the temperature of the curves' source large on top,
// under it a list of the fans as wide as in GNOME Settings (600 px).
// ?stan=spoczynek (default), 85, reczny, fabryczny, stoi (the CPU fan
// turned and stands now, D65: a warning above the list). Values as in the test
// tree of the app: MS-7E61, BIOS curves of the owner; the manual 45 % of
// SYS_FAN2 is illustrative. ?w=1024 sets the window width, ?h=1100 a
// fixed window height (one screen), ?bare=1 draws the window alone.

(function () {
  const NB = " ";
  const params = new URLSearchParams(location.search);
  const stateName = params.get("stan") || "spoczynek";
  const winWidth = parseInt(params.get("w") || "0", 10);
  const fixedHeight = parseInt(params.get("h") || "0", 10);
  if (params.get("bare") === "1") document.documentElement.dataset.bare = "1";
  window.afterBuild = window.afterBuild || [];

  const pct = v => Math.floor(v * 100 / 255 + 0.5);
  const fill7 = pts => pts.concat(Array(7 - pts.length).fill(pts[pts.length - 1]));
  const flat = (temps, d) => temps.map(t => [t, d]);
  function dutyAt(points, t) {
    if (t <= points[0][0]) return points[0][1];
    for (let i = 1; i < points.length; i++) {
      const [t0, d0] = points[i - 1];
      const [t1, d1] = points[i];
      if (t <= t1) return t1 === t0 ? d1 : d0 + (d1 - d0) * (t - t0) / (t1 - t0);
    }
    return points[points.length - 1][1];
  }

  const FANS = [
    { key: "cpu1", name: "Wentylator procesora 1", header: "CPU_FAN1", min: 30, curve: fill7([[0, 122], [75, 124], [80, 191], [85, 255]]) },
    { key: "pump1", name: "Pompa 1", header: "PUMP_SYS1", min: 50, unchecked: true, curve: fill7([[35, 255], [40, 255], [70, 255], [90, 255]]) },
    { key: "sys1", name: "Wentylator systemowy 1", header: "SYS_FAN1", min: 0, curve: fill7([[0, 94], [5, 96], [84, 99], [85, 255]]) },
    { key: "sys2", name: "Wentylator systemowy 2", header: "SYS_FAN2", min: 0, curve: fill7([[0, 104], [5, 107], [84, 109], [85, 255]]) },
    { key: "sys3", name: "Wentylator systemowy 3", header: "SYS_FAN3", min: 0, curve: fill7(flat([40, 55, 70, 85], 76)) },
    { key: "sys4", name: "Wentylator systemowy 4", header: "SYS_FAN4", min: 0, curve: fill7(flat([40, 55, 70, 85], 102)) },
  ];
  // The driver does not control the extra header; both drivers report its
  // duty cycle (pwm4, 153 = 60 %).
  const AUX = { key: "jaf1", name: "Wentylator dodatkowy 1", header: "JAF_1", min: 0, unchecked: true, readOnly: true };

  // [rpm, duty 0–255, mode]; "manual" only where the board says so.
  const STATES = {
    spoczynek: {
      temp: 50, curves: true,
      fans: { cpu1: [890, 124], pump1: [0, 255], sys1: [948, 98], sys2: [777, 108], sys3: [753, 76], sys4: [1013, 102] },
    },
    85: {
      temp: 85, curves: true,
      fans: { cpu1: [1520, 200], pump1: [0, 255], sys1: [1480, 180], sys2: [1390, 190], sys3: [753, 76], sys4: [1013, 102] },
    },
    reczny: {
      temp: 50, curves: true,
      fans: { cpu1: [890, 124], pump1: [0, 255], sys1: [948, 98], sys2: [905, 115, "manual"], sys3: [753, 76], sys4: [1013, 102] },
    },
    // Driver of the Fedora kernel: speeds; duty cycles only of the CPU fan,
    // the pump and the extra header; no curves.
    fabryczny: {
      temp: 50, curves: false,
      fans: { cpu1: [890, 124], pump1: [0, 255], sys1: [948, null], sys2: [777, null], sys3: [753, null], sys4: [1013, null] },
    },
    stoi: {
      temp: 50, curves: true,
      fans: { cpu1: [0, 124, "stopped"], pump1: [0, 255], sys1: [948, 98], sys2: [777, 108], sys3: [753, 76], sys4: [1013, 102] },
    },
  };
  const S = STATES[stateName];

  // A name never leaves its number alone on a line.
  const keepNumber = name => name.replace(/ (\d+)$/, `${NB}$1`);

  function model(f, [rpm, duty, mode]) {
    const manual = mode === "manual";
    // The controller rounds up (§4).
    const target = S.curves && f.curve && !manual ? Math.ceil(dutyAt(f.curve, S.temp)) : null;
    const moving = target != null && duty != null && pct(target) !== pct(duty);
    // On screen only a change of 5 points or more; screen readers get all.
    const shown = moving && Math.abs(pct(target) - pct(duty)) >= 5;
    let state = "";
    // D65: it turned since the start, so the header is not empty.
    const stopped = mode === "stopped";
    if (stopped) state = "Stoi";
    else if (rpm === 0 && duty !== 0 && !f.readOnly) state = "Nie kręci się – złącze może być puste.";
    else if (manual) state = "Ręcznie";
    else if (shown) state = target > duty ? "Przyspiesza" : "Zwalnia";
    return { ...f, name: keepNumber(f.name), rpm, duty, manual, stopped, target, moving, shown, state };
  }
  const fans = FANS.map(f => model(f, S.fans[f.key]));
  const aux = model(AUX, [0, 153]);
  const all = [...fans, aux];

  // ---------- the small chart: one line, one point ----------

  function linePoints(curve) {
    const pts = [];
    for (const p of curve) {
      const last = pts[pts.length - 1];
      if (!last || last[0] !== p[0] || last[1] !== p[1]) pts.push(p);
    }
    if (pts[0][0] > 0) pts.unshift([0, pts[0][1]]);
    if (pts[pts.length - 1][0] < 100) pts.push([100, pts[pts.length - 1][1]]);
    return pts.filter((p, i) => {
      if (i === 0 || i === pts.length - 1) return true;
      const [a, b] = [pts[i - 1], pts[i + 1]];
      return (p[1] - a[1]) * (b[0] - p[0]) !== (b[1] - p[1]) * (p[0] - a[0]);
    });
  }

  // Straight segments as the controller runs them (a spline would show duty
  // cycles the fan never gets, D45), the corners rounded a little.
  function roundedPath(p, r) {
    const f = v => v.toFixed(1);
    let d = `M${f(p[0][0])},${f(p[0][1])}`;
    for (let i = 1; i < p.length - 1; i++) {
      const [x0, y0] = p[i - 1];
      const [x1, y1] = p[i];
      const [x2, y2] = p[i + 1];
      const l1 = Math.hypot(x1 - x0, y1 - y0);
      const l2 = Math.hypot(x2 - x1, y2 - y1);
      const rr = Math.min(r, l1 / 2, l2 / 2);
      d += ` L${f(x1 + (x0 - x1) * rr / l1)},${f(y1 + (y0 - y1) * rr / l1)}` +
        ` Q${f(x1)},${f(y1)} ${f(x1 + (x2 - x1) * rr / l2)},${f(y1 + (y2 - y1) * rr / l2)}`;
    }
    const e = p[p.length - 1];
    return d + ` L${f(e[0])},${f(e[1])}`;
  }

  // What a screen reader says for the chart: as full as before D62.
  function chartLabel(f) {
    if (f.manual) {
      return `Tryb ręczny: ${f.name}, stałe wypełnienie ${pct(f.duty)}${NB}% przy każdej temperaturze. Teraz ${S.temp}${NB}°C.`;
    }
    const pts = linePoints(f.curve).map(([t, d]) => `${t}${NB}°C: ${pct(d)}${NB}%`).join(", ");
    let s = `Krzywa: ${f.name}, wypełnienie według temperatury procesora (czujnik płyty). Punkty: ${pts}. ` +
      `Teraz ${S.temp}${NB}°C, wypełnienie ${pct(f.duty)}${NB}%.`;
    if (f.moving) s += ` Kontroler zmienia wypełnienie stopniowo, do ${pct(f.target)}${NB}%.`;
    return s;
  }

  function spark(f, w, h) {
    const L = 4, R = 4, T = 5, B = 5;
    const X = t => L + t * (w - L - R) / 100;
    const Y = d => T + (1 - d / 255) * (h - T - B);
    const curve = f.manual ? [[0, f.duty], [100, f.duty]] : f.curve;
    const line = roundedPath(linePoints(curve).map(([t, d]) => [X(t), Y(d)]), 4);
    const area = `${line} L${X(100)},${Y(0)} L${X(0)},${Y(0)} Z`;
    const cx = X(S.temp), cy = Y(f.duty), id = f.key;
    return `<svg class="mc" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${chartLabel(f)}">
      <defs><linearGradient id="g${id}" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" style="stop-color:var(--accent-bg);stop-opacity:.18"/>
        <stop offset="1" style="stop-color:var(--accent-bg);stop-opacity:0"/>
      </linearGradient></defs>
      <path d="${area}" style="fill:url(#g${id})"/>
      <path d="${line}" class="ml"/>
      <circle cx="${cx}" cy="${cy}" r="7" class="mh"/>
      <circle cx="${cx}" cy="${cy}" r="3.5" class="md"/>
    </svg>`;
  }

  // ---------- rows ----------

  const rpmText = f => `${f.rpm}${NB}rpm`;
  const pctText = f => f.duty == null ? "" : `${pct(f.duty)}${NB}%`;

  // The row as a screen reader reads it: everything the list does not
  // show stays in the description.
  function rowDescription(f) {
    const parts = [f.header, rpmText(f)];
    if (f.duty != null) parts.push(`wypełnienie ${pctText(f)}`);
    if (f.moving) parts.push(`kontroler zmienia je stopniowo do ${pct(f.target)}${NB}%`);
    if (S.curves && f.curve) parts.push(`tryb: ${f.manual ? "ręczny" : "automat BIOS"}`);
    if (f.min > 0 && S.curves) parts.push(`minimum ${f.min}${NB}%`);
    if (f.unchecked) parts.push("złącza nie sprawdzono na tej płycie");
    if (f.state && !f.moving && !f.manual) parts.push(f.state);
    return parts.join(", ");
  }

  function row(f) {
    const chart = S.curves && f.curve;
    // Also in manual mode: the curve stays in the board's controller.
    const open = chart;
    return `<div class="r${f.rpm === 0 ? " zero" : ""}" role="${open ? "button" : "group"}"
        aria-label="${f.name}" aria-description="${rowDescription(f)}" data-key="${f.key}">
      <div class="r-name">${f.name}</div>
      <div class="r-sub"></div>
      <div class="r-spark">${chart ? "<span data-spark></span>" : ""}</div>
      <div class="r-vals"><div class="r-rpm"><b>${f.rpm}</b><span class="unit">${NB}rpm</span></div>
        <div class="r-pct">${pctText(f)}</div></div>
      <div class="r-chev">${open ? `<i data-ic="next" class="chev"></i>` : ""}</div>
    </div>`;
  }

  // ---------- page ----------

  // The line under the temperature keeps its room, so that the list does
  // not move when it shows; the text about the kernel is a lasting state
  // and may take more.
  const speedingUp = fans.some(f => f.shown && f.target > f.duty);
  const under = !S.curves
    ? `<p class="h-kernel">Ten system na razie tylko odczytuje wentylatory. Sterowanie pojawi się w jednej z przyszłych wersji Linuksa. <span class="link">Stan w jądrze<i data-ic="external"></i></span></p>`
    : `<div class="h-sent${speedingUp ? "" : " reserved"}"${speedingUp ? "" : ' aria-hidden="true"'}>Wentylatory przyspieszają</div>`;
  document.getElementById("content").innerHTML = `
    <div class="w-page">
      <div class="h-hero">
        <div class="h-label">Procesor</div>
        <div class="h-temp">${S.temp}<span class="h-unit">${NB}°C</span></div>
        ${under}
      </div>
      <div class="w-clamp">
        ${fans.some(f => f.stopped) ? `<div class="w-alert" role="alert"><i data-ic="warning"></i>
          <div><div class="t">Wentylator procesora stoi</div>
          <div class="s">Kręcił się wcześniej, a od co najmniej 10${NB}s ma 0${NB}rpm.</div></div></div>` : ""}
        <div class="boxed-list w-list">${all.map(row).join("")}</div>
        ${S.curves ? `<p class="quiet">Baseboard na razie tylko pokazuje ustawienia wentylatorów${NB}z${NB}BIOS-u.</p>` : ""}
      </div>
    </div>`;

  window.afterBuild.push(() => {
    const win = document.querySelector(".window");
    if (winWidth) win.style.width = `${winWidth}px`;
    if (fixedHeight) {
      win.style.height = `${fixedHeight}px`;
      win.querySelector(".content-pane").style.overflow = "hidden";
    }
    const narrow = win.querySelector(".w-list").clientWidth < 500;
    for (const el of document.querySelectorAll(".r")) {
      const f = all.find(x => x.key === el.dataset.key);
      // The subtitle: in a narrow window speed, percentage and state; in a
      // wide one the state alone (the numbers have their column). Never
      // broken in a sentence: what does not fit on its line goes, the
      // state stays; a sentence too long for the name column gets the
      // width under the chart too.
      const sub = el.querySelector(".r-sub");
      const fits = () => sub.scrollWidth <= sub.clientWidth;
      if (narrow) el.classList.add("span");
      const options = narrow
        ? [[rpmText(f), pctText(f), f.state].filter(Boolean).join(" · "), f.state]
        : [f.state];
      for (const text of options) {
        sub.textContent = text;
        if (fits()) break;
      }
      if (!fits()) el.classList.add("span");
      if (f.stopped) sub.classList.add("warn");
      if (!sub.textContent) sub.remove();
      const box = el.querySelector("[data-spark]");
      if (box) box.outerHTML = narrow ? spark(f, 84, 28) : spark(f, 192, 36);
    }
  });
})();
