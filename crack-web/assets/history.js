// The history page: draws the inlined first page, then polls for new rows
// every 10 s while visible, loads older rows on demand, and refetches when a
// filter changes. Every string from the server goes in with textContent.
(() => {
  "use strict";

  const root = document.getElementById("history");
  if (!root) return;
  const guild = root.dataset.guild;
  const PAGE_SIZE = 50;
  const POLL_MS = 10000;

  const rowsEl = document.getElementById("rows");
  const olderBtn = document.getElementById("older");
  const badge = document.getElementById("badge");
  const note = document.getElementById("note");
  const gpNote = document.getElementById("gp-note");
  const fAction = document.getElementById("f-action");
  const fSource = document.getElementById("f-source");
  const fSince = document.getElementById("f-since");
  const chip = document.getElementById("f-user");
  const chipName = document.getElementById("f-user-name");
  const chipClear = document.getElementById("f-user-clear");

  const first = JSON.parse(document.getElementById("initial").textContent);
  let rows = first.rows;
  let older = first.older;
  let gameHidden = first.game_hidden;
  let user = null; // { id, label }
  let stopped = false;
  // Set when a reload failed: the controls show a filter that `rows` does not
  // match, so polls and "load older" must reload instead of extending rows.
  let dirty = false;
  // Bumped on every refetch, so a poll or "load older" that began before a
  // filter changed can never mix its rows into the new list.
  let seq = 0;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function say(text) {
    note.textContent = text || "";
    note.hidden = !text;
  }

  function ago(iso) {
    const secs = Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 1000));
    if (secs < 60) return "just now";
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins} min ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours} h ago`;
    return `${Math.floor(hours / 24)} d ago`;
  }

  function rowEl(r) {
    const li = el("li", "row");
    const time = el("span", "when", ago(r.at));
    time.title = new Date(r.at).toLocaleString();
    li.appendChild(time);
    if (r.who.kind === "bot") {
      li.appendChild(el("span", "who bot", "bot"));
    } else {
      const label = r.who.name || r.who.id;
      const who = el("button", "who", label);
      who.type = "button";
      who.title = "Show only this member's changes";
      who.addEventListener("click", () => setUser({ id: r.who.id, label }));
      li.appendChild(who);
    }
    li.appendChild(el("span", "how", r.how));
    li.appendChild(el("span", "what", r.what));
    return li;
  }

  function render() {
    if (rows.length === 0) {
      rowsEl.replaceChildren(el("li", "empty", "Nothing matches."));
    } else {
      rowsEl.replaceChildren(...rows.map(rowEl));
    }
    olderBtn.hidden = !older;
    gpNote.hidden = !gameHidden;
    chip.hidden = !user;
    chipName.textContent = user ? user.label : "";
  }

  function query(extra) {
    const p = new URLSearchParams();
    if (fAction.value) p.set("action", fAction.value);
    if (fSource.value) p.set("source", fSource.value);
    if (fSince.value) p.set("since", fSince.value);
    if (user) p.set("user", user.id);
    for (const [k, v] of Object.entries(extra)) p.set(k, String(v));
    return p.toString();
  }

  // Resolves to the page, or null when the answer means "stop".
  async function fetchPage(extra) {
    const res = await fetch(`/g/${guild}/history.json?${query(extra)}`, {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    });
    if (res.status === 401) {
      window.location.assign(`/auth/login?return_to=${encodeURIComponent(window.location.pathname)}`);
      stopped = true;
      return null;
    }
    if (res.status === 403 || res.status === 404) {
      stopped = true;
      const body = await res.json().catch(() => null);
      say((body && body.error) || "This history is no longer available to you.");
      return null;
    }
    if (!res.ok) throw new Error(String(res.status));
    return res.json();
  }

  async function reload() {
    if (stopped) return;
    const mine = ++seq;
    try {
      const data = await fetchPage({});
      if (!data || mine !== seq) return;
      rows = data.rows;
      older = data.older;
      gameHidden = data.game_hidden;
      badge.hidden = true;
      dirty = false;
      render();
    } catch (_) {
      if (mine === seq) {
        dirty = true;
        badge.hidden = false;
      }
    }
  }

  // One poll at a time: the interval, a tab becoming visible and the
  // page-full chain can overlap, and two polls asking for the same "after"
  // would prepend the same rows twice.
  let polling = false;

  async function poll() {
    if (polling || stopped || document.visibilityState !== "visible") return;
    polling = true;
    let more = false;
    try {
      more = await pollOnce();
    } finally {
      polling = false;
    }
    if (more) poll();
  }

  // Resolves true when a full page came back, so more may be waiting.
  async function pollOnce() {
    if (dirty || rows.length === 0) {
      await reload();
      return false;
    }
    const mine = seq;
    try {
      const data = await fetchPage({ after: rows[0].id });
      if (!data || mine !== seq) return false;
      badge.hidden = true;
      if (gameHidden && !data.game_hidden) {
        // The game ended: its rows can be shown now.
        await reload();
        return false;
      }
      gameHidden = data.game_hidden;
      if (data.rows.length > 0) rows = data.rows.concat(rows);
      render();
      return data.rows.length >= PAGE_SIZE;
    } catch (_) {
      if (mine === seq) badge.hidden = false;
      return false;
    }
  }

  async function loadOlder() {
    if (dirty) return reload();
    if (rows.length === 0) return;
    const mine = seq;
    olderBtn.disabled = true;
    try {
      const data = await fetchPage({ before: rows[rows.length - 1].id });
      if (!data || mine !== seq) return;
      rows = rows.concat(data.rows);
      older = data.older;
      render();
    } catch (_) {
      say("Could not load older entries. Try again in a moment.");
    } finally {
      olderBtn.disabled = false;
    }
  }

  function setUser(u) {
    user = u;
    reload();
  }

  for (const f of [fAction, fSource, fSince]) f.addEventListener("change", reload);
  chipClear.addEventListener("click", () => setUser(null));
  olderBtn.addEventListener("click", loadOlder);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") poll();
  });
  setInterval(poll, POLL_MS);
  render();
})();
