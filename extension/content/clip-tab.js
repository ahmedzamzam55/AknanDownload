// "✂ Part" clip selection tab, preview playback, and clipped download dispatch.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  // a quality that can be cut: a video / sound the app downloads itself
  CS.canCut = (m) => {
    if (m.disabled || m.drm || m.resolve) return false;
    if (m.yt) return true;
    return !!m.url && /\.(mp4|m4v|webm|mkv|mov|m3u8|mp3|m4a)(\?|$)/i.test(m.url);
  };

  // keys typed in our boxes stay ours (not the player's shortcuts)
  function keepKeys(input) {
    for (const t of ["keydown", "keyup", "keypress"]) input.addEventListener(t, (e) => e.stopPropagation());
  }

  // a short name of a quality for the part ("1080p", "MP3")
  function chipOf(m) {
    const y = m.yt;
    if (y) {
      if (y.quality === "mp3") return "MP3";
      if (y.quality === "audio" || /^audio\|/.test(y.key || "")) return (y.ext || "M4A").toUpperCase();
      return (y.quality || (y.ext || "MP4").toUpperCase()) + (y.codec ? " " + y.codec : "");
    }
    const h = CS.heightOf(m.label || "");
    return h ? `${h}p` : (m.ext || (m.url || "").split("?")[0].split(".").pop() || "file").toUpperCase();
  }

  // the page's own player plays the part (nothing is downloaded)
  function playPart(b) {
    const v = b.video;
    const c = b.clip;
    if (!(c.to > c.from)) return;
    CS.stopPart(b);
    const stop = () => {
      if (v.currentTime >= c.to || v.currentTime < c.from - 1) {
        v.pause();
        CS.stopPart(b);
      }
    };
    b.partWatch = stop;
    try {
      v.currentTime = c.from;
      v.addEventListener("timeupdate", stop);
      const p = v.play();
      if (p && p.catch) p.catch(() => {});
    } catch (_) {
      // a player that does not let us move it: nothing to show
    }
  }

  CS.stopPart = (b) => {
    if (b.partWatch) b.video.removeEventListener("timeupdate", b.partWatch);
    b.partWatch = null;
  };

  function downloadPart(b, m) {
    const c = b.clip;
    const dur = Number.isFinite(b.video.duration) ? b.video.duration : 0;
    if (!(c.to > c.from) || (dur && c.from >= dur)) return CS.flashOne(b, aknanI18n.t("The end must come after the start."), true);
    const clip = { start: c.from, end: dur ? Math.min(c.to, dur) : c.to, exact: !!c.exact };
    const fileName = `${CS.cleanTitle()}.${m.ext || "mp4"}`;
    CS.stopPart(b);
    chrome.runtime.sendMessage(
      { type: "swiftget-download", item: m, title: document.title, fileName, referrer: location.href, clip, part: true },
      (res) => {
        if (chrome.runtime.lastError || !res || !res.ok) CS.notRunning(b, () => downloadPart(b, m));
        else {
          b.clipMode = false;
          CS.flashOne(b, aknanI18n.t("The part {} → {} was sent to Aknan Download ✔", CS.clock(clip.start), CS.clock(clip.end)), false);
        }
      }
    );
  }

  // The "✂ Part" tab: From / To, quick or exact, the quality, a look at the
  // part on the page's own player, and its own button
  CS.clipPanel = (b, usable) => {
    const { el, clock, parseClock } = CS;
    const c = b.clip;
    const v = b.video;
    const dur = Number.isFinite(v.duration) ? v.duration : 0;
    const choices = usable.filter(CS.canCut);
    if (!choices.some((m) => m === b.cutPick)) {
      b.cutPick = choices.find((m) => m.yt && m.yt.quality === "720p") || choices.find((m) => CS.heightOf(m.label || "") === 720) || choices[0];
    }
    const len = el("span", { class: "len" });
    const ok = () => c.to > c.from && (!dur || c.from < dur);
    const showLen = () => {
      len.textContent = ok() ? clock(c.to - c.from) : "!";
      len.title = ok() ? aknanI18n.t("Length of the part") : aknanI18n.t("The end must come after the start.");
      len.classList.toggle("err", !ok());
    };
    const time = (label, key) => {
      const input = el("input", { type: "text", value: clock(c[key]), "aria-label": label });
      keepKeys(input);
      input.addEventListener("input", () => {
        const t = parseClock(input.value);
        input.classList.toggle("bad", t == null);
        if (t != null) {
          c[key] = dur ? Math.min(t, dur) : t;
          showLen();
        }
      });
      const now = el("span", { class: "now", title: aknanI18n.t("The time the video is at now"), text: aknanI18n.t("⏱ Now") });
      now.addEventListener("click", () => {
        c[key] = Math.floor(v.currentTime || 0);
        input.value = clock(c[key]);
        input.classList.remove("bad");
        showLen();
      });
      return { label: el("b", { text: label }), input, now };
    };
    const from = time(aknanI18n.t("From"), "from");
    const to = time(aknanI18n.t("To"), "to");
    const seg = el("div", { class: "seg" });
    const showSeg = () => {
      seg.replaceChildren();
      for (const [exact, text, words] of [
        [false, aknanI18n.t("Quick cut"), aknanI18n.t("Takes seconds, the quality stays as it is; it may start a second or two early.")],
        [true, aknanI18n.t("Exact cut"), aknanI18n.t("Starts on the very second you chose; the picture is made again, so it takes longer.")],
      ]) {
        const k = el("span", { class: c.exact === exact ? "on" : "", text, title: words });
        k.addEventListener("click", () => {
          c.exact = exact;
          showSeg();
        });
        seg.appendChild(k);
      }
    };
    const qs = el("div", { class: "qs" });
    const showQs = () => {
      qs.replaceChildren();
      for (const m of choices) {
        const d = CS.describe(m);
        const q = el("span", { class: m === b.cutPick ? "on" : "", text: chipOf(m), title: [d.main, d.sub].filter(Boolean).join(" · ") });
        q.addEventListener("click", () => {
          b.cutPick = m;
          showQs();
        });
        qs.appendChild(q);
      }
    };
    const preview = el("span", { class: "pv", text: aknanI18n.t("▶ Preview") });
    preview.addEventListener("click", () => playPart(b));
    const go = el("span", { class: "go", text: aknanI18n.t("✂ Download the part") });
    go.addEventListener("click", () => b.cutPick && downloadPart(b, b.cutPick));
    const box = el(
      "div",
      { class: "cut" },
      el("div", { class: "lead", text: aknanI18n.t("Only the part you choose is downloaded") }),
      el("div", { class: "times" }, from.label, from.input, to.label, to.input, to.now, len),
      seg,
      qs,
      el("div", { class: "acts" }, preview, go)
    );
    showLen();
    showSeg();
    showQs();
    return box;
  };
})();
