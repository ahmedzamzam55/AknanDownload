// Shadow DOM stylesheet for the floating video download bubble and panel.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  CS.STYLE = `
    :host { all: initial; }
    .wrap { font: 11px/1.25 "Segoe UI", system-ui, sans-serif; color: #1f2733; display: inline-block; zoom: var(--z, 1); position: relative; }
    .btn { display: inline-flex; align-items: stretch; color: #fff; border: 1px solid #0f4fb5;
           background: linear-gradient(#3f8cf5, #1d63d6); border-radius: 3px;
           box-shadow: 0 1px 6px rgba(0,0,0,.35); font: inherit; font-weight: 600;
           opacity: var(--o, .9); transition: opacity .15s; user-select: none; white-space: nowrap; }
    .btn:hover, .btn.open { opacity: 1; }
    .btn.drag { opacity: 1; }
    .part { display: inline-flex; align-items: center; gap: 5px; padding: 3px 7px; cursor: default; }
    .part:hover { background: rgba(255,255,255,.12); }
    .btn.drag .part { cursor: grabbing; }
    .side { padding: 3px 6px; cursor: default; border-left: 1px solid rgba(255,255,255,.28); display: inline-flex; align-items: center; }
    .side:hover { background: rgba(255,255,255,.18); }
    .ico { width: 13px; height: 13px; flex: none; }
    .caret { font-size: 9px; opacity: .9; }
    .spin { width: 9px; height: 9px; border: 2px solid rgba(255,255,255,.4); border-top-color: #fff; border-radius: 50%;
            animation: r .8s linear infinite; flex: none; }
    @keyframes r { to { transform: rotate(360deg); } }
    /* the list floats under the button (the button never moves): to its
       left when the button is on the right of the video, else to its right,
       and above it when there is no room below */
    .list { position: absolute; top: 100%; right: 0; width: 330px; margin-top: 4px; background: #fff; border-radius: 6px;
            box-shadow: 0 6px 22px rgba(0,0,0,.28); overflow: hidden; max-height: 360px; overflow-y: auto; border: 1px solid #d5dbe4; }
    .wrap.toright .list { right: auto; left: 0; }
    .wrap.up .list { top: auto; bottom: 100%; margin-top: 0; margin-bottom: 4px; }
    .btn.locked { background: linear-gradient(#7b8794, #5b6573); border-color: #4a535f; }
    .head { padding: 7px 10px; background: #f4f6fa; border-bottom: 1px solid #e3e8ef; font-weight: 600; color: #334;
            overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .item { padding: 6px 10px; cursor: default; border-bottom: 1px solid #eef1f5; display: flex; gap: 6px; }
    .item:last-child { border-bottom: 0; }
    .item:hover { background: #e8f0fe; }
    .num { color: #6b7686; min-width: 16px; text-align: right; }
    .body { flex: 1; min-width: 0; }
    .t { font-weight: 600; display: flex; align-items: center; gap: 5px; flex-wrap: wrap; }
    .s { color: #6b7686; font-size: 10.5px; overflow-wrap: anywhere; }
    .tag { font-size: 9.5px; font-weight: 700; padding: 0 4px; border-radius: 3px; color: #fff; background: #7b8794; line-height: 14px; }
    .tag.HD { background: #1d8f5a; } .tag.FullHD { background: #1d63d6; } .tag.K2 { background: #7b3fd6; }
    .tag.K4 { background: #d6631d; } .tag.K8 { background: #c0262d; }
    .codec { font-size: 9.5px; color: #4a5566; border: 1px solid #c9d1dc; border-radius: 3px; padding: 0 3px; line-height: 13px; }
    .all { font-weight: 700; color: #1d63d6; }
    .msg { padding: 8px 10px; color: #1a9e5a; }
    .help { padding: 8px 10px; color: #334; background: #fffbe6; border-bottom: 1px solid #f0e2a8; }
    .act { margin-top: 6px; display: inline-block; background: #1d63d6; color: #fff; border-radius: 4px; padding: 3px 9px; cursor: default; }
    .act:hover { background: #1a56b8; }
    .off { cursor: default; opacity: .7; }
    .off:hover { background: #fff; }
    .err { color: #d1342f; }
    /* "Download a part" */
    .clip { padding: 8px 10px; border-bottom: 1px solid #e3e8ef; background: #fafbfd; }
    .clip .row { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
    .clip .row b { min-width: 34px; font-weight: 600; }
    .clip input[type=text] { width: 76px; font: inherit; padding: 3px 5px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; background: #fff; }
    .clip input.bad { border-color: #d1342f; }
    .clip .now, .clip .btn2 { background: #e8f0fe; color: #1d63d6; border-radius: 4px; padding: 3px 8px; cursor: default; white-space: nowrap; }
    .clip .now:hover, .clip .btn2:hover { background: #d6e4fd; }
    .clip .len { color: #334; margin-bottom: 6px; }
    .clip label { display: flex; gap: 6px; align-items: flex-start; margin: 4px 0; cursor: default; }
    .clip label input { margin: 2px 0 0; flex: none; }
    .clip label small { display: block; color: #6b7686; font-size: 10.5px; }
    .clip .acts { display: flex; gap: 6px; margin-top: 6px; }
    .clip .pick { margin-top: 8px; font-weight: 600; color: #334; }
    .cliprow .t { color: #1d63d6; }
    /* "⬇ Whole video" / "✂ Part" (the part in orange) */
    .tabs { display: flex; border-bottom: 1px solid #e3e8ef; background: #f4f6fa; }
    .tab { flex: 1; text-align: center; padding: 7px 4px; cursor: default; color: #5b6575; font-weight: 600; border-bottom: 3px solid transparent; }
    .tab:hover { background: #eef2f8; }
    .tab.on { color: #1d63d6; border-bottom-color: #1d63d6; background: #fff; }
    .tab.part.on { color: #e8590c; border-bottom-color: #e8590c; }
    .cut { padding: 9px 12px 11px; background: #fff; }
    .cut .lead { color: #e8590c; font-weight: 700; margin-bottom: 8px; }
    .cut .times { display: flex; align-items: center; gap: 6px; margin-bottom: 8px; }
    .cut .times b { font-weight: 700; color: #1f2733; }
    .cut input[type=text] { width: 58px; font: inherit; text-align: center; padding: 3px 4px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; background: #fff; }
    .cut input.bad { border-color: #d1342f; }
    .cut .now { color: #e8590c; cursor: default; white-space: nowrap; font-size: 10.5px; }
    .cut .now:hover { text-decoration: underline; }
    .cut .len { margin-left: auto; font-weight: 700; color: #1f2733; }
    .cut .len.err { color: #d1342f; }
    .cut .seg { display: inline-flex; border: 1px solid #e8590c; border-radius: 4px; overflow: hidden; margin-bottom: 8px; }
    .cut .seg span { padding: 3px 10px; color: #e8590c; cursor: default; background: #fff; }
    .cut .seg span + span { border-left: 1px solid #e8590c; }
    .cut .seg span.on { background: #e8590c; color: #fff; }
    .cut .qs { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
    .cut .qs span { padding: 3px 9px; border: 1px solid #c9d1dc; border-radius: 4px; color: #1f2733; cursor: default; background: #fff; font-weight: 600; }
    .cut .qs span:hover { border-color: #e8590c; }
    .cut .qs span.on { border-color: #e8590c; background: #fff3eb; color: #e8590c; }
    .cut .acts { display: flex; gap: 8px; }
    .cut .pv { padding: 6px 10px; border-radius: 5px; background: #fff3eb; color: #e8590c; cursor: default; white-space: nowrap; }
    .cut .pv:hover { background: #ffe4d2; }
    .cut .go { flex: 1; text-align: center; padding: 6px 10px; border-radius: 5px; color: #fff; font-weight: 700; cursor: default;
               background: linear-gradient(#f57a2e, #e05a0c); box-shadow: 0 2px 6px rgba(224,90,12,.35); }
    .cut .go:hover { background: linear-gradient(#ff8a3d, #e8630f); }
    .wrap[dir="rtl"] .side { border-left: 0; border-right: 1px solid rgba(255,255,255,.28); }
    .wrap[dir="rtl"].light .side { border-right-color: #c9d3df; }
    .wrap[dir="rtl"].compact #close { border-right-color: rgba(255,255,255,.3); }
    .wrap[dir="rtl"] .num { text-align: left; }
    .wrap[dir="rtl"] .cut .len { margin-left: 0; margin-right: auto; }
    .wrap[dir="rtl"] .cut .seg span + span { border-left: 0; border-right: 1px solid #e8590c; }
    /* looks */
    .dark .btn { background: linear-gradient(#3a3f47, #23272e); border-color: #15181c; }
    .dark .btn:hover, .dark .btn.open { background: linear-gradient(#454b55, #2a2f37); }
    .dark .ico path { fill: #23272e; }
    .light .btn { background: linear-gradient(#ffffff, #e9eef5); border-color: #b8c3d1; color: #1f2733; }
    .light .btn:hover, .light .btn.open { background: linear-gradient(#ffffff, #dfe7f1); }
    .light .ico circle { fill: #1d63d6; }
    .light .ico path { fill: #fff; }
    .light .side { border-left-color: #c9d3df; }
    .light .spin { border-color: rgba(29,99,214,.3); border-top-color: #1d63d6; }
    .green .btn { background: linear-gradient(#35b36b, #1f8a4c); border-color: #176b3a; }
    .green .btn:hover, .green .btn.open { background: linear-gradient(#3cc276, #1d8048); }
    .green .ico path { fill: #1f8a4c; }
    .compact .btn { border-radius: 7px; background: linear-gradient(#2f72dc, #1e56b9); border-color: #163f8a; }
    .compact .btn:hover, .compact .btn.open { background: linear-gradient(#3a7deb, #1f5cc6); }
    .compact #label, .compact .caret, .compact #help { display: none; }
    .compact .part { padding: 4px 7px; }
    .compact .ico { width: 18px; height: 18px; }
    .compact .ico circle { fill: #e4e6ea; }
    .compact .ico path { fill: #1e56b9; }
    .compact #close { padding: 4px 8px; font-size: 13px; border-left-color: rgba(255,255,255,.3); }
    .icon .btn { border-radius: 50%; }
    .icon .part { padding: 5px; }
    .icon #label, .icon .caret, .icon .side { display: none; }
    .icon .ico { width: 16px; height: 16px; }`;
})();
