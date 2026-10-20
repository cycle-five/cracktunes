// The rooms settings page: keeps the name preview current, saves the form
// as JSON and shows what the server answered. Every string from the server
// goes in with textContent (the CSP's Trusted Types forbids HTML sinks).
(() => {
  "use strict";

  const root = document.getElementById("settings");
  if (!root) return;
  const guild = root.dataset.guild;

  const form = document.getElementById("rooms-form");
  const enabled = document.getElementById("s-enabled");
  const hub = document.getElementById("s-hub");
  const template = document.getElementById("s-template");
  const preview = document.getElementById("s-preview");
  const limit = document.getElementById("s-limit");
  const grace = document.getElementById("s-grace");
  const save = document.getElementById("s-save");
  const note = document.getElementById("s-note");
  const findings = document.getElementById("s-findings");

  // The bot's rule (render_room_name) for the sample name "Saucey": fill
  // {name}, drop control characters, trim, cap at 100 characters, and fall
  // back to "Room" when nothing is left.
  function render(t) {
    const filled = t
      .split("{name}")
      .join("Saucey")
      .replace(/[\u0000-\u001f\u007f-\u009f]/g, "")
      .trim();
    const capped = Array.from(filled).slice(0, 100).join("").trim();
    return capped || "Room";
  }

  function say(text, bad) {
    note.textContent = text || "";
    note.hidden = !text;
    note.className = bad ? "bad" : "";
  }

  function showFindings(view) {
    findings.textContent = "";
    if (view.findings === null) {
      const p = document.createElement("p");
      p.textContent = view.hub === null ? findings.dataset.noHub : findings.dataset.unknown;
      findings.appendChild(p);
    } else if (view.findings.length === 0) {
      const p = document.createElement("p");
      p.textContent = findings.dataset.ready;
      findings.appendChild(p);
    } else {
      const ul = document.createElement("ul");
      for (const f of view.findings) {
        const li = document.createElement("li");
        li.textContent = f;
        ul.appendChild(li);
      }
      findings.appendChild(ul);
    }
  }

  template.addEventListener("input", () => {
    preview.textContent = render(template.value);
  });

  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    save.disabled = true;
    say("");
    const body = {
      enabled: enabled.checked,
      hub: hub.value === "" ? null : hub.value,
      name_template: template.value,
      default_limit: Number(limit.value),
      empty_grace_secs: Number(grace.value),
    };
    try {
      const r = await fetch(`/g/${guild}/settings/rooms`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      const text = await r.text();
      let data = null;
      try {
        data = JSON.parse(text);
      } catch (_) {
        data = null;
      }
      if (r.ok && data) {
        showFindings(data);
        say(root.dataset.saved, false);
      } else {
        say((data && data.error) || `Error ${r.status}`, true);
      }
    } catch (_) {
      say(root.dataset.offline, true);
    } finally {
      save.disabled = false;
    }
  });

  // ---- Mod log ----
  const logForm = document.getElementById("modlog-form");
  if (logForm) {
    const logEnabled = document.getElementById("m-enabled");
    const logChannel = document.getElementById("m-channel");
    const logSave = document.getElementById("m-save");
    const logNote = document.getElementById("m-note");
    const logVisible = document.getElementById("m-visible");
    const logFindings = document.getElementById("m-findings");

    function logSay(text, bad) {
      logNote.textContent = text || "";
      logNote.hidden = !text;
      logNote.className = bad ? "bad" : "";
    }

    function showLog(view) {
      logVisible.textContent = view.visibility || "";
      logFindings.textContent = "";
      if (view.findings === null) {
        const p = document.createElement("p");
        p.textContent =
          view.channel === null ? logFindings.dataset.noChannel : logFindings.dataset.unknown;
        logFindings.appendChild(p);
      } else if (view.findings.length === 0) {
        const p = document.createElement("p");
        p.textContent = logFindings.dataset.ready;
        logFindings.appendChild(p);
      } else {
        const ul = document.createElement("ul");
        for (const f of view.findings) {
          const li = document.createElement("li");
          li.textContent = f;
          ul.appendChild(li);
        }
        logFindings.appendChild(ul);
      }
    }

    logForm.addEventListener("submit", async (ev) => {
      ev.preventDefault();
      logSave.disabled = true;
      logSay("");
      const body = {
        enabled: logEnabled.checked,
        channel: logChannel.value === "" ? null : logChannel.value,
      };
      try {
        const r = await fetch(`/g/${guild}/settings/modlog`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
        const text = await r.text();
        let data = null;
        try {
          data = JSON.parse(text);
        } catch (_) {
          data = null;
        }
        if (r.ok && data) {
          showLog(data);
          logSay(root.dataset.saved, false);
        } else {
          logSay((data && data.error) || `Error ${r.status}`, true);
        }
      } catch (_) {
        logSay(root.dataset.offline, true);
      } finally {
        logSave.disabled = false;
      }
    });
  }

  // ---- Economy ----
  const ecoForm = document.getElementById("economy-form");
  if (ecoForm) {
    const el = (id) => document.getElementById(id);
    const ecoSave = el("e-save");
    const ecoNote = el("e-note");

    function ecoSay(text, bad) {
      ecoNote.textContent = text || "";
      ecoNote.hidden = !text;
      ecoNote.className = bad ? "bad" : "";
    }

    function showEconomy(view) {
      el("e-enabled").checked = view.enabled;
      el("e-name").value = view.currency_name;
      el("e-emoji").value = view.currency_emoji;
      el("e-voice").value = view.voice_credits_per_minute;
      el("e-message").value = view.message_credits;
      el("e-cooldown").value = view.message_cooldown_seconds;
      for (const p of view.prices) {
        for (const row of el("e-prices").querySelectorAll("tr")) {
          if (row.dataset.kind === p.kind) {
            row.querySelector(".e-price-on").checked = p.on;
            row.querySelector(".e-price").value = p.price;
          }
        }
      }
      el("e-optout").checked = view.immunity_optout;
      el("e-shop").checked = view.immunity_shop;
      el("e-mods").checked = view.immunity_mods;
      el("e-optout-days").value = view.optout_cooldown_days;
      el("e-hours").value = view.bought_immunity_hours;
      el("e-gambling").checked = view.gambling_enabled;
      el("e-max-bet").value = view.max_bet;
    }

    // A blank or non-numeric box is NaN, which JSON turns into null: the
    // server then refuses the body as unreadable rather than guessing.
    const num = (id) => (el(id).value.trim() === "" ? NaN : Number(el(id).value));

    ecoForm.addEventListener("submit", async (ev) => {
      ev.preventDefault();
      ecoSave.disabled = true;
      ecoSay("");
      const prices = [];
      for (const row of el("e-prices").querySelectorAll("tr")) {
        const price = row.querySelector(".e-price").value.trim();
        prices.push({
          kind: row.dataset.kind,
          price: price === "" ? NaN : Number(price),
          on: row.querySelector(".e-price-on").checked,
        });
      }
      const body = {
        enabled: el("e-enabled").checked,
        currency_name: el("e-name").value,
        currency_emoji: el("e-emoji").value,
        voice_credits_per_minute: num("e-voice"),
        message_credits: num("e-message"),
        message_cooldown_seconds: num("e-cooldown"),
        prices,
        immunity_optout: el("e-optout").checked,
        immunity_shop: el("e-shop").checked,
        immunity_mods: el("e-mods").checked,
        optout_cooldown_days: num("e-optout-days"),
        bought_immunity_hours: num("e-hours"),
        gambling_enabled: el("e-gambling").checked,
        max_bet: num("e-max-bet"),
      };
      try {
        const r = await fetch(`/g/${guild}/settings/economy`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        });
        const text = await r.text();
        let data = null;
        try {
          data = JSON.parse(text);
        } catch (_) {
          data = null;
        }
        if (r.ok && data) {
          showEconomy(data);
          ecoSay(root.dataset.saved, false);
        } else {
          ecoSay((data && data.error) || `Error ${r.status}`, true);
        }
      } catch (_) {
        ecoSay(root.dataset.offline, true);
      } finally {
        ecoSave.disabled = false;
      }
    });
  }
})();
