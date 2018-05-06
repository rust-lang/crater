use hyper::header::{Authorization, Scheme};
use hyper::server::{Request, Response};
use server::Data;
use server::api_types::ApiResponse;
use server::http::{Context, Handler, ResponseExt, ResponseFuture};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Token {
    pub token: String,
}

impl Scheme for Token {
    fn scheme() -> Option<&'static str> {
        Some("token")
    }

    fn fmt_scheme(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.token)
    }
}

impl FromStr for Token {
    type Err = ::hyper::Error;

    fn from_str(s: &str) -> ::hyper::Result<Token> {
        Ok(Token {
            token: s.to_owned(),
        })
    }
}

enum TokenType {
    Agent,
}

pub struct AuthDetails {
    pub name: String,
}

pub struct AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    func: F,
    token_type: TokenType,
}

impl<F> Handler<Data> for AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    fn handle(&self, req: Request, data: Arc<Data>, ctx: Arc<Context>) -> ResponseFuture {
        let provided_token = req.headers()
            .get::<Authorization<Token>>()
            .map(|t| t.token.clone());

        let mut authorized_as = None;
        if let Some(provided_token) = provided_token {
            let tokens = match self.token_type {
                TokenType::Agent => &data.tokens.agents,
            };

            if let Some(name) = tokens.get(&provided_token) {
                authorized_as = Some(name.clone());
            }
        }

        if let Some(name) = authorized_as {
            (self.func)(req, data, ctx, AuthDetails { name: name.clone() })
        } else {
            let resp: ApiResponse<bool> = ApiResponse::Unauthorized;
            Response::api(resp).unwrap().as_future()
        }
    }
}

pub fn auth_agent<F>(func: F) -> AuthMiddleware<F>
where
    F: Fn(Request, Arc<Data>, Arc<Context>, AuthDetails) -> ResponseFuture,
{
    AuthMiddleware {
        func,
        token_type: TokenType::Agent,
    }
}
