use results::TestResult;

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
        Rusqlite(::rusqlite::Error);
        RusotoTls(::rusoto_core::request::TlsError);
        RusotoParseRegion(::rusoto_core::region::ParseRegionError);
        R2D2(::r2d2::Error);
        Base64Decode(::base64::DecodeError);
        Tera(::tera::Error);
        Utf8(::std::string::FromUtf8Error);
        CratesIndex(::crates_index::Error);
        Csv(::csv::Error);
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
        InvalidToolchainFlag(name: String) {
            description("invalid toolchain flag")
            display("invalid toolchain flag: {}", name)
        }

        ExperimentNotFound(name: String) {
            description("experiment not found")
            display("experiment '{}' not found", name)
        }
        ExperimentAlreadyExists(name: String) {
            description("experiment already exists")
            display("experiment '{}' already exists", name)
        }
        DuplicateToolchains {
            description("duplicate toolchains provided")
        }
        CanEditOnlyQueuedExperiments {
            description("it's only possible to edit queued experiments")
        }

        EmptyAssignee {
            description("the assignee is empty")
        }
        InvalidAssigneeKind(name: String) {
            description("invalid assignee kind")
            display("invalid assignee kind: {}", name)
        }
        UnexpectedAssigneePayload {
            description("unexpected assignee payload")
        }

        KillProcessFailed(reason: String) {
            description("killing a process failed")
            display("killing a process failed: {}", reason)
        }

        BadConfig {
            description("the config file contains errors")
        }

        OverrideResult(result: TestResult) {
            description("overridden task result")
            display("overridden task result to {}", result.to_str())
        }
    }
}
