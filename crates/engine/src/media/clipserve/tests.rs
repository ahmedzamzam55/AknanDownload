use super::*;


#[test]
fn ranges() {
    assert_eq!(parse_range("bytes=100-"), Some((100, None)));
    assert_eq!(parse_range(" bytes=0-99"), Some((0, Some(99))));
    assert_eq!(parse_range("bytes=9-3"), None);
    assert_eq!(parse_range("bytes=0-1,5-9"), None);
    assert_eq!(parse_range("items=0-1"), None);
    assert_eq!(total_of("bytes 0-99/1000"), Some(1000));
    assert_eq!(total_of("bytes 0-99/*"), None);
}
