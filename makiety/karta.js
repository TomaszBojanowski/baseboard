// SPDX-License-Identifier: GPL-3.0-or-later

// The card of a part of the computer (D67), as the app draws it
// (`app::part_card`, `app::history_chart`): the name with its icon, the
// temperature in large type, the last 5 minutes as a line with their range,
// a few facts in plain words and a note under them that keeps its room.
// Close to its limit the icon, the number and the line take the color of
// warnings. The same card is on the Overview (przeglad.js) and on the Fans
// page (wentylatory-lista.js).

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

  // The line as wide as its box: an area under it, the newest reading as a
  // dot at the right end; at least 12 °C of scale, so that a quiet sensor
  // stays a quiet line.
  function spark(values, cls = "") {
    const { lo, hi } = range(values);
    const mid = (lo + hi) / 2;
    const span = Math.max(12, hi - lo + 2);
    const bottom = mid - span / 2, top = mid + span / 2;
    const W = 300, H = 100;
    const y = v => H - (v - bottom) / (top - bottom) * H;
    const line = `M${values.map((v, i) => `${(i * W / (values.length - 1)).toFixed(1)},${y(v).toFixed(1)}`).join(" L")}`;
    const id = `g${Math.random().toString(36).slice(2, 8)}`;
    const now = values[values.length - 1];
    return `<div class="spark ${cls}" role="img" aria-label="Ostatnie 5 minut: ${rangeText(values)}. Teraz ${whole(now)}${NB}°C.">
      <svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" aria-hidden="true">
        <defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style="stop-color:currentColor;stop-opacity:.28"/>
          <stop offset="1" style="stop-color:currentColor;stop-opacity:0"/>
        </linearGradient></defs>
        <path d="${line} L${W},${H} L0,${H} Z" style="fill:url(#${id})"/>
        <path d="${line}" class="sl" vector-effect="non-scaling-stroke"/>
      </svg>
      <span class="dot" style="top:${(y(now) / H * 100).toFixed(1)}%"></span>
    </div>`;
  }

  // `part`: { name, icon, value, values, highest, facts: [[title, value,
  // warn]], hot, opens, note: { text, shown } }. The highest temperature
  // since the app started is the last fact, never under the line.
  function card(part) {
    const highest = Math.max(part.highest, ...part.values);
    const facts = [...part.facts, ["Najwyższa od uruchomienia", `${whole(highest)}${NB}°C`]];
    const note = part.note
      ? `<div class="pc-note${part.note.shown ? "" : " reserved"}"${part.note.shown ? "" : ' aria-hidden="true"'}>${part.note.text}</div>`
      : "";
    return `<div class="pcard${part.hot ? " hot" : ""}"${part.opens ? ' role="button"' : ' role="group"'}>
        <div class="pc-head"><span class="badge${part.hot ? " warn" : ""}"><i data-ic="${part.hot ? "warning" : part.icon}"></i></span>
          <span class="pc-name">${part.name}</span>${part.opens ? `<i data-ic="chevron" class="chev"></i>` : ""}</div>
        <div class="pc-main">
          <div class="pc-num"><div class="n">${whole(part.value)}<span class="u">°C</span></div></div>
          <div class="facts">${facts.map(([t, v, warn]) =>
            `<div class="fact${warn ? " warn" : ""}"><span>${t}</span><b>${warn ? `<i data-ic="warning"></i>` : ""}${v}</b></div>`).join("")}</div>
          <div class="pc-hist">
            ${spark(part.values, "big")}
            <div class="cap"><span>Ostatnie 5 minut</span><span>${rangeText(part.values)}</span></div>
          </div>
        </div>
        ${note}
      </div>`;
  }

  window.Karta = { NB, whole, random, history, processor, rangeText, card };
})();
