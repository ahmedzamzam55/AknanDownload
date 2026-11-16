use crate::i18n::{t, tf};
use slint::{ModelRc, SharedString, VecModel};
use swiftget_engine::{filename, AfterDone, Settings, Status};

/// File-type icon for a file name.
pub(crate) fn kind_for(name: &str) -> &'static str {
    match filename::extension(name).as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "ts" | "3gp" | "mpg" | "mpeg" | "vob"
        | "rm" | "rmvb" | "ogv" | "asf" => "video",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "mid" | "mka" => "audio",
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "cab" | "arj" | "lzh" => "archive",
        "iso" | "img" | "dmg" => "disc",
        "pdf" => "pdf",
        "doc" | "docx" | "odt" | "rtf" | "epub" => "doc",
        "xls" | "xlsx" | "csv" | "ods" => "sheet",
        "ppt" | "pptx" | "odp" | "pps" => "slides",
        "txt" | "md" | "log" | "json" | "xml" => "text",
        "exe" | "msi" | "msix" | "appx" | "bat" | "jar" | "deb" | "rpm" => "exe",
        "apk" | "xapk" => "apk",
        "torrent" => "torrent",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "tif" | "tiff" | "heic" | "ico" | "psd" => "image",
        _ => "general",
    }
}

/// Sidebar icon of a category.
pub(super) fn kind_of_category(name: &str) -> &'static str {
    match name {
        "Compressed" => "archive",
        "Documents" => "doc",
        "Music" => "audio",
        "Programs" => "exe",
        "Video" => "video",
        "Images" => "image",
        "General" => "general",
        _ => "folder",
    }
}

/// Display name for a queue (localizes the canonical default queue name only for the UI).
pub(crate) fn queue_title(q: &swiftget_engine::config::QueueDef) -> String {
    if q.id == swiftget_engine::config::MAIN_QUEUE && q.name == "Main download queue" {
        t("Main download queue")
    } else {
        q.name.clone()
    }
}

pub(crate) fn queue_names(s: &Settings) -> ModelRc<SharedString> {
    let names: Vec<SharedString> = s.queues.iter().map(|q| queue_title(q).into()).collect();
    ModelRc::new(VecModel::from(names))
}

pub(crate) fn state_of(s: Status) -> i32 {
    match s {
        Status::Queued => 0,
        Status::Connecting | Status::Downloading | Status::Assembling => 1,
        Status::Paused => 2,
        Status::Completed => 3,
        Status::Failed => 4,
    }
}

/// Order of the progress window's "when it completes" list:
/// nothing, shut down, sleep, exit, hibernate.
pub(super) fn completion_index(a: AfterDone) -> i32 {
    match a {
        AfterDone::Nothing => 0,
        AfterDone::Shutdown => 1,
        AfterDone::Sleep => 2,
        AfterDone::Exit => 3,
        AfterDone::Hibernate => 4,
    }
}

pub(super) fn completion_from(i: i32) -> AfterDone {
    match i {
        1 => AfterDone::Shutdown,
        2 => AfterDone::Sleep,
        3 => AfterDone::Exit,
        4 => AfterDone::Hibernate,
        _ => AfterDone::Nothing,
    }
}

/// Downloads > When all downloads finish (same numbers as the progress window).
pub(super) fn menu_done_from(i: i32) -> AfterDone {
    completion_from(i)
}

pub(crate) fn fmt_time(t: chrono::DateTime<chrono::Utc>) -> String {
    t.with_timezone(&chrono::Local).format("%b %d %H:%M").to_string()
}

