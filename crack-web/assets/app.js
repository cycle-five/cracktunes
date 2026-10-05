// The dashboard's one renderer. It draws the view inlined in the page and
// every view the server sends after it. Track titles are third-party text,
// so nothing here parses a string as HTML: every node is built with
// createElement and filled with textContent.
(() => {
  "use strict";

  // The note under the heading; pages without one get it added above
  // their content.
  function say(text) {
    let note = document.getElementById("note");
    if (!note) {
      note = document.createElement("p");
      note.id = "note";
      document.querySelector("main").prepend(note);
    }
    note.textContent = text || "";
    note.hidden = !text;
  }

  const logout = document.getElementById("logout");
  if (logout) {
    logout.addEventListener("click", async () => {
      try {
        const res = await fetch("/auth/logout", { method: "POST", credentials: "same-origin" });
        if (!res.ok) throw new Error(String(res.status));
      } catch (_) {
        say("Could not log out. Try again in a moment.");
        return;
      }
      window.location.assign("/");
    });
  }

  const root = document.getElementById("dash");
  if (!root) return; // the picker page
  const guild = root.dataset.guild;
  const nowEl = document.getElementById("now");
  const listEl = document.getElementById("upcoming");
  const badge = document.getElementById("badge");
  const controlsEl = document.getElementById("controls");
  const premiumEl = document.getElementById("premium-controls");
  const pauseBtn = document.getElementById("c-pause");
  const skipBtn = document.getElementById("c-skip");
  const shuffleBtn = document.getElementById("c-shuffle");
  const repeatBtn = document.getElementById("c-repeat");

  let state = JSON.parse(document.getElementById("initial").textContent);
  let dragging = false;
  // The newest state the stream sent while it could not be drawn. It is kept
  // until drawn: the stream sends a view once, on change, and never again.
  let pending = null;
  // A control request is on its way. The buttons stay disabled until its
  // answer is drawn; the server's `expect` is the real guard, this is courtesy.
  let busy = false;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function duration(secs) {
    if (secs === null || secs === undefined) return "";
    const m = Math.floor(secs / 60);
    const s = String(secs % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  function track(t, handle) {
    const li = el("li", "track");
    li.dataset.id = t.id;
    if (handle) li.appendChild(el("span", "handle", "⠿"));
    const title = t.url ? el("a", "title", t.title) : el("span", "title", t.title);
    if (t.url) {
      title.href = t.url;
      title.rel = "noopener noreferrer";
      title.target = "_blank";
    }
    li.appendChild(title);
    li.appendChild(el("span", "meta", [duration(t.duration_secs), t.requester].filter(Boolean).join(" · ")));
    return li;
  }

  // The ✕ on an upcoming row. The title is third-party text: it reaches the
  // label through setAttribute, which never parses it.
  function removeButton(t, enabled) {
    const b = el("button", "remove", "✕");
    b.type = "button";
    b.setAttribute("aria-label", `Remove ${t.title || "this track"}`);
    b.disabled = !enabled;
    b.addEventListener("click", () => control({ type: "remove", id: t.id }));
    return b;
  }

  function render() {
    const v = state.view;
    nowEl.replaceChildren();
    listEl.replaceChildren();
    const playing = v.state === "playing";
    const premium = state.plan === "premium";
    const live = playing && premium && !!state.can_control && !busy;
    controlsEl.hidden = !playing;
    premiumEl.hidden = !playing || premium;
    for (const b of [pauseBtn, skipBtn, shuffleBtn, repeatBtn]) b.disabled = !live;
    if (playing) {
      pauseBtn.textContent = v.paused ? "▶ Resume" : "⏸ Pause";
      repeatBtn.setAttribute("aria-pressed", String(!!v.looping));
      repeatBtn.classList.toggle("pressed", !!v.looping);
    }
    if (v.state === "idle") {
      nowEl.appendChild(el("p", "empty", "Nothing is playing."));
    } else if (v.state === "hidden") {
      nowEl.appendChild(el("p", "empty", "A Guilty Pleasure game is on — the queue is hidden until it ends."));
    } else {
      const now = track(v.now, false);
      now.classList.add("now");
      nowEl.appendChild(now);
      for (const t of v.upcoming) {
        const li = track(t, state.can_control);
        li.appendChild(removeButton(t, live));
        listEl.appendChild(li);
      }
      if (v.upcoming.length === 0) listEl.appendChild(el("li", "empty", "Nothing queued after this."));
    }
    sortable.option("disabled", !(state.can_control && v.state === "playing"));
    root.classList.toggle("can-control", !!state.can_control);
  }

  async function move(id, to) {
    let body = null;
    let status = 0;
    try {
      const res = await fetch(`/g/${guild}/move`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id, to }),
      });
      status = res.status;
      body = await res.json().catch(() => null);
    } catch (_) {
      say("Could not reach the server. Nothing was moved.");
    }
    const result = body && body.result;
    if (result === "moved") say("");
    else if (result === "conflict") say("The queue changed while you were dragging — here it is now.");
    else if (result === "not_allowed") say("Join the bot's voice channel to reorder.");
    else if (result === "game_in_progress") say("A Guilty Pleasure game is on — the queue is locked.");
    else if (result === "not_playing") say("Nothing is playing.");
    else if (status) say(`That did not work (${status}).`);
    // The answer's view is read after the move. Never keep an order the
    // server did not accept; if another drag began meanwhile, it waits.
    settle(body);
  }

  // Draw an answer's view, or else the newest the stream sent, else the one
  // on screen; never mid-drag, where it waits in `pending` like the stream's.
  function settle(body) {
    const base = pending || state;
    const next = body && body.view ? { ...base, view: body.view } : base;
    if (dragging) { pending = next; return; }
    pending = null;
    state = next;
    render();
  }

  async function control(req) {
    if (busy) return;
    busy = true;
    for (const b of root.querySelectorAll("#controls button, .remove")) b.disabled = true;
    let body = null;
    let status = 0;
    try {
      const res = await fetch(`/g/${guild}/control`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(req),
      });
      status = res.status;
      body = await res.json().catch(() => null);
    } catch (_) {
      say("Could not reach the server. Nothing changed.");
    }
    const result = body && body.result;
    if (result === "done") say("");
    else if (result === "conflict") say("The queue changed — here it is now.");
    else if (result === "not_allowed") say("Join the bot's voice channel to use the controls.");
    else if (result === "premium_required") say("Dashboard controls are a premium feature.");
    else if (result === "game_in_progress") say("A Guilty Pleasure game is on — the queue is locked.");
    else if (result === "not_playing") say("Nothing is playing.");
    else if (result === "too_many") say("Slow down a little.");
    else if (result === "failed") say("That did not work.");
    else if (status) say(`That did not work (${status}).`);
    busy = false;
    settle(body);
  }

  // Each control reads the view on screen at click time.
  function playingView() {
    const v = state.view;
    return v.state === "playing" ? v : null;
  }
  pauseBtn.addEventListener("click", () => {
    const v = playingView();
    if (v) control({ type: v.paused ? "resume" : "pause" });
  });
  skipBtn.addEventListener("click", () => {
    const v = playingView();
    if (v) control({ type: "skip", id: v.now.id });
  });
  shuffleBtn.addEventListener("click", () => {
    if (playingView()) control({ type: "shuffle" });
  });
  repeatBtn.addEventListener("click", () => {
    const v = playingView();
    if (v) control({ type: "repeat", on: !v.looping });
  });

  const sortable = Sortable.create(listEl, {
    handle: ".handle",
    animation: 150,
    disabled: true,
    onStart: () => { dragging = true; },
    onEnd: (ev) => {
      dragging = false;
      if (ev.oldIndex === ev.newIndex) {
        if (pending) { state = pending; pending = null; render(); }
        return;
      }
      move(ev.item.dataset.id, ev.newIndex); // `pending` waits for its answer
    },
  });

  const stream = new EventSource(`/g/${guild}/events`);
  stream.onmessage = (e) => {
    const next = JSON.parse(e.data);
    badge.hidden = true;
    if (dragging) { pending = next; return; } // do not yank a row from under the cursor
    pending = null; // superseded
    state = next;
    render();
  };
  stream.onerror = () => {
    badge.textContent = stream.readyState === EventSource.CLOSED
      ? "Disconnected — reload the page."
      : "Reconnecting…";
    badge.hidden = false;
  };

  render();
})();
