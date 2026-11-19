use super::*;

#[test]
fn links_written_as_text() {
    let html = r#"<pre>Mirror 1: https://files.example/part1.rar, and https://files.example/part2.rar.</pre>
        <script>var u = "https:\/\/cdn.example\/v.mp4"; var w = "https:\/\/cdn.example\/get?id=1\u0026t=2";</script>"#;
    let links: Vec<String> = extract_links(html, "https://site/").into_iter().map(|(u, _)| u).collect();
    assert!(links.contains(&"https://files.example/part1.rar".to_string()), "{links:?}");
    assert!(links.contains(&"https://files.example/part2.rar".to_string()), "{links:?}");
    assert!(links.contains(&"https://cdn.example/v.mp4".to_string()), "{links:?}");
    assert!(links.contains(&"https://cdn.example/get?id=1&t=2".to_string()), "{links:?}");
}

#[test]
fn links_from_html() {
    let html = r##"<html><body>
        <a href="files/a.pdf">Document <b>A</b></a>
        <a href='/b.pdf?x=1&amp;y=2'>B</a>
        <a href="#top">top</a> <a href="mailto:x@y.z">mail</a>
        <img src="//cdn.example.com/p.png"> <a href=page2.html>next</a>
    </body></html>"##;
    let l = extract_links(html, "https://example.com/dir/index.html");
    let urls: Vec<&str> = l.iter().map(|(u, _)| u.as_str()).collect();
    assert!(urls.contains(&"https://example.com/dir/files/a.pdf"));
    assert!(urls.contains(&"https://example.com/b.pdf?x=1&y=2"));
    assert!(urls.contains(&"https://cdn.example.com/p.png"));
    assert!(urls.contains(&"https://example.com/dir/page2.html"));
    assert!(!urls.iter().any(|u| u.contains("mailto") || u.ends_with("#top")));
    assert_eq!(l.iter().find(|(u, _)| u.ends_with("a.pdf")).unwrap().1, "Document A");
}

#[test]
fn extracts_srcset_data_attrs_css_json_base_entities_and_spa() {
    let html = r#"<html><head><base href="https://cdn.example.com/assets/"></head><body>
        <img srcset="img-1x.webp 1x, img-2x.webp 2x" data-lazy-src="hero.png">
        <a data-download="files/pack&#x2e;zip&#38;v=1" data-bg="url('../bg/tile.jpg')">dl</a>
        <object data="manual.pdf"></object><embed src=clip.mp4>
        <script>var p="\u002fmedia\u002faudio.mp3\u003fdl\u003d1";</script>
    </body></html>"#;
    let urls: Vec<String> = extract_links(html, "https://site.example/page").into_iter().map(|(u, _)| u).collect();
    assert!(urls.contains(&"https://cdn.example.com/assets/img-2x.webp".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/assets/hero.png".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/assets/files/pack.zip&v=1".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/bg/tile.jpg".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/assets/manual.pdf".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/assets/clip.mp4".into()), "{urls:?}");
    assert!(urls.contains(&"https://cdn.example.com/media/audio.mp3?dl=1".into()), "{urls:?}");

    let spa = r#"<html><body><div id="root"></div><noscript>Please enable JavaScript</noscript><script src="/a.js"></script><script src="/b.js"></script></body></html>"#;
    assert!(looks_dynamic_spa(spa));
    assert!(!looks_dynamic_spa(html));
}
