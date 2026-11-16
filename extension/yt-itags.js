// YouTube stream itag table and transient player request parameters.
const YT_ITAGS = {
  // video + audio in one file (old progressive formats)
  18: { kind: "av", h: 360, ext: "mp4" },
  22: { kind: "av", h: 720, ext: "mp4" },
  // H.264 (mp4)
  160: { kind: "v", h: 144, ext: "mp4" },
  133: { kind: "v", h: 240, ext: "mp4" },
  134: { kind: "v", h: 360, ext: "mp4" },
  135: { kind: "v", h: 480, ext: "mp4" },
  136: { kind: "v", h: 720, ext: "mp4" },
  137: { kind: "v", h: 1080, ext: "mp4" },
  298: { kind: "v", h: 720, fps: 60, ext: "mp4" },
  299: { kind: "v", h: 1080, fps: 60, ext: "mp4" },
  264: { kind: "v", h: 1440, ext: "mp4" },
  266: { kind: "v", h: 2160, ext: "mp4" },
  // AV1 (mp4)
  394: { kind: "v", h: 144, ext: "mp4", codec: "AV1" },
  395: { kind: "v", h: 240, ext: "mp4", codec: "AV1" },
  396: { kind: "v", h: 360, ext: "mp4", codec: "AV1" },
  397: { kind: "v", h: 480, ext: "mp4", codec: "AV1" },
  398: { kind: "v", h: 720, ext: "mp4", codec: "AV1" },
  399: { kind: "v", h: 1080, ext: "mp4", codec: "AV1" },
  400: { kind: "v", h: 1440, ext: "mp4", codec: "AV1" },
  401: { kind: "v", h: 2160, ext: "mp4", codec: "AV1" },
  // VP9 (webm)
  278: { kind: "v", h: 144, ext: "webm" },
  242: { kind: "v", h: 240, ext: "webm" },
  243: { kind: "v", h: 360, ext: "webm" },
  244: { kind: "v", h: 480, ext: "webm" },
  247: { kind: "v", h: 720, ext: "webm" },
  248: { kind: "v", h: 1080, ext: "webm" },
  271: { kind: "v", h: 1440, ext: "webm" },
  313: { kind: "v", h: 2160, ext: "webm" },
  302: { kind: "v", h: 720, fps: 60, ext: "webm" },
  303: { kind: "v", h: 1080, fps: 60, ext: "webm" },
  308: { kind: "v", h: 1440, fps: 60, ext: "webm" },
  315: { kind: "v", h: 2160, fps: 60, ext: "webm" },
  // audio only
  139: { kind: "a", abr: 48, ext: "m4a" },
  140: { kind: "a", abr: 128, ext: "m4a" },
  141: { kind: "a", abr: 256, ext: "m4a" },
  249: { kind: "a", abr: 50, ext: "webm" },
  250: { kind: "a", abr: 70, ext: "webm" },
  251: { kind: "a", abr: 160, ext: "webm" },
};

// Parameters that only describe the player's current small request.
const PIECE_PARAMS = ["range", "rn", "rbuf", "ump", "srfvp", "alr"];

if (typeof globalThis !== "undefined") {
  globalThis.YT_ITAGS = YT_ITAGS;
  globalThis.PIECE_PARAMS = PIECE_PARAMS;
}
if (typeof module !== "undefined" && module.exports) {
  module.exports = { YT_ITAGS, PIECE_PARAMS };
}
