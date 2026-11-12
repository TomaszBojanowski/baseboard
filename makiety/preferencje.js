// SPDX-License-Identifier: GPL-3.0-or-later

// Preferences of D64: one page instead of two tabs ("Readings" with the
// refresh interval and "Visible readings", "Names"); the switch for every
// reading one click further, on a subpage without drivers and sensors.
// ?dialog=preferencje (default) or odczyty (the subpage), ?theme=,
// ?width=narrow (a bottom sheet), ?w=, ?h=, ?bare=1.

(function () {
  const NB = " ";
  const params = new URLSearchParams(location.search);
  const which = params.get("dialog") || "preferencje";
  const narrow = params.get("width") === "narrow";
  window.afterBuild = window.afterBuild || [];

  const row = (t, end, o = {}) => `<div class="n-row">
      <div class="grow"><div class="t">${t}</div>${o.s ? `<div class="s">${o.s}</div>` : ""}</div>
      ${end || ""}
    </div>`;
  const group = (title, rows) => `<section class="n-group">
      ${title ? `<div class="heading">${title}</div>` : ""}
      <div class="boxed-list">${rows.join("")}</div></section>`;
  const sw = on => `<span class="switch${on ? " on" : ""}"></span>`;
  const spin = v => `<span class="spin"><span class="val">${v}</span>
      <span class="b">${icon("minus")}</span><span class="b">${icon("plus")}</span></span>`;
  const chev = `<i data-ic="chevron" class="chev"></i>`;
  const edit = `<span class="btn flat image">${icon("pencil")}</span>`;
  const value = v => `<span class="v">${v}</span>`;
  const C = v => `${v}${NB}°C`;
  const V = v => `${v}${NB}V`;

  function main() {
    return {
      title: "Preferencje",
      back: false,
      body: `
        ${group("Odczyty", [
          row("Odświeżanie", spin(`1${NB}s`)),
          row("Widoczne odczyty", chev, { s: "Wybierz, co pokazuje Przegląd" }),
        ])}
        ${group("Nazwy", [
          row("Nazwy z lm-sensors", sw(false), { s: "Takie jak w poleceniu sensors" }),
          row("Własne nazwy", edit, { s: "Zmiany działają po ponownym uruchomieniu" }),
        ])}`,
    };
  }

  function readings() {
    const r = (t, v) => row(t, value(v) + sw(true));
    return {
      title: "Widoczne odczyty",
      back: true,
      body: `
        <p class="page-desc">Wyłączone odczyty nie są pokazywane na stronie Przegląd, np. złącza bez wentylatora. Numerowane czujniki temperatury dysków są domyślnie wyłączone.</p>
        ${group("Procesor", [r("Procesor (Tctl)", C("68,2")), r("Chiplet CCD1", C("69,1")), r("Procesor (czujnik płyty)", C("50,0"))])}
        ${group("Płyta", [r("Temperatura systemowa", C("37,5")), r("Sekcja zasilania procesora", C("36,0")), r("Chipset", C("37,0"))])}
        ${group("Napięcia", [r("+12 V", V("12,10")), r("+5 V", V("4,96")), r("+3,3 V", V("3,31"))])}`,
    };
  }

  const d = which === "odczyty" ? readings() : main();

  window.afterBuild.push(() => {
    const win = document.querySelector(".window");
    const shade = document.createElement("div");
    shade.className = narrow ? "n-shade sheet" : "n-shade";
    shade.innerHTML = `<div class="n-dialog${narrow ? " narrow" : ""}" role="dialog" aria-label="${d.title}">
        <div class="hb">
          <div>${d.back ? `<span class="btn flat image">${icon("back")}</span>` : ""}</div>
          <div class="tt">${d.title}</div>
          <div class="end"><span class="btn flat image circular">${icon("close")}</span></div>
        </div>
        <div class="body">${d.body}</div>
      </div>`;
    win.appendChild(shade);
    shade.querySelectorAll("i[data-ic]").forEach(el => {
      el.outerHTML = icon(el.dataset.ic, el.className);
    });
  });
})();
