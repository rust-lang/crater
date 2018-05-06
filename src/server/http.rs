use errors::*;
use futures::future::{self, Future};
use futures::prelude::*;
use futures_cpupool::CpuPool;
use hyper::{Method, StatusCode};
use hyper::header::{ContentLength, ContentType};
use hyper::server::{Http, Request, Response, Service};
use serde::Serialize;
use server::api_types::ApiResponse;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use tokio_core::reactor::{Core, Handle};

pub type ResponseFuture = Box<Future<Item = Response, Error = ::hyper::Error>>;

pub trait Handler<D: 'static> {
    fn handle(&self, req: Request, data: Arc<D>, ctx: Arc<Context>) -> ResponseFuture;
}

impl<D: 'static, F: Fn(Request, Arc<D>, Arc<Context>) -> ResponseFuture> Handler<D> for F {
    fn handle(&self, req: Request, data: Arc<D>, ctx: Arc<Context>) -> ResponseFuture {
        (*self)(req, data, ctx)
    }
}

pub struct Context {
    pub handle: Handle,
    pub pool: CpuPool,
}

pub struct Server<D: 'static> {
    data: Arc<D>,
    context: Arc<Context>,
    routes: HashMap<(Method, &'static str), Box<Handler<D>>>,
    core: Option<Core>,
}

impl<D: 'static> Server<D> {
    pub fn new(data: D) -> Result<Self> {
        let core = Core::new()?;
        Ok(Server {
            data: Arc::new(data),
            context: Arc::new(Context {
                handle: core.handle(),
                pool: CpuPool::new_num_cpus(),
            }),
            routes: HashMap::new(),
            core: Some(core),
        })
    }

    pub fn add_route<H: Handler<D> + 'static>(
        &mut self,
        method: Method,
        path: &'static str,
        handler: H,
    ) {
        self.routes.insert((method, path), Box::new(handler));
    }

    pub fn run(mut self) -> Result<()> {
        let mut core = self.core.take().unwrap();
        let handle = core.handle();

        let addr = "127.0.0.1:8000".parse().unwrap();
        let inst = Arc::new(self);
        let server = Http::new().serve_addr_handle(&addr, &handle, move || Ok(inst.clone()))?;

        let handle_cloned = handle.clone();
        handle.spawn(
            server
                .for_each(move |conn| {
                    handle_cloned.spawn(
                        conn.map(|_| ())
                            .map_err(|err| error!("http error: {}", err)),
                    );
                    Ok(())
                })
                .map_err(|_| ()),
        );

        core.run(future::empty::<(), Error>())?;

        Ok(())
    }
}

impl<D: 'static> Service for Server<D> {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Request) -> ResponseFuture {
        if let Some(handler) = self.routes
            .get(&(req.method().clone(), &req.path().to_string()))
        {
            handler.handle(req, self.data.clone(), self.context.clone())
        } else {
            Response::text("404: Not Found")
                .with_status(StatusCode::NotFound)
                .as_future()
        }
    }
}

pub trait ResponseExt {
    fn text<S: Display>(text: S) -> Response;
    fn html<S: Display>(html: S) -> Response;
    fn json<S: Serialize>(data: &S) -> Result<Response>;
    fn api<T: Serialize>(resp: ApiResponse<T>) -> Result<Response>;
    fn as_future(self) -> ResponseFuture;
}

impl ResponseExt for Response {
    fn text<S: Display>(text: S) -> Response {
        let text = text.to_string();

        Response::new()
            .with_header(ContentLength(text.len() as u64))
            .with_header(ContentType::plaintext())
            .with_body(text)
    }

    fn html<S: Display>(html: S) -> Response {
        let html = html.to_string();

        Response::new()
            .with_header(ContentLength(html.len() as u64))
            .with_header(ContentType::html())
            .with_body(html)
    }

    fn json<S: Serialize>(data: &S) -> Result<Response> {
        let text = ::serde_json::to_vec(data)?;

        Ok(Response::new()
            .with_header(ContentLength(text.len() as u64))
            .with_header(ContentType::json())
            .with_body(text))
    }

    fn api<T: Serialize>(resp: ApiResponse<T>) -> Result<Response> {
        Ok(Response::json(&resp)?.with_status(resp.status_code()))
    }

    fn as_future(self) -> ResponseFuture {
        Box::new(future::ok(self))
    }
}

#[macro_export]
macro_rules! api_endpoint {
    ($name:ident: |
        $body_name:ident,
        $data_name:ident,
        $($other_name:ident: $other_type:ty),*
    | -> $result:ty $code:block, $inner:ident) => {
        fn $inner(
            $body_name: Vec<u8>,
            $data_name: Arc<Data>,
            $($other_name: $other_type)*,
        ) -> Result<ApiResponse<$result>> $code

        pub fn $name(
            req: Request,
            data: Arc<Data>,
            ctx: Arc<Context>,
            $($other_name: $other_type),*
        ) -> ResponseFuture {
            Box::new(req.body().concat2().and_then(move |body| {
                let body = body.iter().cloned().collect::<Vec<u8>>();

                ctx.pool
                    .spawn_fn(move || future::done($inner(body, data, $($other_name),*)))
                    .and_then(|resp| future::ok(Response::api(resp).unwrap()))
                    .or_else(|err| {
                        error!("internal error while processing request: {}", err);
                        let resp: ApiResponse<bool> = ApiResponse::InternalError {
                            error: err.to_string(),
                        };
                        future::ok(Response::api(resp).unwrap())
                    })
            }))
        }
    }
}
