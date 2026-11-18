use super::Manager;
use crate::sync::LockOk;
use crate::http::{self, Progress, SendOrdered};
use crate::model::{DownloadId, DownloadRecord, Step};
use crate::mux;
use crate::subs::convert_subtitles;
use crate::{filename, m3u8, ytdlp};
use anyhow::{anyhow, bail};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

async fn fetch_text(client: &reqwest::Client, url: &str, headers: &crate::model::RequestHeaders, ua: &str) -> anyhow::Result<String> {
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        http::request(client, url, headers, ua).send_ordered(),
    )
    .await
    .map_err(|_| anyhow!("subtitle download timed out"))??;
    if !resp.status().is_success() {
        bail!("HTTP {}", resp.status().as_u16());
    }
    let bytes = resp.bytes().await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn download_sub_raw(mgr: &Manager, rec: &DownloadRecord, url: &str, cookies: Option<&str>) -> anyhow::Result<String> {
    let ua = rec.headers.user_agent.clone().unwrap_or_else(|| mgr.settings().user_agent);
    let mut headers = rec.headers.clone();
    if let Some(c) = cookies.filter(|s| !s.is_empty()) {
        headers.cookies = Some(c.to_string());
    }
    if headers.referer.is_none() {
        headers.referer = rec.source_page.clone();
    }
    let client = http::client_for(&mgr.shared.client(), url);
    let text = fetch_text(&client, url, &headers, &ua).await?;
    if !text.trim_start().starts_with("#EXTM3U") {
        return Ok(text);
    }
    let pl = m3u8::parse_media(url, &text)?;
    let mut joined = String::new();
    for p in pl.parts {
        if let Ok(seg) = fetch_text(&client, &p.url, &headers, &ua).await {
            joined.push_str(&seg);
            joined.push_str("\n\n");
        }
    }
    Ok(joined)
}

async fn fetch_srt_for(mgr: &Arc<Manager>, rec: &DownloadRecord, sub: &crate::model::SubtitleTask) -> String {
    if let Some(raw) = sub.body.as_deref().filter(|s| !s.trim().is_empty()) {
        let srt = convert_subtitles(raw, "srt");
        if !srt.trim().is_empty() {
            return srt;
        }
    }
    if let Some(u) = sub.url.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(raw) = download_sub_raw(mgr, rec, u, sub.cookies.as_deref()).await {
            let srt = convert_subtitles(&raw, "srt");
            if !srt.trim().is_empty() {
                return srt;
            }
        }
    }
    let Some(page) = rec.source_page.clone() else { return String::new() };
    let lang = sub.lang.clone();
    let dir = std::env::temp_dir().join(format!("aknan-sub-{}", rec.id));
    let _ = std::fs::create_dir_all(&dir);
    let _ = tokio::task::spawn_blocking({
        let (page, dir, lang) = (page, dir.clone(), lang);
        move || ytdlp::fetch_subtitles(&page, &dir, "sub", &lang)
    })
    .await;
    let found = std::fs::read_dir(&dir)
        .ok()
        .and_then(|rd| rd.flatten().find(|e| e.path().is_file()).map(|e| e.path()));
    let srt = found
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|raw| convert_subtitles(&raw, "srt"))
        .unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    srt
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
        let srt = fetch_srt_for(self, rec, &sub).await;
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
        let srt = fetch_srt_for(self, &rec, &sub).await;
        if srt.trim().is_empty() {
            return;
        }
        let (stem, _) = filename::split_ext(&rec.file_name);
        let safe_lang = filename::sanitize(&sub.lang);
        let lang_part = if safe_lang.is_empty() { "sub" } else { safe_lang.as_str() };
        let name = filename::unique_name(&rec.save_dir, &format!("{stem}.{lang_part}.srt"));
        let _ = std::fs::create_dir_all(&rec.save_dir);
        let _ = std::fs::write(rec.save_dir.join(name), srt.as_bytes());
    }
}
