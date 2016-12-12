error_chain! {
    foreign_links {
        IoError(::std::io::Error);
        UrlParseError(::url::ParseError);
        SerdeJson(::serde_json::Error);
    }

    errors {
        Timeout {
            description("the operation timed out")
        }
    }
}
