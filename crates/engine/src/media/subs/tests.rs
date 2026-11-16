use super::convert_subtitles;

#[test]
fn converts_vtt_to_srt_vtt_and_txt() {
    let vtt = "\u{feff}WEBVTT\nKind: captions\nLanguage: ar\n\n00:01.500 --> 00:04.200 align:start\n<c>مرحبا &amp; أهلا</c>\n\n00:04.200 --> 00:05.000\nمرحبا &amp; أهلا\n\n00:06.000 --> 00:08.500\n<b>السطر</b> الثاني\n";
    let srt = convert_subtitles(vtt, "srt");
    assert!(srt.contains("1\n00:00:01,500 --> 00:00:05,000\nمرحبا & أهلا"), "{srt}");
    assert!(srt.contains("2\n00:00:06,000 --> 00:00:08,500\nالسطر الثاني"), "{srt}");
    let out_vtt = convert_subtitles(vtt, "vtt");
    assert!(out_vtt.starts_with("WEBVTT\n\n00:00:01.500 --> 00:00:05.000"), "{out_vtt}");
    let txt = convert_subtitles(vtt, "txt");
    assert_eq!(txt, "مرحبا & أهلا\nالسطر الثاني");
}

#[test]
fn converts_json3_and_xml_to_srt() {
    let json3 = r#"{"events":[{"tStartMs":1200,"dDurationMs":2300,"segs":[{"utf8":"Hello "},{"utf8":"world"}]},{"tStartMs":4000,"dDurationMs":1000,"segs":[{"utf8":"\n"}]}]}"#;
    let srt = convert_subtitles(json3, "srt");
    assert_eq!(srt, "1\n00:00:01,200 --> 00:00:03,500\nHello world\n\n");

    let srv3 = r#"<timedtext><body><p t="500" d="1500">Line 1<br/>Line 2 &#39;ok&#39;</p></body></timedtext>"#;
    let srt2 = convert_subtitles(srv3, "srt");
    assert!(srt2.contains("1\n00:00:00,500 --> 00:00:02,000\nLine 1\nLine 2 'ok'"), "{srt2}");

    let ttml = r#"<tt><body><div><p begin="00:00:10.000" end="00:00:12.500">One</p></div></body></tt>"#;
    assert!(convert_subtitles(ttml, "srt").contains("00:00:10,000 --> 00:00:12,500\nOne"));
}
