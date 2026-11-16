// other systems (the tests): no user key store, so nothing is encrypted
pub fn protect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}
pub fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
    None
}
