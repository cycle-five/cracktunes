"use strict";
// Renders, sorts and filters. Every number shown comes from the embedded model.
// Text is only ever inserted as text nodes: unit ids contain < and >.
const M = JSON.parse(document.getElementById("data").textContent);
const STATES = ["clean", "signature", "body", "inherited"];
const ROW_CAP = 400;

function el(tag, attrs, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (k === "class") n.className = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v);
  }
  for (const k of kids.flat()) {
    if (k === null || k === undefined || k === false) continue;
    n.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return n;
}
const num = (n) => (n === null || n === undefined ? "" : String(n));
const locText = (l) => (l && l.file ? l.file + ":" + l.line : "");

function badge(state) {
  return el("span", { class: "badge s-" + state }, state);
}

function bar(lines) {
  const b = el("div", { class: "bar", title: STATES.map((s) => s + " " + lines[s]).join(", ") });
  for (const s of STATES) {
    const seg = el("span", { class: s });
    seg.style.flexGrow = String(lines[s]);
    b.append(seg);
  }
  return b;
}

function unitLink(index) {
  const u = M.units[index];
  return el("button", { class: "link", type: "button", onclick: () => openDetail(index) }, u.id);
}
const byId = new Map(M.units.map((u, i) => [u.id, i]));
function idLink(id) {
  return byId.has(id) ? unitLink(byId.get(id)) : el("span", { class: "mono" }, id);
}

// A sortable, filterable table. cols: {label, num, cell(row), sort(row)}; text(row) feeds the filter.
function table(cols, rows, text, initialSort) {
  const state = { col: initialSort ? initialSort[0] : -1, dir: initialSort ? initialSort[1] : 1, q: "", all: false };
  const host = el("div");
  const info = el("p", { class: "muted" });
  const body = el("tbody");
  const head = el("tr");
  cols.forEach((c, i) => {
    const th = el("th", { class: c.num ? "num" : "" }, c.label);
    th.addEventListener("click", () => {
      state.dir = state.col === i ? -state.dir : 1;
      state.col = i;
      draw();
    });
    head.append(th);
  });
  const filter = el("input", { type: "search", placeholder: "filter", "aria-label": "filter" });
  filter.addEventListener("input", () => {
    state.q = filter.value.toLowerCase();
    state.all = false;
    draw();
  });
  function draw() {
    let shown = rows.filter((r) => !state.q || text(r).toLowerCase().includes(state.q));
    if (state.col >= 0 && cols[state.col].sort) {
      const f = cols[state.col].sort;
      shown = shown.slice().sort((a, b) => {
        const x = f(a), y = f(b);
        return (x < y ? -1 : x > y ? 1 : 0) * state.dir;
      });
    }
    body.replaceChildren();
    const cut = state.all ? shown : shown.slice(0, ROW_CAP);
    for (const r of cut) body.append(el("tr", {}, cols.map((c) => el("td", { class: c.num ? "num" : "" }, c.cell(r)))));
    info.replaceChildren(
      "showing " + cut.length + " of " + shown.length + " (" + rows.length + " total) ",
      ...(shown.length > cut.length ? [el("button", { class: "btn", type: "button", onclick: () => { state.all = true; draw(); } }, "show all")] : [])
    );
  }
  host.append(el("div", { class: "controls" }, filter), info, el("div", { class: "scroll" }, el("table", {}, el("thead", {}, head), body)));
  draw();
  return host;
}

function groupSelect(onchange) {
  const s = el("select", { "aria-label": "group" }, M.groups.map((g) => el("option", { value: g }, g)));
  s.value = M.headline_group;
  s.addEventListener("change", () => onchange(s.value));
  return s;
}

// Tabs that pick a group: re-render the body when the group changes.
function withGroup(build) {
  const host = el("div");
  const inner = el("div");
  const draw = (g) => inner.replaceChildren(build(g));
  host.append(el("div", { class: "controls" }, el("label", {}, "group "), groupSelect(draw)), inner);
  draw(M.headline_group);
  return host;
}

const linesCols = (get) =>
  STATES.map((s) => ({ label: s, num: true, cell: (r) => num(get(r)[s]), sort: (r) => get(r)[s] }));

