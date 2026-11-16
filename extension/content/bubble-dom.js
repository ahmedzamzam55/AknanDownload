// Floating bubble Shadow DOM creation and drag/keyboard/video lifecycle handlers.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  CS.makeBubble = (video) => {
    const { el } = CS;
    const host = document.createElement("div");
    host.style.all = "initial";
    host.style.position = "fixed";
    host.style.zIndex = "2147483647";
    host.style.setProperty("--z", String(CS.zoom));
    host.style.setProperty("--o", String(CS.solid));
    const root = host.attachShadow({ mode: "closed" });
    // early.js keeps the page's own player from taking presses on it
    (globalThis.__aknanRoots = globalThis.__aknanRoots || new WeakMap()).set(host, root);
    const svgNs = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(svgNs, "svg");
    svg.setAttribute("class", "ico");
    svg.setAttribute("viewBox", "0 0 16 16");
    const circle = document.createElementNS(svgNs, "circle");
    for (const [k, v] of [["cx", "8"], ["cy", "8"], ["r", "7.5"], ["fill", "#fff"]]) circle.setAttribute(k, v);
    const arrow = document.createElementNS(svgNs, "path");
    arrow.setAttribute("d", "M6.6 3.5h2.8v4.6h2.3L8 12 4.3 8.1h2.3z");
    arrow.setAttribute("fill", "#1d63d6");
    svg.append(circle, arrow);
    const list = el("div", { class: "list", id: "list" });
    list.hidden = true;
    const spin = el("span", { class: "spin", id: "spin" });
    spin.hidden = true;
    root.append(
      el("style", { text: CS.STYLE }),
      el(
        "div",
        { class: "wrap " + CS.look, dir: aknanI18n.isRtl() ? "rtl" : "ltr" },
        el(
          "div",
          { class: "btn", id: "main" },
          el(
            "span",
            { class: "part", id: "grab", tabindex: "0", role: "button", "aria-haspopup": "true", "aria-label": aknanI18n.t("Download this video") },
            svg,
            spin,
            el("span", { id: "label", text: aknanI18n.t("Download this video") }),
            el("span", { class: "caret", text: "▼" })
          ),
          el("span", { class: "side", id: "help", title: aknanI18n.t("Help"), text: "?", tabindex: "0", role: "button", "aria-label": aknanI18n.t("Help") }),
          el("span", { class: "side", id: "close", title: aknanI18n.t("Hide"), text: "✕", tabindex: "0", role: "button", "aria-label": aknanI18n.t("Hide") })
        ),
        list
      )
    );
    const b = { host, root, video, open: false, own: null, loading: false, dx: null, dy: null, help: false, place: CS.sitePlace, auto: null };
    const grab = root.getElementById("grab");
    // drag: move it anywhere on the video; a short press is a click
    let down = null;
    grab.addEventListener("mousedown", (e) => {
      if (e.button !== 0 || !CS.userPress(e)) return;
      const hr = host.getBoundingClientRect();
      down = { x: e.clientX, y: e.clientY, ox: e.clientX - hr.left, oy: e.clientY - hr.top, moved: false };
      e.preventDefault();
    });
    addEventListener("mousemove", (e) => {
      if (!down) return;
      if (!down.moved && Math.abs(e.clientX - down.x) + Math.abs(e.clientY - down.y) < 5) return;
      down.moved = true;
      root.getElementById("main").classList.add("drag");
      const r = video.getBoundingClientRect();
      b.dx = e.clientX - down.ox - r.left;
      b.dy = e.clientY - down.oy - r.top;
      CS.placeOne(b);
    });
    const up = (e) => {
      if (!down || !CS.userPress(e)) return;
      const wasDrag = down.moved;
      down = null;
      root.getElementById("main").classList.remove("drag");
      if (!wasDrag) return CS.clicked(b);
      // dropped: it goes to the nearest edge of the video, and stays there on
      // this site (its other videos, next time too)
      const r = video.getBoundingClientRect();
      const main = root.getElementById("main").getBoundingClientRect();
      const roomX = Math.max(1, r.width - main.width - 16);
      const roomY = Math.max(1, r.height - main.height - 16);
      const at = { x: Math.min(1, Math.max(0, (b.dx - 8) / roomX)), y: Math.min(1, Math.max(0, (b.dy - 8) / roomY)) };
      b.place = CS.snapToEdge(at);
      b.dx = null;
      b.dy = null;
      CS.keepPlace(b.place);
      CS.placeOne(b);
    };
    addEventListener("mouseup", up);
    // heard here when the page's player keeps it from the window (early.js)
    root.addEventListener("mouseup", up);
    const press = (id, run) => {
      const x = root.getElementById(id);
      x.addEventListener("click", (e) => CS.userPress(e) && run());
      x.addEventListener("keydown", (e) => {
        if ((e.key === "Enter" || e.key === " ") && e.isTrusted) {
          e.preventDefault();
          e.stopPropagation();
          run();
        }
      });
    };
    press("help", () => {
      b.help = !b.help;
      b.open = true;
      CS.renderOne(b);
    });
    press("close", () => {
      b.dismissed = true;
      host.remove();
    });
    // the keyboard: Enter / Space opens it like a click
    grab.addEventListener("keydown", (e) => {
      if ((e.key === "Enter" || e.key === " ") && e.isTrusted) {
        e.preventDefault();
        e.stopPropagation();
        CS.clicked(b);
      }
    });
    // one player for many videos (the previews of a home page, a feed): a
    // new video in it forgets what was found for the one before
    // (only when it really is another video: a site's own player changes
    // its blob: source and empties itself often for the same video)
    video.addEventListener("emptied", () => {
      if (CS.stale(b)) CS.forget(b);
    });
    video.addEventListener("loadstart", () => {
      if (CS.stale(b)) CS.forget(b);
    });
    CS.layer().appendChild(host);
    CS.bubbles.set(video, b);
    CS.renderOne(b);
    CS.placeOne(b);
    return b;
  };
})();
