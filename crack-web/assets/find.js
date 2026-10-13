// Find a track: search the queue, this server's history, YouTube and
// SoundCloud, then add a hit or play it next. Third-party text (titles,
// artists) only ever reaches the page through textContent and setAttribute.
(() => {
  "use strict";
  const form = document.getElementById("find");
  const dash = window.crackDash;
  if (!form || !dash) return; // not the queue page
  const root = document.getElementById("dash");
  const guild = root.dataset.guild;
  const input = document.getElementById("find-q");
  const go = document.getElementById("find-go");
  const results = document.getElementById("results");
  const boxes = Array.from(document.querySelectorAll("#find-src input[type=checkbox]"));
  const KEY = "crack.find.sources";
  const NAMES = { queue: "Queue", history: "History", youtube: "YouTube", soundcloud: "SoundCloud" };
  const STATUS = {
    hidden: "Hidden while a Guilty Pleasure game is on.",
    no_database: "This bot keeps no history.",
    failed: "did not answer.",
    timed_out: "took too long.",
  };

  try {
    const saved = JSON.parse(localStorage.getItem(KEY) || "null");
    if (Array.isArray(saved)) for (const b of boxes) b.checked = saved.includes(b.value);
  } catch (_) { /* private window or blocked storage: keep the defaults */ }
  for (const b of boxes) {
    b.addEventListener("change", () => {
      try { localStorage.setItem(KEY, JSON.stringify(boxes.filter((x) => x.checked).map((x) => x.value))); } catch (_) {}
    });
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function duration(secs) {
    if (secs === null || secs === undefined) return "";
    const m = Math.floor(secs / 60);
    return `${m}:${String(secs % 60).padStart(2, "0")}`;
  }

  function ago(unix) {
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
    if (s < 3600) return `${Math.max(1, Math.floor(s / 60))} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return `${Math.floor(s / 86400)} d ago`;
  }

  function detail(d) {
    if (d.kind === "queue") return d.position === 0 ? "now playing" : `#${d.position} in queue`;
    if (d.kind === "history") return `queued ${d.plays}× · ${ago(d.last_unix)}`;
    return "";
  }

  // Why the add buttons are off, from the live state; null when they are on.
  function addBlocked() {
    const s = dash.state();
    if (s.view && s.view.state === "hidden") return "A Guilty Pleasure game is on — the queue is locked.";
    if (s.plan !== "premium") return "Adding from the dashboard is a premium feature.";
    if (!s.can_control) return "Join the bot's voice channel to add tracks.";
    return null;
  }

  function refreshButtons() {
    const blocked = addBlocked();
    for (const b of results.querySelectorAll("button.add")) {
      if (b.dataset.busy !== "1") b.disabled = !!blocked;
      if (blocked) b.title = blocked; else b.removeAttribute("title");
    }
  }
  dash.onState(refreshButtons);

  async function add(row, id, at, title) {
    const buttons = row.querySelectorAll("button.add");
    for (const b of buttons) { b.disabled = true; b.dataset.busy = "1"; }
    let body = null;
    let status = 0;
    try {
      const res = await fetch(`/g/${guild}/add`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hit: id, at }),
      });
      status = res.status;
      body = await res.json().catch(() => null);
    } catch (_) {
      dash.say("Could not reach the server. Nothing was added.");
    } finally {
      for (const b of buttons) delete b.dataset.busy;
    }
    const result = body && body.result;
    if (result === "added") dash.say(at === "next" ? `Up next: ${title}.` : `Added ${title}.`);
    else if (result === "expired") { dash.say("Those results expired — search again."); results.replaceChildren(); }
    else if (result === "not_allowed") dash.say("Join the bot's voice channel to add tracks.");
    else if (result === "premium_required") dash.say("Adding from the dashboard is a premium feature.");
    else if (result === "game_in_progress") dash.say("A Guilty Pleasure game is on — the queue is locked.");
    else if (result === "not_playing") dash.say("The bot is not in a voice channel.");
    else if (result === "too_many") dash.say("Slow down a little.");
    else if (result === "failed") dash.say("That track could not be queued.");
    else if (status) dash.say(`That did not work (${status}).`);
    if (body && body.view) dash.settle(body);
    refreshButtons();
  }

  function hitRow(h) {
    const li = el("li", "track hit");
    const title = el("a", "title", h.title);
    title.href = h.url; // the server sends http(s) only
    title.rel = "noopener noreferrer";
    title.target = "_blank";
    li.appendChild(title);
    li.appendChild(el("span", "meta", [h.artist, duration(h.duration_secs), detail(h.detail)].filter(Boolean).join(" · ")));
    for (const [at, label, aria] of [["end", "＋ Add", "Add"], ["next", "⏭ Next", "Play next:"]]) {
      const b = el("button", "add", label);
      b.type = "button";
      b.setAttribute("aria-label", `${aria} ${h.title}`);
      b.addEventListener("click", () => add(li, h.id, at, h.title));
      li.appendChild(b);
    }
    return li;
  }

  function draw(answer) {
    results.replaceChildren();
    const free = dash.state().plan !== "premium";
    for (const s of answer.sources) {
      const block = el("section", "source");
      block.appendChild(el("h2", null, NAMES[s.source] || s.source));
      if (s.status === "ok" && s.hits.length === 0) block.appendChild(el("p", "empty", "No matches."));
      else if (s.status === "failed" || s.status === "timed_out") block.appendChild(el("p", "empty", `${NAMES[s.source]} ${STATUS[s.status]}`));
      else if (s.status !== "ok") block.appendChild(el("p", "empty", STATUS[s.status] || s.status));
      if (s.source === "history" && free) block.appendChild(el("p", "empty", "Free servers search the last 24 hours."));
      if (s.hits.length) {
        const ol = el("ol", "hits");
        for (const h of s.hits) ol.appendChild(hitRow(h));
        block.appendChild(ol);
      }
      results.appendChild(block);
    }
    refreshButtons();
  }

  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const q = input.value.trim();
    const src = boxes.filter((b) => b.checked).map((b) => b.value);
    if (q.length < 2) { dash.say("Type at least two characters to search."); return; }
    if (!src.length) { dash.say("Pick at least one place to search."); return; }
    go.disabled = true;
    results.replaceChildren(el("p", "empty", "Searching…"));
    try {
      const params = new URLSearchParams({ q, src: src.join(",") });
      const res = await fetch(`/g/${guild}/search?${params}`, { credentials: "same-origin" });
      const body = await res.json().catch(() => null);
      if (res.ok && body) { dash.say(""); draw(body); }
      else if (res.status === 429) { results.replaceChildren(); dash.say("Slow down a little."); }
      else { results.replaceChildren(); dash.say((body && body.error) || `That did not work (${res.status}).`); }
    } catch (_) {
      results.replaceChildren();
      dash.say("Could not reach the server.");
    } finally {
      go.disabled = false;
    }
  });
})();
