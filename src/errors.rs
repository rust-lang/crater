error_chain! {
    foreign_links {
        IoError(::std::io::Error);
        UrlParseError(::url::ParseError);
    }

    errors {
        Timeout {
            description("the operation timed out")
        }
    }
}
