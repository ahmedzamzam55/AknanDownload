use super::types::AddRequest;

/// Whether the program gets a file at `add.url` with the browser's sign-in
/// (a short look, a few seconds at most; when it can't tell, yes).
pub(super) fn can_fetch(add: &AddRequest) -> bool {
    let mut headers = swiftget_engine::RequestHeaders {
        referer: add.referrer.clone(),
        cookies: add.cookies.clone(),
        user_agent: add.user_agent.clone(),
        extra: Vec::new(),
        auth: None,
        post: add.post.clone(),
        post_type: add.post_type.clone(),
    };
    if let Some(list) = &add.headers {
        swiftget_engine::ytdlp::apply_headers(&mut headers, list);
    }
    // one lasting runtime for these looks: the answer a look keeps for the
    // download stays open only while the runtime it came on runs
    static RT: std::sync::LazyLock<Option<tokio::runtime::Runtime>> =
        std::sync::LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().ok());
    let Some(rt) = RT.as_ref() else { return true };
    let Ok(client) = swiftget_engine::http::build_client_with(&swiftget_engine::http::current_proxy()) else { return true };
    let url = add.url.clone();
    rt.block_on(async move {
        let look = swiftget_engine::http::probe_quick(&client, &url, &headers, swiftget_engine::config::DEFAULT_USER_AGENT);
        match tokio::time::timeout(std::time::Duration::from_millis(2500), look).await {
            // a sign-in page instead of the file
            Ok(Ok(p)) => !p.is_web_page(),
            Ok(Err(e)) => match swiftget_engine::http::http_status_of(&e) {
                Some(code) => !matches!(code, 400 | 401 | 403 | 404 | 405 | 410),
                None => {
                    let text = format!("{e:#}");
                    !(text.contains("HTTP 40") || text.contains("HTTP 410") || text.contains("user name and password"))
                }
            },
            // slow to answer: the program takes it (it waits longer)
            Err(_) => true,
        }
    })
}
