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
        Response { text: String::from("This is a response!") }
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
        Response { out: format!("Got {}!", post.input) }
    }
}