const TABS = {
  Overview() {
    const o = M.overview;
    const root = el("div");
    root.append(el("h2", {}, "Crates"));
    root.append(
      table(
        [
          { label: "crate", cell: (c) => c.id, sort: (c) => c.id },
          { label: "package", cell: (c) => c.package, sort: (c) => c.package },
          { label: "kind", cell: (c) => c.kind + (c.test_only ? " (test)" : ""), sort: (c) => c.kind },
          { label: "lines", num: true, cell: (c) => num(c.lines), sort: (c) => c.lines },
          { label: M.headline_group + " split", cell: (c) => (c.split ? bar(c.split) : "in group"), sort: (c) => (c.split ? c.split.clean : -1) },
          ...STATES.map((s) => ({ label: s, num: true, cell: (c) => (c.split ? num(c.split[s]) : ""), sort: (c) => (c.split ? c.split[s] : -1) })),
        ],
        o.crates,
        (c) => c.id + " " + c.package,
        [3, -1]
      )
    );
    root.append(el("h2", {}, "Rebuild timings"));
    if (!o.timings) {
      root.append(el("p", { class: "muted" }, "No timings were supplied."));
    } else {
      const t = o.timings;
      root.append(el("p", { class: "muted" }, t.rustc + ", profile " + t.profile + ", " + t.jobs + " jobs, " + t.repeats + " repeats, " + t.host));
      if (t.rebuild) {
        root.append(el("p", {}, "Touch " + t.rebuild.package + ", build through " + t.rebuild.through + ": median " + t.rebuild.median_secs.toFixed(1) + " s"));
      }
      root.append(
        table(
          [
            { label: "package", cell: (c) => c.package, sort: (c) => c.package },
            { label: "target", cell: (c) => c.target, sort: (c) => c.target },
            { label: "median s", num: true, cell: (c) => c.median_secs.toFixed(1), sort: (c) => c.median_secs },
          ],
          t.crates,
          (c) => c.package,
          [2, -1]
        )
      );
    }
    return root;
  },

  Modules() {
    return withGroup((g) => {
      const rows = M.modules.find((x) => x.group === g).rows;
      return table(
        [
          { label: "module", cell: (r) => idLink(r.module), sort: (r) => r.module },
          { label: "split", cell: (r) => bar(r.lines) },
          ...linesCols((r) => r.lines).map((c) => ({ ...c, label: c.label + " lines" })),
          ...STATES.map((s) => ({ label: s + " units", num: true, cell: (r) => num(r.units[s]), sort: (r) => r.units[s] })),
          { label: "groups reached", cell: (r) => Object.entries(r.groups_reached).map(([k, v]) => k + " " + v).join(", "), sort: (r) => Object.keys(r.groups_reached).length },
          { label: "imports", cell: (r) => r.imports.join(", "), sort: (r) => r.imports.length },
        ],
        rows,
        (r) => r.module + " " + r.imports.join(" "),
        [2, -1]
      );
    });
  },

  Seams() {
    return withGroup((g) => {
      const rows = M.seams.find((x) => x.group === g).seams;
      return el(
        "div",
        {},
        el("p", { class: "muted" }, "Tainted units at most " + M.near_clean_max_cuts + " cuts from clean, cheapest and biggest first."),
        table(
          [
            { label: "unit", cell: (r) => unitLink(r.index), sort: (r) => r.unit },
            { label: "state", cell: (r) => badge(r.state), sort: (r) => r.state },
            { label: "cuts", num: true, cell: (r) => num(r.cuts), sort: (r) => r.cuts },
            { label: "lines", num: true, cell: (r) => num(r.lines), sort: (r) => r.lines },
            { label: "where", cell: (r) => locText(r.loc), sort: (r) => locText(r.loc) },
          ],
          rows,
          (r) => r.unit + " " + r.state,
          [2, 1]
        )
      );
    });
  },

  Tests() {
    return withGroup((g) => {
      const rows = M.tests.find((x) => x.group === g).units;
      return el(
        "div",
        {},
        el("p", { class: "muted" }, "Tests whose state is not clean: the ones a split must move or rewrite."),
        table(
          [
            { label: "test", cell: (r) => unitLink(r.unit), sort: (r) => M.units[r.unit].id },
            { label: "state", cell: (r) => badge(r.state), sort: (r) => r.state },
            { label: "cuts", num: true, cell: (r) => num(r.cuts), sort: (r) => r.cuts },
            { label: "where", cell: (r) => locText(M.units[r.unit].loc), sort: (r) => locText(M.units[r.unit].loc) },
          ],
          rows,
          (r) => M.units[r.unit].id,
          [2, 1]
        )
      );
    });
  },

  Cycles() {
    const root = el("div");
    for (const [title, list] of [["Between top-level modules", M.cycles.by_top_module], ["Between modules", M.cycles.by_module]]) {
      root.append(el("h2", {}, title + " (" + list.length + ")"));
      if (!list.length) root.append(el("p", { class: "muted" }, "None."));
      for (const c of list) {
        root.append(
          el(
            "div",
            { class: "scroll" },
            el("p", { class: "muted" }, c.crate),
            el("ul", {}, c.modules.map((m) => el("li", {}, idLink(m)))),
            el("ul", {}, c.edges.map((e) => el("li", {}, idLink(e.from), " -> ", idLink(e.to), " x" + e.count + " ", el("span", { class: "muted mono" }, locText(e.first)))))
          )
        );
      }
    }
    return root;
  },

  "Wide structs"() {
    const root = el("div");
    if (!M.wide_structs.length) root.append(el("p", { class: "muted" }, "None."));
    for (const w of M.wide_structs) {
      const mods = [...new Set(Object.values(w.uses).flatMap((m) => Object.keys(m)))].sort();
      const head = el("tr", {}, el("th", {}, "field"), mods.map((m) => el("th", { class: "num" }, m)));
      const rows = w.fields.map((f) => el("tr", {}, el("td", {}, f), mods.map((m) => el("td", { class: "num" }, num((w.uses[f] || {})[m])))));
      root.append(el("h2", {}, idLink(w.id)), el("p", { class: "muted mono" }, locText(w.loc)), el("div", { class: "scroll" }, el("table", {}, el("thead", {}, head), el("tbody", {}, rows))));
    }
    return root;
  },

  Orphans() {
    return table(
      [
        { label: "package", cell: (r) => r.package, sort: (r) => r.package },
        { label: "file", cell: (r) => r.file, sort: (r) => r.file },
        { label: "lines", num: true, cell: (r) => num(r.lines), sort: (r) => r.lines },
      ],
      M.orphans,
      (r) => r.package + " " + r.file,
      [2, -1]
    );
  },

  Warnings() {
    const root = el("div");
    if (!M.warnings.length) root.append(el("p", { class: "muted" }, "None."));
    for (const g of M.warnings) {
      root.append(
        el("h2", {}, g.kind + " (" + g.count + ")"),
        el("p", { class: "muted" }, g.count > g.first.length ? "First " + g.first.length + " shown." : ""),
        el("div", { class: "scroll" }, el("table", {}, el("tbody", {}, g.first.map((w) => el("tr", {}, el("td", {}, w.message), el("td", { class: "mono" }, locText(w.loc)))))))
      );
    }
    return root;
  },
};

