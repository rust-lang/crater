error_chain! {
    foreign_links {
        ::std::io::Error, IoError;
        ::url::ParseError, UrlParseError;
    }
}
