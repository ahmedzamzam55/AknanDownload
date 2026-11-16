use super::paths::replace;

#[test]
fn replace_a_tool() {
    let dir = std::env::temp_dir().join(format!("aknan-tools-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("tool.exe");
    std::fs::write(&target, b"old").unwrap();
    let new = dir.join("tool.new");
    std::fs::write(&new, b"fresh").unwrap();
    replace(&new, &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"fresh");
    let _ = std::fs::remove_dir_all(&dir);
}
