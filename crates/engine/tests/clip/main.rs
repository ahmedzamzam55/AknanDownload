//! "Download a part of a video": only the part is read from the site, and
//! the part is cut where asked (quick and exact). Needs a full ffmpeg on
//! the computer that runs the tests (skipped without one).

mod cut;
mod helpers;
mod stream_and_av;
