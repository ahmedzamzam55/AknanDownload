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
