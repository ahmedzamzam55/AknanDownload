use super::*;

#[test]
fn wildcards() {
    assert!(wildcard_match("*.example.com", "dl.example.com"));
    assert!(!wildcard_match("*.example.com", "example.org"));
    assert!(wildcard_match("r0*", "r01"));
    assert!(host_matches("www.site.com", "site.com"));
    assert!(!host_matches("othersite.com", "site.com"));
}

#[test]
fn categories_and_logins() {
    let mut s = Settings::default();
    assert_eq!(s.category_of("a.ZIP", "https://example.com/get?id=1"), "Compressed");
    assert_eq!(s.category_of("noext", "https://example.com/"), "General");
    s.categories.push(CategoryDef {
        name: "Courses".into(),
        exts: vec![],
        sites: vec!["courses.example.com".into()],
        builtin: false,
    });
    assert_eq!(s.category_of("v.mp4", "https://www.courses.example.com/v.mp4"), "Courses");
    s.logins.push(SiteLogin { site: "example.com/members".into(), user: "u".into(), password: "p".into() });
    assert!(s.login_for("https://example.com/members/a.pdf").is_some());
    assert!(s.login_for("https://example.com/public/a.pdf").is_none());
    s.server_limits.push(ServerLimit { host: "slow.example.com".into(), connections: 2 });
    assert_eq!(s.connections_for("https://a.slow.example.com/x", 32), 2);
    assert_eq!(s.connections_for("https://fast.example.com/x", 32), 32);
}

#[cfg(test)]
mod column_tests {
    use super::*;

    #[test]
    fn columns_keep_their_order_and_width() {
        let mut s = Settings::default();
        let keys = |s: &Settings| s.column_layout().into_iter().map(|c| c.0).collect::<Vec<_>>();
        // new settings: the name first, then the default columns
        assert_eq!(keys(&s)[0], "name");
        assert!(keys(&s).contains(&"size".to_string()));
        s.column_order = vec!["size".into(), "name".into(), "bogus".into()];
        s.column_widths.insert("size".into(), 5);
        s.column_widths.insert("name".into(), 400);
        let l = s.column_layout();
        assert_eq!(l[0], ("size".to_string(), 36)); // never narrower than 36
        assert_eq!(l[1], ("name".to_string(), 400));
        assert!(!keys(&s).contains(&"bogus".to_string()));
        // hidden in View > Columns: not shown, order kept for later
        s.columns.retain(|c| c != "size");
        assert_eq!(keys(&s)[0], "name");
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn the_next_start_of_a_queue() {
        // Wednesday 2026-11-11 10:00
        let now = chrono::Local.with_ymd_and_hms(2026, 11, 11, 10, 0, 0).unwrap();
        let mut s = QueueSchedule { start_enabled: true, start_time: "02:30".into(), ..Default::default() };
        // every day: tomorrow at 02:30
        assert_eq!(s.next_start(now).unwrap().format("%Y-%m-%d %H:%M").to_string(), "2026-11-12 02:30");
        // only on Mondays
        s.days = [true, false, false, false, false, false, false];
        assert_eq!(s.next_start(now).unwrap().format("%Y-%m-%d").to_string(), "2026-11-16");
        // once, on a day gone by: never
        s.once = true;
        s.once_date = "2026-11-01".into();
        assert!(s.next_start(now).is_none());
        s.once_date = "2026-11-11".into();
        s.start_time = "23:00".into();
        assert_eq!(s.next_start(now).unwrap().format("%H:%M").to_string(), "23:00");
        s.start_enabled = false;
        assert!(s.next_start(now).is_none());
    }
}
