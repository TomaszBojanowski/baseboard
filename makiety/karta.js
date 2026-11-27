// SPDX-License-Identifier: GPL-3.0-or-later

// The card of a part of the computer (D67, D80), as the app draws it
// (`app::part_card`, `app::history_chart`): the name with its icon, the
// temperature in large type, the last 5 minutes as a line, a few facts in
// plain words with the range of the line, and a note under them that keeps
// its room. Close to its limit the icon, the number and the line take the
// color of warnings. The same card is on the Overview (przeglad.js) and on
// the Fans page (wentylatory-lista.js), which call Karta.draw once the
// window has its size.

(function () {
  const NB = " ";
  const whole = v => String(Math.round(v));

  // A seeded random generator, so that every render is the same.
  function random(seed) {
    return () => {
      seed = (seed + 0x6D2B79F5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // 300 readings, one a second (5 minutes), ending at `now`; `shape` gives
  // the value of reading i with some noise.
  function history(seed, now, shape) {
    const r = random(seed);
    const out = [];
    let noise = 0;
    for (let i = 0; i < 300; i++) {
      noise = noise * 0.7 + (r() - 0.5) * 0.9;
      out.push(shape(i, noise, r));
    }
    const shift = now - out[299];
    return out.map((v, i) => v + shift * Math.min(1, Math.max(0, (i - 240) / 59)));
  }

  // Tctl of a Ryzen at rest moves by a few degrees with every short task;
  // `load` from reading 190 on (140 when it gets hot) climbs to `now`.
  function processor(now, { load = false, hot = false, stopped = false } = {}) {
    const rest = (i, n) => 66.5 + 1.8 * Math.sin(i / 23) + 0.9 * Math.sin(i / 7.1 + 1) + 1.1 * n +
      (i % 71 < 9 ? 2.2 * Math.sin((i % 71) / 9 * Math.PI) : 0);
    const start = hot ? 140 : 190;
    return history(7, now, (i, n) => {
      if (load && i >= start) return rest(start, 0) + (now - rest(start, 0)) * (1 - Math.exp(-(i - start) / 16)) + 0.5 * n;
      if (stopped && i >= 262) return rest(i, n) + (i - 262) * 0.045;
      return rest(i, n);
    });
  }

  const range = values => ({ lo: Math.min(...values), hi: Math.max(...values) });
  function rangeText(values) {
    const { lo, hi } = range(values);
    return Math.round(lo) === Math.round(hi) ? `${whole(hi)}${NB}°C` : `${whole(lo)}–${whole(hi)}${NB}°C`;
  }

  // The limit in words, as the status sentence says it.
  const limitWords = (processor, limit) => processor
    ? `Przy ${limit}${NB}°C zaczyna zwalniać, żeby się nie przegrzać.`
    : `Limit to ${limit}${NB}°C.`;
  // The range as a fact: "Od 72 do 88 °C", or one value.
  function fromTo(values) {
    const { lo, hi } = range(values);
    return Math.round(lo) === Math.round(hi)
      ? `${whole(hi)}${NB}°C` : `Od${NB}${whole(lo)}${NB}do ${whole(hi)}${NB}°C`;
  }

  // The line of the last 5 minutes (D80), drawn by `draw` at its size.
  function chart(values, limit, processor) {
    const now = values[values.length - 1];
    const words = `Ostatnie 5 minut: ${rangeText(values)}. Teraz ${whole(now)}${NB}°C.` +
      (limit ? ` ${limitWords(processor, limit)}` : "");
    return `<div class="hchart" role="img" aria-label="${words}"
        data-values="${values.map(v => v.toFixed(2)).join(" ")}"${limit ? ` data-limit="${limit}"` : ""}>
      <div class="hplot"></div>
      <div class="htime"><span>5 min temu</span><span>teraz</span></div>
    </div>`;
  }

  // With cards side by side the plots take the free height of the page,
  // from 120 to 320 px and no taller than wide without the labels (D81).
  function fit(charts) {
    const pane = document.querySelector(".content-pane");
    const page = document.getElementById("content");
    if (!pane || !page || !charts.length || pane.clientWidth < 880) return;
    const plots = charts.map(el => el.querySelector(".hplot"));
    plots.forEach(plot => { plot.style.height = "120px"; });
    const head = pane.querySelector(".headerbar")?.offsetHeight || 0;
    const free = pane.clientHeight - head - page.scrollHeight;
    const width = Math.max(...plots.map(plot => plot.clientWidth)) - 46;
    const height = Math.round(Math.max(120, Math.min(120 + free, 320, width)));
    plots.forEach(plot => { plot.style.height = `${height}px`; });
  }

  // Draws every line of the page at its size, as `app::history_chart`:
  // one scale for all cards, 0–100 °C, lower or higher in whole 10 °C when
  // a reading needs it; lines at 20, 40, 60 and 80 °C labeled on the left,
  // a line closer than 10 °C to the limit giving way to it; the limit
  // dashed in the color of warnings; a fill from the highest point of the
  // line to the bottom; time from the left (five minutes ago) to the dot
  // over "now". Pages call it once the window has its size.
  function draw() {
    const charts = [...document.querySelectorAll(".hchart")];
    fit(charts);
    const series = charts.map(el => el.dataset.values.split(" ").map(Number));
    const all = series.flat();
    const bottom = Math.floor(Math.min(0, ...all) / 10) * 10;
    const top = Math.ceil(Math.max(100, ...all) / 10) * 10;
    const DOT = 5.5, RING = 2.5, PAD = DOT + RING, MARGIN = 46;
    charts.forEach((el, n) => {
      const values = series[n];
      const plot = el.querySelector(".hplot");
      const W = plot.clientWidth, H = plot.clientHeight;
      const limit = el.dataset.limit ? Number(el.dataset.limit) : null;
      const shown = limit != null && limit >= bottom && limit <= top ? limit : null;
      const X = i => MARGIN + (W - MARGIN - PAD) * (1 - (values.length - 1 - i) / 300);
      const Y = v => PAD + (top - v) / (top - bottom) * (H - 2 * PAD);
      const f = v => v.toFixed(1);
      const label = (v, cls) =>
        `<text class="hl${cls}" x="${MARGIN - 8}" y="${f(Y(v) + 4)}" text-anchor="end">${v}${NB}°C</text>`;
      let svg = "";
      for (const g of [20, 40, 60, 80]) {
        if (g <= bottom || g >= top || (shown != null && Math.abs(shown - g) < 10)) continue;
        svg += `<line class="hg" x1="${MARGIN}" x2="${W}" y1="${f(Y(g))}" y2="${f(Y(g))}"/>${label(g, "")}`;
      }
      if (shown != null) {
        svg += `<line class="hlimit" x1="${MARGIN}" x2="${W}" y1="${f(Y(shown))}" y2="${f(Y(shown))}"/>${label(shown, " warn")}`;
      }
      const line = values.map((v, i) => `${f(X(i))},${f(Y(v))}`).join(" L");
      const id = `hg${n}`;
      svg += `<defs><linearGradient id="${id}" gradientUnits="userSpaceOnUse" x1="0" y1="${f(Y(Math.max(...values)))}" x2="0" y2="${H}">
          <stop offset="0" style="stop-color:var(--hist-fill);stop-opacity:var(--hist-strong)"/>
          <stop offset="1" style="stop-color:var(--hist-fill);stop-opacity:var(--hist-weak)"/></linearGradient></defs>
        <path d="M${line} L${f(X(values.length - 1))},${H} L${f(X(0))},${H} Z" style="fill:url(#${id})"/>
        <path class="hline" d="M${line}"/>
        <circle class="hring" cx="${f(X(values.length - 1))}" cy="${f(Y(values[values.length - 1]))}" r="${DOT + RING}"/>
        <circle class="hdot" cx="${f(X(values.length - 1))}" cy="${f(Y(values[values.length - 1]))}" r="${DOT}"/>`;
      plot.innerHTML = `<svg width="${W}" height="${H}" aria-hidden="true">${svg}</svg>`;
      el.querySelector(".htime").style.paddingLeft = `${MARGIN}px`;
    });
  }

  // `part`: { name, icon, value, values, highest, limit, facts: [[title,
  // value, warn]], hot, opens, note: { text, shown } }. The range of the
  // line stands beside the temperature and, in a page of one card a row or
  // narrower, among the facts (karta.css); the highest temperature since
  // the app started is the last fact.
  function card(part) {
    const highest = Math.max(part.highest, ...part.values);
    const span = ["Ostatnie 5 minut", fromTo(part.values)];
    const facts = [...part.facts, span,
      ["Najwyższa od uruchomienia", `${whole(highest)}${NB}°C`]];
    const note = part.note
      ? `<div class="pc-note${part.note.shown ? "" : " reserved"}"${part.note.shown ? "" : ' aria-hidden="true"'}>${part.note.text}</div>`
      : "";
    return `<div class="pcard${part.hot ? " hot" : ""}"${part.opens ? ' role="button"' : ' role="group"'}>
        <div class="pc-head"><span class="badge${part.hot ? " warn" : ""}"><i data-ic="${part.hot ? "warning" : part.icon}"></i></span>
          <span class="pc-name">${part.name}</span>${part.opens ? `<i data-ic="chevron" class="chev"></i>` : ""}</div>
        <div class="pc-main">
          <div class="pc-num"><div class="n">${whole(part.value)}<span class="u">°C</span></div>
            <div class="pc-range" aria-hidden="true"><span>${span[0]}</span><b>${span[1]}</b></div></div>
          <div class="facts">${facts.map(fact => {
            const [t, v, warn] = fact;
            return `<div class="fact${warn ? " warn" : ""}${fact === span ? " range" : ""}"><span>${t}</span><b>${warn ? `<i data-ic="warning"></i>` : ""}${v}</b></div>`;
          }).join("")}</div>
          <div class="pc-hist">${chart(part.values, part.limit, part.icon === "cpu")}</div>
        </div>
        ${note}
      </div>`;
  }

  window.Karta = { NB, whole, random, history, processor, rangeText, card, draw };
})();
