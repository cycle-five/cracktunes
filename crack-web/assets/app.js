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

  let state = JSON.parse(document.getElementById("initial").textContent);
  let dragging = false;
  // The newest state the stream sent while it could not be drawn. It is kept
  // until drawn: the stream sends a view once, on change, and never again.
  let pending = null;

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

  function render() {
    const v = state.view;
    nowEl.replaceChildren();
    listEl.replaceChildren();
    if (v.state === "idle") {
      nowEl.appendChild(el("p", "empty", "Nothing is playing."));
    } else if (v.state === "hidden") {
      nowEl.appendChild(el("p", "empty", "A Guilty Pleasure game is on — the queue is hidden until it ends."));
    } else {
      const now = track(v.now, false);
      now.classList.add("now");
      nowEl.appendChild(now);
      for (const t of v.upcoming) listEl.appendChild(track(t, state.can_control));
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
    // The answer's view is read after the move; without one, the newest view
    // the stream sent, else the one on screen. Never keep an order the server
    // did not accept.
    const base = pending || state;
    const next = body && body.view ? { ...base, view: body.view } : base;
    if (dragging) { pending = next; return; } // another drag began meanwhile
    pending = null;
    state = next;
    render();
  }

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
