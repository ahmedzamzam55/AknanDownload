use super::*;
#[test]
fn args() {
    assert_eq!(split_args(r#"/scan "C:\a b\x.zip" -q"#), vec!["/scan", r"C:\a b\x.zip", "-q"]);
    assert_eq!(split_args(""), Vec::<String>::new());
}

#[test]
fn mark_of_the_web() {
    let z = zone_identifier("https://user:pw@cdn.example.com/a.exe?x=1#frag", Some("https://example.com/page"));
    assert!(z.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"));
    assert!(z.contains("HostUrl=https://cdn.example.com/a.exe?x=1\r\n"));
    assert!(z.contains("ReferrerUrl=https://example.com/page\r\n"));
    assert!(!z.contains("pw") && !z.contains("frag"));
    // only web addresses are written down
    let z = zone_identifier("file:///C:/x.exe", Some("javascript:alert(1)"));
    assert_eq!(z, "[ZoneTransfer]\r\nZoneId=3\r\n");
}
