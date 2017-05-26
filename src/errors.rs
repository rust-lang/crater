error_chain! {
    foreign_links {
        IoError(::std::io::Error);
        UrlParseError(::url::ParseError);
        SerdeJson(::serde_json::Error);
        ReqwestError(::reqwest::Error);
    }

    errors {
        Timeout(what: &'static str, when: u64) {
            description("the operation timed out")
            display("process killed after {} {}s", what, when)
        }
    }
}
