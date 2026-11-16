use super::*;

#[test]
fn times_of_a_part() {
    assert_eq!(parse_clock("1:02:05"), Some(3725.0));
    assert_eq!(parse_clock("02:05"), Some(125.0));
    assert_eq!(parse_clock("125"), Some(125.0));
    assert_eq!(parse_clock("2:05.5"), Some(125.5));
    assert_eq!(parse_clock("٠١:٣٠"), Some(90.0));
    assert_eq!(parse_clock("1:75"), None);
    assert_eq!(parse_clock("abc"), None);
    assert_eq!(parse_clock(""), None);
    assert_eq!(clock(75.9, ':'), "01:15");
    assert_eq!(clock(3725.0, '-'), "1-02-05");
    assert!(Clip::new(5.0, 5.2, false).is_none());
    assert!(Clip::new(-1.0, 5.0, false).is_none());
    let c = Clip::new(80.0, 225.0, false).unwrap();
    assert_eq!(DownloadRecord::clip_name("Trip [1080p].webm", &c), "Trip [1080p] [01-20 → 03-45].mp4");
    assert_eq!(DownloadRecord::clip_name("Song.mp3", &c), "Song [01-20 → 03-45].mp3");
}
