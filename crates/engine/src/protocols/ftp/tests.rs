use super::*;

#[test]
fn addresses() {
    let s = Site::new("ftp://ftp.test/pub/a%20b.iso", None).unwrap();
    assert_eq!((s.port, s.user.as_str(), s.path.as_str(), s.implicit, s.explicit), (21, "anonymous", "/pub/a b.iso", false, false));
    let s = Site::new("ftps://u%40x:p%3Aw@ftp.test/f.zip", None).unwrap();
    assert_eq!((s.port, s.user.as_str(), s.password.as_str(), s.implicit), (990, "u@x", "p:w", true));
    let s = Site::new("ftps://ftp.test:21/f.zip", Some(&("me".into(), "pw".into()))).unwrap();
    assert!(s.explicit && !s.implicit && s.user == "me");
    assert!(Site::new("ftp://ftp.test/", None).is_err());
    assert!(Site::new("ftp://ftp.test/a%0D%0ADELE%20x", None).is_err(), "no command smuggled in a path");
    assert_eq!(Site::without_password("ftp://u:p@h.test/f"), "ftp://h.test/f");
    assert_eq!(Site::login_in("ftp://u:p%21@h.test/f"), Some(("u".into(), "p!".into())));
    assert_eq!(file_name("ftp://h.test/dir/My%20File.zip"), "My File.zip");
    assert!(is_ftp("FTPS://x/y") && !is_ftp("https://x"));
}
