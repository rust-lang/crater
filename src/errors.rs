error_chain! {
    foreign_links {
        ::std::io::Error, IoError;
        ::url::ParseError, UrlParseError;
    }

    errors {
        Timeout {
            description("the operation timed out")
        }
    }
}
