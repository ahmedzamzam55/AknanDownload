//! A file must come whole and from one version: a file changed on the server
//! during a pause, servers that name each copy their own way, links that
//! work for one request only, pages that lead on to the file, and a site's
//! "not a robot" check.

mod helpers;
mod probe_and_post;
mod versions;
