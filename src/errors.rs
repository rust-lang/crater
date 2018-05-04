// https://github.com/rust-lang-nursery/error-chain/issues/213
// needs an upgrade to error-chain 0.11
#![allow(unused_doc_comment)]
#![cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
error_chain! {
    foreign_links {
        IoError(::std::io::Error);
        UrlParseError(::url::ParseError);
        SerdeJson(::serde_json::Error);
        ReqwestError(::reqwest::Error);
        RustupError(::rustup_dist::Error);
        TomlDe(::toml::de::Error);
        Hyper(::hyper::Error);
        ParseInt(::std::num::ParseIntError);
        Parse(::std::string::ParseError);
        RusotoTls(::rusoto_core::TlsError);
        Rusqlite(::rusqlite::Error);
        R2D2(::r2d2::Error);
        Base64Decode(::base64::DecodeError);
        Handlebars(::handlebars::TemplateRenderError);
    }

    links {
        CratesIndexError(::crates_index::Error, ::crates_index::ErrorKind);
    }

    errors {
        Timeout(what: &'static str, when: u64) {
            description("the operation timed out")
            display("process killed after {} {}s", what, when)
        }
        Download{}
        BadS3Uri {
            description("the S3 URI could not be parsed.")
        }
    }
}
