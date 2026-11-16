mod drawing_tests {
    use super::super::details_draw::{ring_path, speed_paths};

    #[test]
    fn ring_and_speed() {
        assert_eq!(ring_path(0.0), "");
        assert_eq!(ring_path(0.5), "M 50 7 A 43 43 0 0 1 50.00 93.00");
        assert!(ring_path(0.75).starts_with("M 50 7 A 43 43 0 1 1 7.00"));
        let (line, area) = speed_paths([0.0, 10.0].into_iter());
        assert!(line.starts_with("M 196.6 29.0 L 200.0 3.0"), "{line}");
        assert!(area.ends_with("L 200 30 L 196.6 30 Z"));
        assert_eq!(speed_paths([5.0].into_iter()).0, "");
    }
}

mod tests {
    use super::super::batch::expand_pattern;
    use super::super::columns::column_at;

    #[test]
    fn header_points_to_columns() {
        let w = [280.0, 30.0, 82.0];
        assert_eq!(column_at(&w, 10.0), 0);
        assert_eq!(column_at(&w, 38.0 + 279.0), 0);
        assert_eq!(column_at(&w, 38.0 + 281.0), 1);
        assert_eq!(column_at(&w, 5000.0), 2);
    }

    #[test]
    fn batch_patterns() {
        let l = expand_pattern("https://example.com/img*.jpg", false, "8", "10", 2).unwrap();
        assert_eq!(l, vec!["https://example.com/img08.jpg", "https://example.com/img09.jpg", "https://example.com/img10.jpg"]);
        let l = expand_pattern("https://example.com/*.pdf", true, "a", "c", 1).unwrap();
        assert_eq!(l.len(), 3);
        assert!(expand_pattern("https://example.com/x.pdf", false, "1", "2", 1).is_err());
    }
}

mod name_tests {
    use super::super::add_open::with_extension_of;

    #[test]
    fn typed_names_keep_the_extension() {
        assert_eq!(with_extension_of("My video", "Title [1080p].mp4"), "My video.mp4");
        assert_eq!(with_extension_of("My video.mkv", "Title.mp4"), "My video.mkv");
        assert_eq!(with_extension_of("Ep. 12 final", "a.rar"), "Ep. 12 final.rar");
        assert_eq!(with_extension_of("setup", ""), "setup");
        assert_eq!(with_extension_of("v1.2", "x.zip"), "v1.2");
    }

    #[test]
    fn toolbar_actions_map_to_handlers() {
        let tb = include_str!("../../ui/shell/toolbar.slint");
        let app = include_str!("../../ui/app.slint");
        let wire = [include_str!("main_wire.rs"), include_str!("main_menu.rs")].concat();
        let mut rest = tb;
        let mut count = 0usize;
        while let Some(i) = rest.find("root.act(\"") {
            let after = &rest[i + "root.act(\"".len()..];
            let Some(end) = after.find('"') else { break };
            let act = &after[..end];
            rest = &after[end + 1..];
            let needle = format!("c == \"{act}\"");
            let Some(pos) = app.find(&needle) else {
                panic!("missing act dispatch in app.slint for {act}");
            };
            let stmt = &app[pos..app[pos..].find('}').map(|n| pos + n).unwrap_or(app.len())];
            assert!(stmt.contains(&format!("root.{act}(")), "unexpected target for {act}: {stmt}");
            let cb = format!("on_{}", act.replace('-', "_"));
            assert!(wire.contains(&cb), "missing Rust callback {cb} for toolbar action {act}");
            count += 1;
        }
        assert!(count >= 13, "expected toolbar actions, saw {count}");
    }
}
