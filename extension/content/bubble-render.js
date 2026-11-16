// Bubble appearance application, option description formatting, and menu rendering.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  CS.applyLook = () => {
    for (const b of CS.bubbles.values()) {
      const w = b.root.querySelector(".wrap");
      if (w) w.className = "wrap " + CS.look;
      CS.placeOne(b);
    }
  };

  CS.applyZoom = () => {
    for (const b of CS.bubbles.values()) {
      b.host.style.setProperty("--z", String(CS.zoom));
      b.host.style.setProperty("--o", String(CS.solid));
      CS.placeOne(b);
    }
  };

  // One line of the menu, in plain words: "MP4 file · 1080p [Full HD] AV1",
  // under it "12:34 · 2,500 kbps · 245 MB".
  CS.describe = (m) => {
    const y = m.yt;
    if (!y) return { main: m.label, tag: "", codec: "", sub: m.sub || "" };
    const audio = y.quality === "audio" || y.quality === "mp3" || /^audio\|/.test(y.key || "");
    const ext = (y.ext || "mp4").toUpperCase();
    const q = audio ? "" : y.quality || "";
    const tag = audio ? "" : y.tag || CS.tagOf(CS.heightOf(q));
    const codec = audio ? "" : y.codec || (/\bAV1\b/.test(m.label || "") ? "AV1" : "");
    const kbps = y.kbps || (y.size && y.duration ? Math.round((y.size * 8) / y.duration / 1000) : 0);
    const sub = [
      CS.fmtLen(y.duration),
      kbps ? `${kbps.toLocaleString("en")} ${aknanI18n.t("kbps")}` : "",
      y.size ? (y.quality === "mp3" ? aknanI18n.t("about ") : "") + CS.fmtSize(y.size) : "",
      y.quality === "mp3" ? aknanI18n.t("made into MP3 after the download") : "",
      // some players play only the sound of an AV1 video
      codec === "AV1" ? aknanI18n.t("AV1: smaller file, needs a newer player") : "",
    ]
      .filter(Boolean)
      .join(" · ");
    const kind = aknanI18n.t(audio ? "{} audio" : "{} file", ext);
    return { main: q ? `${kind} · ${q}` : kind, tag, codec, sub };
  };

  // the list may open up or down: decided again after every change of it
  CS.renderOne = (b, note, isError, action) => {
    CS.renderBubble(b, note, isError, action);
    // a protected (DRM) video: the button says so with a lock
    const locked = /🔒/.test(note || "") || CS.itemsOf(b).some((i) => i.drm);
    b.root.getElementById("main").classList.toggle("locked", locked);
    CS.placeOne(b);
  };

  CS.renderBubble = (b, note, isError, action) => {
    const { el } = CS;
    const list = CS.itemsOf(b);
    const usable = list.filter((i) => !i.disabled);
    const busy = b.loading || (!usable.length && list.some((i) => i.busy || /Getting/.test(i.label)));
    const wrap = b.root.querySelector(".wrap");
    if (wrap) wrap.setAttribute("dir", aknanI18n.isRtl() ? "rtl" : "ltr");
    b.root.getElementById("grab").setAttribute("aria-label", aknanI18n.t("Download this video"));
    b.root.getElementById("help").setAttribute("title", aknanI18n.t("Help"));
    b.root.getElementById("help").setAttribute("aria-label", aknanI18n.t("Help"));
    b.root.getElementById("close").setAttribute("title", aknanI18n.t("Hide"));
    b.root.getElementById("close").setAttribute("aria-label", aknanI18n.t("Hide"));
    b.root.getElementById("label").textContent = usable.length > 1 ? aknanI18n.t("Download this video ({})", usable.length) : aknanI18n.t("Download this video");
    b.root.getElementById("spin").hidden = !busy;
    b.root.querySelector("svg").style.display = busy ? "none" : "";
    b.root.getElementById("main").classList.toggle("open", b.open);
    const box = b.root.getElementById("list");
    box.hidden = !b.open;
    box.replaceChildren();
    if (b.help) {
      box.appendChild(
        el("div", {
          class: "help",
          text: aknanI18n.t("Click the button (or press Enter on it) to choose a quality. Drag it to an edge of the video: it stays there on this site. ✕ hides it for this video. Its size can be changed in the Aknan Download add-in menu."),
        })
      );
      const never = el("div", { class: "act", tabindex: "0", role: "button", text: aknanI18n.t("Don't show the button on this site") });
      never.addEventListener("click", (e) => CS.userPress(e) && CS.noButtonOnThisSite());
      never.addEventListener("keydown", (e) => e.key === "Enter" && e.isTrusted && CS.noButtonOnThisSite());
      box.appendChild(never);
    }
    if (note) {
      // dir "auto": a note in Arabic reads right to left (its full stop at the end)
      const d = el("div", { class: "msg" + (isError ? " err" : ""), dir: "auto", text: note });
      if (action) {
        const a = el("div", { class: "act", text: action.text });
        a.addEventListener("click", action.run);
        d.append(el("br"), a);
      }
      box.appendChild(d);
      return;
    }
    if (!list.length) {
      box.appendChild(el("div", { class: "msg", text: aknanI18n.t("Looking for the video qualities…") }));
      return;
    }
    const title = (list.find((m) => m.yt && m.yt.title) || {}).yt;
    box.appendChild(el("div", { class: "head", dir: "auto", title: (title && title.title) || CS.cleanTitle(), text: (title && title.title) || CS.cleanTitle() }));
    // two tabs: the whole video (never a part), or a part of it
    const partOk = usable.some(CS.canCut);
    const partTab = b.clipMode && partOk;
    if (partOk) {
      const tab = (on, text, part) => {
        const t = el("div", { class: "tab" + (part ? " part" : "") + (on ? " on" : ""), text });
        t.addEventListener("click", () => {
          if (b.clipMode === part) return;
          b.clipMode = part;
          if (part && !b.clip) {
            const v = b.video;
            const at = Math.floor(v.currentTime || 0);
            const dur = Number.isFinite(v.duration) ? v.duration : 0;
            b.clip = { from: at, to: dur ? Math.min(dur, at + 30) : at + 30, exact: false };
          }
          if (!part) CS.stopPart(b);
          CS.renderOne(b);
        });
        return t;
      };
      box.appendChild(el("div", { class: "tabs" }, tab(!partTab, aknanI18n.t("⬇ Whole video"), false), tab(partTab, aknanI18n.t("✂ Part"), true)));
    }
    if (partTab) {
      box.appendChild(CS.clipPanel(b, usable));
      return;
    }
    const all = CS.downloadAllOf(b, usable);
    if (all) {
      const row = el("div", { class: "item" }, el("span", { class: "num", text: "⇊" }), el("div", { class: "body" }, el("div", { class: "t all", text: all.text })));
      row.addEventListener("click", all.run);
      box.appendChild(row);
    }
    let n = 0;
    list.forEach((m) => {
      const d = CS.describe(m);
      const t = el("div", { class: "t" }, el("span", { text: d.main }));
      if (d.tag) t.append(el("span", { class: "tag " + d.tag.replace(/\s/g, "").replace(/^(\d)K$/, "K$1"), text: d.tag }));
      if (d.codec) t.append(el("span", { class: "codec", text: d.codec }));
      const sub = d.sub.length > 110 ? d.sub.slice(0, 110) + "…" : d.sub;
      const row = el(
        "div",
        { class: "item" + (m.disabled ? " off" : "") },
        el("span", { class: "num", text: m.disabled ? "" : `${++n}.` }),
        el("div", { class: "body" }, t, el("div", { class: "s", text: sub }))
      );
      if (!m.disabled) {
        row.setAttribute("tabindex", "0");
        row.setAttribute("role", "button");
        row.addEventListener("click", () => CS.download(b, m));
        row.addEventListener("keydown", (e) => {
          if (e.key === "Enter" && e.isTrusted) {
            e.preventDefault();
            CS.download(b, m);
          }
        });
      }
      box.appendChild(row);
    });
  };
})();