/// An error of the engine in words the user understands, in their language.
pub(crate) fn err_text(e: &str) -> String {
    if e.contains("needs a login or an age check") {
        t("This video needs a login or an age check. Open it once in the browser where you are signed in (with the Aknan Download extension), then try again.")
    } else if e.contains("BitLocker") || e.contains("-2144272384") {
        t("The drive of the download folder is locked by BitLocker. Unlock it in Windows, or choose another folder (right-click the download > Properties).")
    } else if e.contains("is already registered") {
        t("This torrent is already running (it may still be sharing its files). Wait a little, or exit Aknan Download and open it again.")
    } else if let Some(rest) = e.split("the link stopped working - getting a new one in ").nth(1) {
        let secs = rest.split(' ').next().unwrap_or("?");
        tf("The link stopped working. A new one is fetched in {} s and checked before the download goes on.", &[&secs])
    } else if let Some(rest) = e.split("the server can't resume, starting again in ").nth(1) {
        let secs = rest.split(' ').next().unwrap_or("?");
        tf("The server had trouble and can't resume this file, so it starts again from the beginning in {} s.", &[&secs])
    } else if e.contains(swiftget_engine::http::CAPTCHA) {
        t("The site asks to confirm you are not a robot. Open the link in the browser, confirm there, then click Resume.")
    } else if e.contains(swiftget_engine::TORRENT_DRY_TEXT) {
        t("Connected to people who send nothing yet (they don't have the missing pieces, or they are busy). Aknan Download keeps looking for others who have the file.")
    } else if e.contains("server answered HTTP 403") || e.contains("server answered HTTP 401") {
        t("The server refused Aknan Download (it lets only the browser in). The link was opened in the browser, which hands it back with the site's sign-in; if the site still refuses, the browser downloads it itself.")
    } else if e.contains("DRM") {
        t("This video is protected by its site (DRM): no download program can save it. It can only be watched on the site.")
    } else if e.contains("nobody shares this torrent right now") {
        t("Nobody shares this torrent right now. It keeps looking and starts by itself as soon as someone does. If it stays like this, the torrent has no sharers left, or your internet provider or firewall blocks torrents.")
    } else if e.contains("ffmpeg.exe is missing") {
        t("ffmpeg.exe (it joins the video and the sound) is missing and could not be downloaded again. Allow it in your antivirus or install Aknan Download again, then click Resume: nothing downloaded is lost.")
    } else if let Some(rest) = e.split("Not enough free space on the disk ").nth(1) {
        // "C:: the file needs 148 GB more, only 83 GB free."
        let (disk, rest) = rest.split_once(": the file needs ").unwrap_or(("", rest));
        let (need, free) = rest.split_once(" more, only ").unwrap_or((rest, ""));
        let free = free.trim_end_matches(" free.");
        tf(
            "Not enough free space on the disk {}: the file needs {} more, only {} is free. Choose a folder on another disk (right-click the download > Move / Rename), or free some space, then click Resume.",
            &[&disk, &need, &free],
        )
    } else if e.contains("the site gave a list of pieces of a video stream") || e.contains("the site gave a web page, not the video") {
        t("The site gave a page instead of the video (it protects its videos, or the link expired), so no broken file was kept. Play the video on its page and use the Aknan Download button that appears on it.")
    } else if e.contains("the downloaded data is gone") {
        t("The downloaded data is gone: its temporary file was removed while it downloaded (an antivirus or a cleaning program?). Click Redownload; if it happens again, allow the download folder in the antivirus.")
    } else if e.contains("ffmpeg could not cut the part") {
        t("The part could not be cut. Try the quick cut, or another quality.")
    } else if e.contains("the part asked for is after the end of the video") {
        t("The part you chose is after the end of the video.")
    } else if e.contains("the new link is not the same file") {
        t("The new link gives another file, so nothing more was written. Right-click the download and use \"Refresh download address\".")
    } else if e.contains("the link has expired or the file was removed") {
        t("The link has expired or the file was removed. Right-click the download and use \"Refresh download address\".")
    } else if e.contains("the downloaded file is damaged: its checksum") {
        t("The downloaded file is damaged: its checksum is not the one it should have. Click Redownload.")
    } else if e.contains(swiftget_engine::http_download::FILE_CHANGED) {
        t("The file on the server changed since the download began, so its parts would not fit together. Click Redownload.")
    } else if e.contains("the link opens one web page after another") {
        t("The link opens one web page after another, not a file. Open it in the browser and download it from there.")
    } else if e.contains("file names that lead out of its folder") {
        t("This torrent has file names that lead out of its folder, so it was stopped to keep your other files safe.")
    } else {
        e.to_string()
    }
}

/// What a download does after its data is all here (its second bar).
pub(super) fn step_text(st: swiftget_engine::Step) -> String {
    use swiftget_engine::Step;
    t(match st {
        Step::Join => "Joining the picture and the sound",
        Step::ToMp4 => "Making the MP4 file",
        Step::ToMp3 => "Making the MP3 file",
        Step::Move => "Moving the file to its folder",
        Step::Cut => "Cutting the part",
        Step::Check => "Checking the file",
        Step::BurnSubs => "Burning subtitles into the video",
    })
}
