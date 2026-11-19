use super::sub_retry::{fetch_sub_with_retry, fetch_ytdlp_sub_retry};
use super::Manager;
use crate::http::{self, Progress};
use crate::model::{DownloadId, DownloadRecord, Step, SubtitleTask};
use crate::subs::convert_subtitles;
use crate::sync::LockOk;
use crate::{filename, m3u8, mux};
use anyhow::bail;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

async fn download_sub_raw(
    mgr: &Manager,
    rec: &DownloadRecord,
    sub: &SubtitleTask,
    url: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<String> {
    let ua = rec.headers.user_agent.clone().unwrap_or_else(|| mgr.settings().user_agent);
    let mut headers = rec.headers.clone();
    if let Some(c) = sub.cookies.as_deref().filter(|s| !s.is_empty()) {
        headers.cookies = Some(c.to_string());
    }
    if headers.referer.is_none() {
        headers.referer = rec.source_page.clone();
    }
    for (k, v) in &sub.extra_headers {
        if !k.is_empty() && !v.is_empty() && !headers.extra.iter().any(|(ek, _)| ek.eq_ignore_ascii_case(k)) {
            headers.extra.push((k.clone(), v.clone()));
        }
    }
    let client = http::client_for(&mgr.shared.client(), url);
    let text = fetch_sub_with_retry(&client, url, &headers, &ua, cancel).await?;
    if !text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U") {
        return Ok(text);
    }
    let pl = m3u8::parse_media(url, &text)?;
    let mut joined = String::new();
    for p in pl.parts {
        if let Ok(seg) = fetch_sub_with_retry(&client, &p.url, &headers, &ua, cancel).await {
            joined.push_str(&seg);
            joined.push_str("\n\n");
        }
    }
    Ok(joined)
}

async fn fetch_formatted_sub(
    mgr: &Arc<Manager>,
    rec: &DownloadRecord,
    sub: &SubtitleTask,
    target_fmt: &str,
    cancel: &CancellationToken,
) -> String {
    if let Some(raw) = sub.body.as_deref().filter(|s| !s.trim().is_empty()) {
        let converted = convert_subtitles(raw, target_fmt);
        if !converted.trim().is_empty() {
            return converted;
        }
    }
    if let Some(u) = sub.url.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(raw) = download_sub_raw(mgr, rec, sub, u, cancel).await {
            let converted = convert_subtitles(&raw, target_fmt);
            if !converted.trim().is_empty() {
                return converted;
            }
        }
    }
    let Some(page) = rec.source_page.clone() else { return String::new() };
    let dir = std::env::temp_dir().join(format!("aknan-sub-{}", rec.id));
    let _ = std::fs::create_dir_all(&dir);
    let _ = fetch_ytdlp_sub_retry(page, &dir, "sub".into(), sub.lang.clone(), Some(cancel)).await;
    let found = std::fs::read_dir(&dir)
        .ok()
        .and_then(|rd| rd.flatten().find(|e| e.path().is_file()).map(|e| e.path()));
    let out = found
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|raw| convert_subtitles(&raw, target_fmt))
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

impl Manager {
    pub(super) fn apply_sub_convert(&self, id: DownloadId, part: &Path, fmt: &str, progress: &Arc<Progress>) -> anyhow::Result<()> {
        let bytes = std::fs::read(part)?;
        let out = convert_subtitles(&String::from_utf8_lossy(&bytes), fmt);
        std::fs::write(part, out.as_bytes())?;
        let len = out.len() as u64;
        progress.bytes.store(len, Ordering::Relaxed);
        self.update(id, move |x| x.total_size = Some(len));
        Ok(())
    }

    pub(super) async fn process_video_subtitle(
        self: &Arc<Self>,
        id: DownloadId,
        rec: &DownloadRecord,
        progress: &Arc<Progress>,
        cancel: &CancellationToken,
        part: &Path,
    ) -> anyhow::Result<()> {
        let Some(sub) = rec.subtitle.clone() else { return Ok(()) };
        if !sub.burn {
            return Ok(());
        }
        let srt = fetch_formatted_sub(self, rec, &sub, "srt", cancel).await;
        if srt.trim().is_empty() {
            bail!("could not fetch subtitle track to burn into the video");
        }
        progress.assembling.store(true, Ordering::Relaxed);
        crate::tools::restore_ffmpeg(&self.client()).await;
        progress.set_step(Some(Step::BurnSubs));
        let tmp = part.with_extension("burntmp");
        let (in_p, out_p, final_name) = (part.to_path_buf(), tmp.clone(), rec.file_name.clone());
        let (p2, c2) = (progress.clone(), cancel.clone());
        let r = tokio::task::spawn_blocking(move || mux::burn_subtitles(&in_p, &srt, &out_p, &final_name, &|f| p2.step_at(f), Some(&c2))).await?;
        if let Err(e) = r {
            progress.set_step(None);
            return Err(e);
        }
        std::fs::rename(&tmp, part)?;
        if let Ok(m) = std::fs::metadata(part) {
            let len = m.len();
            progress.bytes.store(len, Ordering::Relaxed);
            self.update(id, move |x| x.total_size = Some(len));
        }
        Ok(())
    }

    pub(super) async fn save_video_sidecar_subtitle(self: &Arc<Self>, id: DownloadId, fallback: &DownloadRecord) {
        let rec = self
            .inner
            .lock_ok()
            .entries
            .get(&id)
            .map(|e| e.rec.clone())
            .unwrap_or_else(|| fallback.clone());
        let Some(sub) = rec.subtitle.clone() else { return };
        if sub.burn {
            return;
        }
        let fmt = sub
            .format
            .as_deref()
            .filter(|f| matches!(*f, "srt" | "vtt" | "txt"))
            .unwrap_or("srt");
        let cancel = CancellationToken::new();
        let text = fetch_formatted_sub(self, &rec, &sub, fmt, &cancel).await;
        if text.trim().is_empty() {
            return;
        }
        let (stem, _) = filename::split_ext(&rec.file_name);
        let safe_lang = filename::sanitize(&sub.lang);
        let lang_part = if safe_lang.is_empty() { "sub" } else { safe_lang.as_str() };
        let name = filename::unique_name(&rec.save_dir, &format!("{stem}.{lang_part}.{fmt}"));
        let _ = std::fs::create_dir_all(&rec.save_dir);
        let _ = std::fs::write(rec.save_dir.join(name), text.as_bytes());
    }
}