function section(title, ...kids) {
  return [el("h3", {}, title), ...kids];
}

function openDetail(index) {
  const u = M.units[index];
  const d = document.getElementById("detail");
  const edgeList = (ids, other) =>
    el(
      "ul",
      {},
      ids.map((i) => {
        const e = M.edges[i];
        return el("li", {}, unitLink(e[other]), " ", el("span", { class: "muted" }, e.position + " x" + e.count + " " + locText(e.first)));
      })
    );
  const parts = [
    el("button", { class: "btn close", type: "button", onclick: () => (d.hidden = true) }, "close"),
    el("h3", { class: "mono" }, u.id),
    el("p", { class: "muted" }, u.kind + " in " + u.crate + (u.test ? ", test" : "") + ", " + u.lines + " lines, " + locText(u.loc)),
  ];
  M.groups.forEach((g, i) => {
    const t = u.taint[i];
    if (!t) return;
    parts.push(el("h3", {}, g, " ", badge(t.state), " ", el("span", { class: "muted" }, t.cuts + " cuts")));
    if (t.direct.length) {
      parts.push(el("ul", {}, t.direct.map((e) => el("li", {}, e.package + " " + e.symbol, " ", el("span", { class: "muted" }, e.position + " x" + e.count + " " + locText(e.first))))));
    }
    if (t.via.length) parts.push(el("p", { class: "muted" }, "via"), el("ul", {}, t.via.map((v) => el("li", {}, idLink(v)))));
  });
  if (u.uses && u.uses.length) parts.push(...section("Uses (" + u.uses.length + ")", edgeList(u.uses, "to")));
  if (u.used_by && u.used_by.length) parts.push(...section("Used by (" + u.used_by.length + ")", edgeList(u.used_by, "from")));
  d.replaceChildren(...parts);
  d.hidden = false;
  d.scrollTop = 0;
}

function showTab(name) {
  document.querySelectorAll("#tabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === name)));
  document.getElementById("view").replaceChildren(TABS[name]());
}

function init() {
  const o = M.overview;
  document.getElementById("headline").textContent = M.name + " topology";
  const h = o.headline;
  document.getElementById("subline").textContent =
    o.crates.length + " crates, " + o.total_lines + " lines. " + M.headline_group + ": clean " + h.clean + ", signature " + h.signature + ", body " + h.body + ", inherited " + h.inherited + " lines.";
  const nav = document.getElementById("tabs");
  for (const name of Object.keys(TABS)) {
    const b = el("button", { type: "button", role: "tab", "data-tab": name, onclick: () => showTab(name) }, name);
    nav.append(b);
  }
  showTab("Overview");
}
init();
