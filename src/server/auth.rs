use hyper::header::Authorization;
use hyper::server::{Request, Response};
use server::api_types::{ApiResponse, CraterToken};
use server::http::{Context, Handler, ResponseExt, ResponseFuture};
use server::Data;
use std::sync::Arc;

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
        let provided_token = req
            .headers()
            .get::<Authorization<CraterToken>>()
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
