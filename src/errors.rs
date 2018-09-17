// https://github.com/rust-lang-nursery/error-chain/issues/213
// needs an upgrade to error-chain 0.11

// FIXME: replace this with #![allow(unused_doc_comment)] when we don't care about 1.26.* anymore.
#![allow(unknown_lints, renamed_and_removed_lints, unused_doc_comments, unused_doc_comment)]

error_chain! {
    foreign_links {
        IoError(::std::io::Error);
        UrlParseError(::url::ParseError);
        SerdeJson(::serde_json::Error);
        ReqwestError(::reqwest::Error);
        TomlDe(::toml::de::Error);
        Hyper(::hyper::Error);
        ParseInt(::std::num::ParseIntError);
        Parse(::std::string::ParseError);
        RusotoTls(::rusoto_core::TlsError);
        RusotoParseRegion(::rusoto_core::ParseRegionError);
        Rusqlite(::rusqlite::Error);
        R2D2(::r2d2::Error);
        Base64Decode(::base64::DecodeError);
        Tera(::tera::Error);
        Utf8(::std::string::FromUtf8Error);
        CratesIndex(::crates_index::Error);
    }

    errors {
        Error404 {
            description("not found")
        }
        Timeout(what: &'static str, when: u64) {
            description("the operation timed out")
            display("process killed after {} {}s", what, when)
        }
        Download{}
        BadS3Uri {
            description("the S3 URI could not be parsed.")
        }
        ServerUnavailable {
            description("the server is not available at the moment")
        }

        EmptyToolchainName {
            description("empty toolchain name")
        }
        InvalidToolchainSourceName(name: String) {
            description("invalid toolchain source name")
            display("invalid toolchain source name: {}", name)
        }
    }
}
