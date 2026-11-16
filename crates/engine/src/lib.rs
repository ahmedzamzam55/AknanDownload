//! Aknan Download engine.
//!
//! * `segments`       – splitting a file between connections (dynamic segmentation)
//! * `http_download`  – multi-connection downloader with resume and retries
//! * `hls`            – .m3u8 video streams (AES-128 supported, DRM refused)
//! * `manager`        – queue, scheduler, speed meter, persistence
//!
//! The UI only needs [`Manager`].

pub mod domain;
pub mod infra;
pub mod protocols;
pub mod media;
pub mod services;

pub use domain::{category, ef2, filename, model, segments};
pub use infra::{checksum, config, fileio, limiter, secret, store, sync, system};
pub use protocols::{bt, ftp, hls, http, http_download, m3u8, torrent};
pub use media::{clip, clipserve, mux, ytdlp};
pub use services::{grabber, manager, selfupdate, tools};

pub use config::{AfterDone, Schedule, Settings};
pub use manager::{default_data_dir, ClipSource, DetailsEdit, Event, Manager, PreviewSource, TORRENT_DRY_TEXT};
pub use model::*;
