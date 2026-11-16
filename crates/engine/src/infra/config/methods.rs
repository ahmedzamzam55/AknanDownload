use super::{default_column_width, QueueDef, Settings, ALL_COLUMNS};
use std::path::{Path, PathBuf};

impl Settings {
    /// Category of a file: by site first, then by extension; "General" otherwise.
    pub fn category_of(&self, file_name: &str, url: &str) -> String {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .unwrap_or_default();
        if !host.is_empty() {
            for c in &self.categories {
                if c.sites.iter().any(|s| host_matches(&host, s)) {
                    return c.name.clone();
                }
            }
        }
        let ext = crate::filename::extension(file_name);
        if !ext.is_empty() {
            for c in &self.categories {
                if c.exts.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                    return c.name.clone();
                }
            }
        }
        "General".into()
    }

    /// Names of every category, General last.
    pub fn category_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.categories.iter().map(|c| c.name.clone()).collect();
        v.push("General".into());
        v
    }

    /// Connections allowed for this address (server exceptions).
    pub fn connections_for(&self, url: &str, wanted: u32) -> u32 {
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
            .unwrap_or_default();
        let set = self
            .server_limits
            .iter()
            .find(|l| host_matches(&host, &l.host))
            .map(|l| wanted.min(l.connections.max(1)))
            .unwrap_or(wanted);
        // a site that said "too many requests" before: as many as it took
        crate::http_download::learned_limit(url).map(|n| set.min(n)).unwrap_or(set)
    }

    /// Saved login for this address (Options > Site Logins).
    pub fn login_for(&self, url: &str) -> Option<(String, String)> {
        let u = url::Url::parse(url).ok()?;
        let host = u.host_str()?.to_ascii_lowercase();
        let path = u.path().to_string();
        self.logins
            .iter()
            .filter(|l| !l.user.is_empty())
            .find(|l| {
                let site = l.site.trim().trim_start_matches("http://").trim_start_matches("https://");
                let (h, p) = site.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((site, String::new()));
                host_matches(&host, h) && (p.is_empty() || path.starts_with(&p))
            })
            .map(|l| (l.user.clone(), l.password.clone()))
    }

    pub fn column_visible(&self, key: &str) -> bool {
        self.columns.iter().any(|c| c == key)
    }

    /// The visible columns of the download list, in their order, with their
    /// widths: "name" always, the others as View > Columns says.
    pub fn column_layout(&self) -> Vec<(String, u32)> {
        let mut order: Vec<String> = self
            .column_order
            .iter()
            .filter(|k| k.as_str() == "name" || ALL_COLUMNS.contains(&k.as_str()))
            .cloned()
            .collect();
        order.dedup();
        for k in std::iter::once("name").chain(ALL_COLUMNS) {
            if !order.iter().any(|o| o == k) {
                order.push(k.to_string());
            }
        }
        order
            .into_iter()
            .filter(|k| k == "name" || self.column_visible(k))
            .map(|k| {
                let w = self.column_widths.get(&k).copied().unwrap_or_else(|| default_column_width(&k));
                (k, w.clamp(36, 2000))
            })
            .collect()
    }

    pub fn queue(&self, id: u64) -> Option<&QueueDef> {
        self.queues.iter().find(|q| q.id == id)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        crate::store::write_json_atomic(path, self)
    }

    /// bytes per second for the limiter (0 = full speed)
    pub fn effective_limit(&self) -> u64 {
        if self.speed_limit_on {
            self.speed_limit_kb * 1024
        } else {
            0
        }
    }

    pub fn connections(&self) -> u32 {
        self.connections.clamp(1, 64)
    }

    pub fn min_split(&self) -> u64 {
        self.min_split_kb.max(32) * 1024
    }

    /// Folder for a new download of the given category. "General"
    /// files go straight into the download folder.
    /// (A folder set in the category's properties wins.)
    pub fn dir_for(&self, category: &str) -> PathBuf {
        if let Some(p) = self.category_dirs.get(category).filter(|p| !p.as_os_str().is_empty()) {
            return p.clone();
        }
        if self.category_folders && category != "General" {
            self.download_dir.join(category)
        } else {
            self.download_dir.clone()
        }
    }

    /// Create the download folder and one sub-folder per category, so they
    /// are there in Windows from the start.
    pub fn create_folders(&self) {
        let _ = std::fs::create_dir_all(&self.download_dir);
        if self.category_folders {
            for c in &self.categories {
                if !self.category_dirs.contains_key(&c.name) {
                    let _ = std::fs::create_dir_all(self.download_dir.join(&c.name));
                }
            }
        }
    }
}

/// "example.com" matches example.com and www.example.com; "*.x.com" and
/// "*" patterns work too.
pub fn host_matches(host: &str, pattern: &str) -> bool {
    let p = pattern.trim().trim_start_matches("http://").trim_start_matches("https://");
    let p = p.split('/').next().unwrap_or("").to_ascii_lowercase();
    if p.is_empty() {
        return false;
    }
    if p.contains('*') {
        return wildcard_match(&p, host);
    }
    host == p || host.ends_with(&format!(".{p}"))
}

/// `*` matches any run of characters, `?` one character (case-insensitive).
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ti;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parse "HH:MM" to (hour, minute).
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}
