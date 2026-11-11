// SPDX-License-Identifier: GPL-3.0-or-later

// Fan curve chart for the M2 mockups, drawn as SVG at the pixel size of its
// box, so that text keeps its real size.
//
// The controller interpolates linearly between the points (seen on MS-7E61:
// SYS_FAN1 at 46 °C ran 98/255, a straight line between its points gives
// 97.6), so the curve is straight segments with round joins. A spline would
// show duty cycles the fan never gets. Outside the end points the controller
// holds the end values; those parts are dashed.

(function () {
  const NB = " ";
  // 0–255 to the percent shown in the interface (§3.5): round half up.
  const pct = v => Math.floor(v * 100 / 255 + 0.5);
  const f1 = v => v.toLocaleString("pl-PL", { minimumFractionDigits: 1, maximumFractionDigits: 1 });

  const ctx = document.createElement("canvas").getContext("2d");
  function textWidth(text, size, weight = 400) {
    ctx.font = `${weight} ${size}px "Adwaita Sans", Cantarell, sans-serif`;
    return ctx.measureText(text).width;
  }

  // Duty cycle (0–255, not rounded) of a curve at a temperature.
  function dutyAt(points, t) {
    if (t <= points[0][0]) return points[0][1];
    for (let i = 1; i < points.length; i++) {
      const [t0, d0] = points[i - 1];
      const [t1, d1] = points[i];
      if (t <= t1) return t1 === t0 ? d1 : d0 + (d1 - d0) * (t - t0) / (t1 - t0);
    }
    return points[points.length - 1][1];
  }

  // Runs of equal consecutive points, as [first, last] indices.
  function groups(points) {
    const out = [];
    for (let i = 0; i < points.length; i++) {
      const last = out[out.length - 1];
      if (last && points[i][0] === points[last[1]][0] && points[i][1] === points[last[1]][1]) last[1] = i;
      else out.push([i, i]);
    }
    return out;
  }

  function inlineIcon(name, x, y, size, color) {
    return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 16 16"
      style="fill:none;stroke:${color};stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round;color:${color}">${window.iconPaths[name]}</svg>`;
  }

  // A text label with a background, so that it stays readable over the
  // hatched zone and the grid.
  function label(x, y, text, { anchor = "start", size = 12, cls = "", color = "", weight = 400 } = {}) {
    const w = textWidth(text, size, weight);
    const left = anchor === "end" ? x - w : anchor === "middle" ? x - w / 2 : x;
    const style = color ? ` style="fill:${color}"` : "";
    return `<rect x="${left - 4}" y="${y - size}" width="${w + 8}" height="${size + 6}" rx="4" class="lbg"/>` +
      `<text class="${cls}" x="${x}" y="${y}" text-anchor="${anchor}"${style}>${text}</text>`;
  }

  // o: w, h, uid, label, mini, spark, points, ghost, manual (0–255), min (%),
  // threshold (°C), needsFull, source, now {t, d, target, aheadLabel},
  // handles, selected, focus, issues {index: text}. Sets o.nowShown when the
  // label of the operating point fits on the chart.
  function chart(o) {
    const { w, h, mini, spark } = o;
    const L = spark ? 2 : mini ? 42 : 66;
    const R = spark ? 4 : mini ? 12 : 22;
    const T = spark ? 3 : mini ? 12 : 38;
    const B = spark ? 3 : mini ? 26 : 58;
    const X = t => L + t * (w - L - R) / 100;
    const Yp = p => (h - B) - p * (h - B - T) / 100;
    const Y = d => Yp(d * 100 / 255);
    const id = o.uid;
    const size = mini ? 11 : 12;
    const cls = ["wk", mini ? "mini" : "", spark ? "spark" : ""].join(" ");
    // Boxes that labels drawn later must not cover: the threshold text,
    // handles, warnings and the operating point.
    const obstacles = [];
    const hits = (x, y, bw, bh) => obstacles.some(r =>
      x < r.x + r.w + 4 && x + bw + 4 > r.x && y < r.y + r.h + 4 && y + bh + 4 > r.y);

    let s = `<svg class="${cls}" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${o.label || ""}">`;
    s += `<defs>
      <linearGradient id="g-${id}" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" style="stop-color:var(--accent-bg);stop-opacity:.30"/>
        <stop offset="1" style="stop-color:var(--accent-bg);stop-opacity:.04"/>
      </linearGradient>
      <pattern id="h-${id}" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
        <line x1="0" y1="0" x2="0" y2="6" style="stroke:currentColor;stroke-opacity:.2;stroke-width:2"/>
      </pattern>
    </defs>`;

    // grid and axes
    if (!spark) {
      for (let t = 0; t <= 100; t += mini ? 20 : 10) {
        s += `<line class="grid" x1="${X(t)}" y1="${T}" x2="${X(t)}" y2="${h - B}"/>`;
      }
      for (const p of mini ? [0, 50, 100] : [0, 25, 50, 75, 100]) {
        s += `<line class="${p === 0 ? "axis" : "grid"}" x1="${L}" y1="${Yp(p)}" x2="${w - R}" y2="${Yp(p)}"/>`;
        s += `<text class="tick" x="${L - 7}" y="${Yp(p) + 4}" text-anchor="end">${p}${NB}%</text>`;
      }
      s += `<line class="axis" x1="${L}" y1="${T}" x2="${L}" y2="${h - B}"/>`;
      const step = mini && w < 300 ? 50 : 20;
      for (let t = 0; t <= 100; t += step) {
        s += `<text class="tick" x="${X(t)}" y="${Yp(0) + (mini ? 16 : 20)}" text-anchor="middle">${t}${NB}°C</text>`;
      }
      if (!mini) {
        s += `<text class="atitle" x="${L + (w - L - R) / 2}" y="${h - 10}" text-anchor="middle">Temperatura: ${o.source}</text>`;
        s += `<text class="atitle" transform="translate(16 ${T + (h - B - T) / 2}) rotate(-90)" text-anchor="middle">Wypełnienie</text>`;
      }
    }

    const curve = o.manual != null ? [[0, o.manual], [100, o.manual]] : o.points;

    // emergency band and the zone below the minimum
    if (o.threshold != null && !spark) {
      s += `<rect x="${X(o.threshold)}" y="${T}" width="${X(100) - X(o.threshold)}" height="${h - B - T}" style="fill:var(--error-bg);fill-opacity:.1"/>`;
    }
    if (o.min > 0 && !spark) {
      s += `<rect x="${L}" y="${Yp(o.min)}" width="${w - R - L}" height="${Yp(0) - Yp(o.min)}" style="fill:url(#h-${id})"/>`;
      s += `<line x1="${L}" y1="${Yp(o.min)}" x2="${w - R}" y2="${Yp(o.min)}" style="stroke:currentColor;stroke-opacity:.4;stroke-dasharray:3 3"/>`;
    }

    // area below the curve (or below the manual line)
    if (curve) {
      const first = curve[0];
      const last = curve[curve.length - 1];
      const solid = curve.map(([t, d]) => `${X(t)},${Y(d)}`).join(" ");
      s += `<polygon points="${X(0)},${Yp(0)} ${X(0)},${Y(first[1])} ${solid} ${X(100)},${Y(last[1])} ${X(100)},${Yp(0)}" style="fill:url(#g-${id})"/>`;
    }

    // Texts of the zones, only on the large chart; a card says the minimum in
    // its subtitle and the threshold in the source row above the cards. The
    // minimum sits at the bottom left, below the curve line, so that it never
    // hides the curve; the threshold sits above the plot, at the top of its
    // line, ending at the line.
    if (o.min > 0 && !spark && !mini) {
      const text = `Poniżej minimum ${o.min}${NB}%`;
      s += label(L + 8, Yp(0) - 9, text, { size, cls: "zone" });
      obstacles.push({ x: L + 4, y: Yp(0) - 9 - size, w: textWidth(text, size) + 8, h: size + 6 });
    }
    if (o.threshold != null && !spark) {
      const x = X(o.threshold);
      s += `<line x1="${x}" y1="${mini ? T : T - 10}" x2="${x}" y2="${h - B}" style="stroke:var(--error);stroke-width:1.5;stroke-dasharray:5 3"/>`;
      if (!mini) {
        const text = o.needsFull
          ? `Próg awaryjny ${o.threshold}${NB}°C: od niego 100${NB}%`
          : `Próg awaryjny ${o.threshold}${NB}°C`;
        s += `<text x="${x + 4}" y="${T - 16}" text-anchor="end" style="fill:var(--error);font-weight:700">${text}</text>`;
      }
    }

    // the curve the controller runs now, while the user edits another one
    if (o.ghost) {
      const g = o.ghost;
      const pts = [[0, g[0][1]], ...g, [100, g[g.length - 1][1]]].map(([t, d]) => `${X(t)},${Y(d)}`).join(" ");
      s += `<polyline points="${pts}" class="ghost"/>`;
    }

    // curve or manual line
    if (curve) {
      const first = curve[0];
      const last = curve[curve.length - 1];
      const solid = curve.map(([t, d]) => `${X(t)},${Y(d)}`).join(" ");
      if (first[0] > 0) s += `<line x1="${X(0)}" y1="${Y(first[1])}" x2="${X(first[0])}" y2="${Y(first[1])}" class="curve dashed"/>`;
      if (last[0] < 100) s += `<line x1="${X(last[0])}" y1="${Y(last[1])}" x2="${X(100)}" y2="${Y(last[1])}" class="curve dashed"/>`;
      s += `<polyline points="${solid}" class="curve"/>`;
    }
    if (o.threshold != null && o.needsFull && !mini && !spark) {
      s += `<circle cx="${X(o.threshold)}" cy="${Yp(100)}" r="5" style="fill:var(--surface);stroke:var(--error);stroke-width:2"/>`;
    }

    // operating point: guides, axis markers and the dot (drawn after handles)
    let dot = "";
    if (o.now) {
      const cx = X(o.now.t);
      const cy = Y(o.now.d);
      if (!spark) {
        s += `<line x1="${cx}" y1="${Yp(0)}" x2="${cx}" y2="${cy}" class="guide"/>`;
        if (!mini) {
          s += `<line x1="${L}" y1="${cy}" x2="${cx}" y2="${cy}" class="guide"/>`;
          s += `<path d="M${L - 1},${cy - 5} L${L + 6},${cy} L${L - 1},${cy + 5} Z" style="fill:var(--live)"/>`;
        }
        s += `<path d="M${cx - 5},${Yp(0) + 1} L${cx},${Yp(0) - 6} L${cx + 5},${Yp(0) + 1} Z" style="fill:var(--live)"/>`;
      }
      // where the duty cycle is heading: the curve target while the output
      // ramps, or the value of the edited curve once it is applied
      const ahead = o.now.target != null ? o.now.target : null;
      if (ahead != null && Math.abs(Y(ahead) - cy) > 3) {
        const ty = Y(ahead);
        s += `<line x1="${cx}" y1="${cy}" x2="${cx}" y2="${ty}" style="stroke:var(--accent-bg);stroke-width:2;stroke-dasharray:3 3"/>`;
        s += `<circle cx="${cx}" cy="${ty}" r="${mini ? 4 : 6}" style="fill:var(--surface);stroke:var(--accent-bg);stroke-width:2.5"/>`;
      }
      const r = spark ? 2.5 : mini ? 4.5 : 6.5;
      dot = `<circle cx="${cx}" cy="${cy}" r="${r * 2}" style="fill:var(--live);fill-opacity:.2"/>` +
        `<circle cx="${cx}" cy="${cy}" r="${r}" style="fill:var(--live);stroke:var(--surface);stroke-width:${spark ? 1.5 : 2.5}"/>`;
    }

    // Places the label of the operating point where it covers no handle,
    // warning and the point itself; called once those are drawn.
    function nowLabel() {
      if (!o.now || mini || spark) return "";
      const cx = X(o.now.t);
      const cy = Y(o.now.d);
      const ahead = o.now.target;
      const lines = [`Teraz ${f1(o.now.t)}${NB}°C · ${pct(o.now.d)}${NB}%`];
      if (ahead != null) lines.push(`${o.now.aheadLabel || "cel"} ${pct(ahead)}${NB}%`);
      const cw = Math.max(...lines.map((l, i) => textWidth(l, 12, i === 0 ? 700 : 400))) + 30;
      const ch = 12 + lines.length * 17;
      const inside = (x, y) => x >= L + 4 && x + cw <= w - R && y >= T && y + ch <= h - B - 4;
      const candidates = [];
      for (const dy of [16, 40, 70, -16 - ch, -40 - ch, -70 - ch]) {
        for (const dx of [18, -18 - cw, 40, -40 - cw]) candidates.push([cx + dx, cy + dy]);
      }
      // No free spot (a narrow chart): no label, and the page puts the same
      // values into the legend under the chart (o.nowShown says which).
      const spot = candidates.find(([x, y]) => inside(x, y) && !hits(x, y, cw, ch));
      if (!spot) return "";
      o.nowShown = true;
      const [bx, by] = spot;
      let s2 =`<rect x="${bx}" y="${by}" width="${cw}" height="${ch}" rx="8" class="chip"/>`;
      s2 += `<circle cx="${bx + 13}" cy="${by + 17}" r="4" style="fill:var(--live)"/>`;
      lines.forEach((l, i) => {
        s2 += `<text x="${bx + 23}" y="${by + 21 + i * 17}"${i ? ' class="tick"' : ' style="font-weight:700"'}>${l}</text>`;
      });
      return s2;
    }
    if (o.now && !mini && !spark) {
      obstacles.push({ x: X(o.now.t) - 14, y: Y(o.now.d) - 14, w: 28, h: 28 });
      if (o.now.target != null) obstacles.push({ x: X(o.now.t) - 10, y: Y(o.now.target) - 10, w: 20, h: 20 });
    }

    // points: numbered handles in the editor, dots elsewhere
    const halfWidth = (i, j) => i === j ? 11 : textWidth(`${i + 1}–${j + 1}`, 11, 700) / 2 + 9;
    if (o.points && o.manual == null && o.handles && !spark) {
      for (const [i, j] of groups(o.points)) {
        const hw = halfWidth(i, j);
        obstacles.push({ x: X(o.points[i][0]) - hw - 4, y: Y(o.points[i][1]) - 15, w: 2 * hw + 8, h: 30 });
      }
    }
    if (o.points && o.manual == null) {
      for (const [i, j] of groups(o.points)) {
        const x = X(o.points[i][0]);
        const y = Y(o.points[i][1]);
        if (spark) continue;
        if (!o.handles) {
          s += `<circle cx="${x}" cy="${y}" r="${mini ? 3 : 4.5}" style="fill:var(--accent-bg);stroke:var(--surface);stroke-width:${mini ? 1.5 : 2}"/>`;
          continue;
        }
        const text = i === j ? `${i + 1}` : `${i + 1}–${j + 1}`;
        const sel = o.selected >= i && o.selected <= j;
        let issue = false;
        for (let k = i; k <= j; k++) if (o.issues && o.issues[k]) issue = true;
        const ring = issue ? "var(--warning-bg)" : "var(--accent-bg)";
        const hw = halfWidth(i, j);
        const shape = (pad, style) => i === j
          ? `<circle cx="${x}" cy="${y}" r="${11 + pad}" style="${style}"/>`
          : `<rect x="${x - hw - pad}" y="${y - 11 - pad}" width="${2 * (hw + pad)}" height="${22 + 2 * pad}" rx="${11 + pad}" style="${style}"/>`;
        // A selected handle is filled with the text variant of its color
        // (dark in the light style, light in the dark one) and carries the
        // number in the window background color: 4.5:1 or more in both.
        if (sel && o.focus) s += shape(4.5, "fill:none;stroke:var(--accent-bg);stroke-opacity:.55;stroke-width:2.5");
        s += shape(0, sel ? `fill:${issue ? "var(--warning)" : "var(--accent)"};stroke:var(--surface);stroke-width:2`
          : `fill:var(--surface);stroke:${ring};stroke-width:2.5`);
        s += `<text x="${x}" y="${y + 4}" text-anchor="middle" class="hnum" style="fill:${sel ? "var(--window-bg)" : issue ? "var(--warning)" : "var(--accent)"}">${text}</text>`;
        // A point that breaks a rule: warning color and a warning sign at the
        // handle, so not by color alone; the reason stands in its row of the
        // list of points, not on the chart.
        if (issue) {
          const bx = x + hw - 2;
          const by = y - 24;
          s += `<circle cx="${bx + 7}" cy="${by + 7}" r="9.5" style="fill:var(--surface)"/>`;
          s += inlineIcon("warning", bx, by, 14, "var(--warning)");
        }
      }
    }
    s += dot;
    s += nowLabel();
    s += `</svg>`;
    return s;
  }

  window.fanChart = { NB, pct, f1, dutyAt, chart };
})();
