// "💬 With subtitles" and "📝 Subtitles" tabs inside the floating download panel.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  function keepKeys(el) {
    for (const t of ["keydown", "keyup", "keypress"]) el.addEventListener(t, (e) => e.stopPropagation());
  }

  function langTitle(s) {
    const base = aknanI18n.t(s.name || s.lang || "");
    return s.lang ? `${base} (${s.lang})` : base;
  }

  function fmtCueVtt(sec) {
    const ms = Math.max(0, Math.round(Number(sec || 0) * 1000));
    const p = (n, w = 2) => String(n).padStart(w, "0");
    return `${p(Math.floor(ms / 3600000))}:${p(Math.floor((ms % 3600000) / 60000))}:${p(Math.floor((ms % 60000) / 1000))}.${p(ms % 1000, 3)}`;
  }

  function domTextTracks(video) {
    const out = [], tt = video && video.textTracks;
    if (!tt) return out;
    for (let i = 0; i < tt.length; i += 1) {
      const tr = tt[i];
      if (!tr || (tr.kind !== "subtitles" && tr.kind !== "captions")) continue;
      if (!tr.cues || !tr.cues.length) continue;
      const lines = ["WEBVTT", ""];
      for (let j = 0; j < tr.cues.length; j += 1) {
        const c = tr.cues[j];
        if (c && c.text) lines.push(`${fmtCueVtt(c.startTime)} --> ${fmtCueVtt(c.endTime)}`, String(c.text).trim(), "");
      }
      if (lines.length <= 2) continue;
      const lang = tr.language || "und";
      out.push({ lang, name: tr.label || lang, url: `texttrack:${i}:${lang}`, body: lines.join("\n"), ext: "vtt", auto: false });
    }
    return out;
  }

  function trackKey(s) {
    const low = String(s.lang || "und").toLowerCase();
    const v = s.variant || (low === "und" ? s.name || s.url : "");
    return `${low}|${s.auto ? 1 : 0}|${String(v).toLowerCase()}`;
  }

  CS.subsOf = (b, list) => {
    const out = [], seen = new Set();
    const push = (s) => {
      if (!s || !s.lang) return;
      if (!s.body && !/^(https?:|data:|texttrack:)/i.test(s.url || "")) return;
      const k = trackKey(s);
      if (seen.has(k)) return;
      seen.add(k);
      out.push({ ...s, _key: k });
    };
    for (const m of list || []) if (Array.isArray(m.subs)) for (const s of m.subs) push(s);
    const tracks = b && b.video && b.video.querySelectorAll ? b.video.querySelectorAll("track") : [];
    for (const tr of tracks) {
      const kind = (tr.getAttribute("kind") || "subtitles").toLowerCase();
      if (kind !== "subtitles" && kind !== "captions") continue;
      let url = tr.getAttribute("src") || "";
      try { url = new URL(url, location.href).toString(); } catch { continue; }
      const lang = tr.getAttribute("srclang") || "und";
      push({ lang, name: tr.getAttribute("label") || lang, url, ext: "vtt", auto: false });
    }
    if (b && b.video) for (const s of domTextTracks(b.video)) push(s);
    return out;
  };

  function pickDefaultTrack(b, subs) {
    if (b.subLang) {
      const byKey = subs.find((s) => s._key === b.subLang || s.lang === b.subLang);
      if (byKey) return byKey._key;
    }
    const pref = aknanI18n.isRtl() ? "ar" : "en";
    const match =
      subs.find((s) => !s.auto && s.lang.toLowerCase().startsWith(pref)) ||
      subs.find((s) => !s.auto) ||
      subs.find((s) => s.lang.toLowerCase().startsWith("en")) ||
      subs[0];
    b.subLang = match ? match._key : "";
    return b.subLang;
  }

  CS.withSubsPanel = (b, usable, subs) => {
    const { el } = CS;
    const frag = document.createDocumentFragment();
    const chosenKey = pickDefaultTrack(b, subs);
    const picked = subs.find((s) => s._key === chosenKey) || subs[0];

    const cfg = el("div", { class: "subcfg" });
    const lrow = el("div", { class: "subselrow" }, el("span", { class: "lbl", text: aknanI18n.t("Subtitle language") }));
    const sel = el("select", { class: "sel" });
    keepKeys(sel);
    for (const s of subs) {
      const label = s.auto ? `${langTitle(s)} · ${aknanI18n.t("auto")}` : langTitle(s);
      const opt = el("option", { value: s._key, text: label });
      if (s._key === picked._key) opt.selected = true;
      sel.appendChild(opt);
    }
    sel.addEventListener("change", () => { b.subLang = sel.value; CS.renderOne(b); });
    lrow.appendChild(sel);
    cfg.appendChild(lrow);

    const mode = el("div", { class: "submode" });
    const radio = (burn, text) => {
      const inp = el("input", { type: "radio", name: "aknan-sub-mode" });
      if (!!b.subBurn === burn) inp.checked = true;
      inp.addEventListener("change", () => { b.subBurn = burn; CS.renderOne(b); });
      return el("label", {}, inp, el("span", { text }));
    };
    mode.append(radio(false, aknanI18n.t("Separate file (.srt)")), radio(true, aknanI18n.t("Burned into the picture")));
    cfg.appendChild(mode);
    frag.appendChild(cfg);

    const videoOpts = usable.filter((m) => !(m.yt && (m.yt.quality === "audio" || m.yt.quality === "mp3" || /^audio\|/.test(m.yt.key || ""))));
    const suffix = aknanI18n.t("+ {} subtitles", aknanI18n.t(picked.name || picked.lang));
    videoOpts.forEach((m, idx) => {
      const d = CS.describe(m, b, suffix);
      const t = el("div", { class: "t" }, el("span", { text: d.main }));
      if (d.tag) t.append(el("span", { class: "tag " + d.tag.replace(/\s/g, "").replace(/^(\d)K$/, "K$1"), text: d.tag }));
      if (d.codec) t.append(el("span", { class: "codec", text: d.codec }));
      const sub = d.sub.length > 115 ? d.sub.slice(0, 115) + "…" : d.sub;
      const row = el("div", { class: "item", tabindex: "0", role: "button" }, el("span", { class: "num", text: `${idx + 1}.` }), el("div", { class: "body" }, t, el("div", { class: "s", text: sub })));
      const run = () => CS.download(b, m, { lang: picked.lang, url: picked.url, body: picked.body || undefined, reqHeaders: picked.reqHeaders || undefined, burn: !!b.subBurn });
      row.addEventListener("click", run);
      row.addEventListener("keydown", (e) => e.key === "Enter" && e.isTrusted && (e.preventDefault(), run()));
      frag.appendChild(row);
    });
    return frag;
  };

  CS.subsOnlyPanel = (b, subs, site) => {
    const { el } = CS;
    const frag = document.createDocumentFragment();
    if (!b.subChecked) b.subChecked = new Set(subs.map((s) => s._key));
    if (!b.subFmt) b.subFmt = "srt";

    const checkedCount = subs.filter((s) => b.subChecked.has(s._key)).length;
    const allBox = el("input", { type: "checkbox" });
    allBox.checked = checkedCount === subs.length;
    allBox.indeterminate = checkedCount > 0 && checkedCount < subs.length;
    allBox.addEventListener("change", () => { b.subChecked = allBox.checked ? new Set(subs.map((s) => s._key)) : new Set(); CS.renderOne(b); });

    const fmtSel = el("select", { class: "sel sm" });
    keepKeys(fmtSel);
    for (const f of ["srt", "vtt", "txt"]) {
      const opt = el("option", { value: f, text: f.toUpperCase() });
      if (b.subFmt === f) opt.selected = true;
      fmtSel.appendChild(opt);
    }
    fmtSel.addEventListener("change", () => { b.subFmt = fmtSel.value; CS.renderOne(b); });

    frag.appendChild(el("div", { class: "subtop" }, el("label", { class: "suball" }, allBox, el("b", { text: aknanI18n.t("All subtitles ({})", subs.length) })), el("label", { class: "subfmt" }, el("span", { text: aknanI18n.t("Format") }), fmtSel)));

    const visible = !b.subsAll && subs.length > 5 ? subs.slice(0, 5) : subs;
    for (const s of visible) {
      const box = el("input", { type: "checkbox" });
      box.checked = b.subChecked.has(s._key);
      box.addEventListener("change", () => { if (box.checked) b.subChecked.add(s._key); else b.subChecked.delete(s._key); CS.renderOne(b); });
      frag.appendChild(el("label", { class: "subrow" }, box, el("b", { text: aknanI18n.t(s.name || s.lang) }), el("span", { class: "code", text: `(${s.lang})` }), s.auto ? el("span", { class: "autobadge", text: aknanI18n.t("auto") }) : null));
    }
    if (!b.subsAll && subs.length > 5) {
      const more = el("div", { class: "submore", text: aknanI18n.t("… {} more", subs.length - 5) });
      more.addEventListener("click", () => { b.subsAll = true; CS.renderOne(b); });
      frag.appendChild(more);
    }

    const go = el("button", { class: "subbtn" + (checkedCount ? "" : " off"), type: "button", text: checkedCount === 1 ? aknanI18n.t("⬇ Download 1 subtitle file") : aknanI18n.t("⬇ Download {} subtitle files", checkedCount) });
    go.disabled = !checkedCount;
    go.addEventListener("click", () => {
      const tracks = subs.filter((s) => b.subChecked.has(s._key)).map((s) => ({ lang: s.lang, url: s.url, body: s.body || undefined, reqHeaders: s.reqHeaders || undefined }));
      if (!tracks.length) return;
      chrome.runtime.sendMessage(
        { type: "swiftget-download-subs", tracks, format: b.subFmt, title: document.title, referrer: location.href },
        (res) => {
          if (chrome.runtime.lastError || !res || !res.ok) CS.notRunning(b, () => go.click());
          else CS.flashOne(b, aknanI18n.t("Subtitles sent to Aknan Download ✔"), false);
        }
      );
    });
    const foot = el("div", { class: "subfoot" }, go);
    if (subs.some((s) => s.auto)) {
      const yt = /youtube/i.test(site || "") || /youtube(kids)?\.com/i.test(location.hostname);
      foot.appendChild(el("div", { class: "subnote", text: aknanI18n.t(yt ? '"auto" = made by YouTube automatically (not by the video\'s owner)' : '"auto" = made automatically (not by the video\'s owner)') }));
    }
    frag.appendChild(foot);
    return frag;
  };
})();
