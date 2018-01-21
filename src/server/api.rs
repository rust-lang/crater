//! Each API endpoint has its own module. The modules contain Request and/or
//! Response structs; these contain the specifications for how to interact
//! with the API.
//!
//! The responses are calculated in the server.rs file.

pub mod get {
    use server::{Data, Params};

    #[derive(Serialize, Deserialize)]
    pub struct Response {
        pub text: String,
    }

    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    pub fn handler(_data: &Data, _params: Params) -> Response {
        Response {
            text: String::from("This is a response!"),
        }
    }
}

pub mod post {
    use server::{Data, Params};

    #[derive(Serialize, Deserialize)]
    pub struct Request {
        pub input: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Response {
        pub out: String,
    }

    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    pub fn handler(post: Request, _data: &Data, _params: Params) -> Response {
        Response {
            out: format!("Got {}!", post.input),
        }
    }
}

pub mod ex_report {
    use ex;
    use report::{generate_report, TestResults};
    use server::{Data, Params};

    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    pub fn handler(_data: &Data, params: Params) -> TestResults {
        let ex_name = params.find("experiment").unwrap();
        let ex = ex::Experiment::load(ex_name).unwrap();
        generate_report(&ex).unwrap()
    }
}

pub mod ex_config {
    use ex;
    use server::{Data, Params};

    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    pub fn handler(_data: &Data, params: Params) -> ex::Experiment {
        let ex_name = params.find("experiment").unwrap();
        ex::Experiment::load(ex_name).unwrap()
    }
}

pub mod template_report {
    use report::Context;
    use server::{Data, Params};

    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    pub fn handler(_data: &Data, params: Params) -> Context {
        let ex_name = params.find("experiment").unwrap();
        Context {
            config_url: format!{"/api/ex/{}/config", ex_name},
            results_url: format!{"/api/ex/{}/results", ex_name},
            static_url: "/static/".into(),
        }
    }
}
