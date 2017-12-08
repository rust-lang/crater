use arc_cell::ArcCell;
use futures::{self, Future, Stream};
use futures_cpupool::CpuPool;
use handlebars::Handlebars;
use hyper::{self, Get, Post, StatusCode};
use hyper::header::{ContentLength, ContentType};
use hyper::server::{Http, Request, Response, Service};
use mime;
use route_recognizer::{Match, Params, Router};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json;
use std::env;
use std::net::SocketAddr;
use std::str;
use std::sync::Arc;

mod api;

pub struct Data;

type Handler = Box<
    Fn(&Server, Request, Params) -> Box<Future<Item = Response, Error = hyper::Error>>
        + Sync
        + Send
        + 'static,
>;
struct Server {
    router: Router<Handler>,
    data: ArcCell<Data>,
    pool: CpuPool,
}

impl Server {
    fn handle_get<F, S>(
        &self,
        req: Request,
        params: Params,
        handler: F,
    ) -> <Server as Service>::Future
    where
        F: FnOnce(&Data, Params) -> S,
        S: Serialize,
    {
        if *req.method() != Get {
            return self.error(StatusCode::BadRequest);
        };
        let data = self.data.get();
        let result = handler(&data, params);
        let response = Response::new()
            .with_header(ContentType::json())
            .with_body(serde_json::to_string(&result).unwrap());
        Box::new(futures::future::ok(response))
    }

    fn handle_static(
        &self,
        req: Request,
        _params: Params,
        content_type: ContentType,
        body: &'static str,
    ) -> <Server as Service>::Future {
        if *req.method() != Get {
            return self.error(StatusCode::BadRequest);
        };
        let response = Response::new().with_header(content_type).with_body(body);
        Box::new(futures::future::ok(response))
    }

    fn handle_template<F, S>(
        &self,
        req: Request,
        params: Params,
        content_type: ContentType,
        context_fn: F,
        template: &'static str,
    ) -> <Server as Service>::Future
    where
        F: FnOnce(&Data, Params) -> S + Send + 'static,
        S: Serialize,
    {
        if *req.method() != Get {
            return self.error(StatusCode::BadRequest);
        };
        let data = self.data.get();
        let context = context_fn(&data, params);
        // TODO: Precompile templates.
        // TODO: Stream body.
        // TODO: Errors
        let body = Handlebars::new()
            .template_render(template, &context)
            .unwrap();
        let response = Response::new().with_header(content_type).with_body(body);
        Box::new(futures::future::ok(response))
    }

    fn handle_post<F, D, S>(
        &self,
        req: Request,
        params: Params,
        handler: F,
    ) -> <Server as Service>::Future
    where
        F: FnOnce(D, &Data, Params) -> S + Send + 'static,
        D: DeserializeOwned,
        S: Serialize,
    {
        if *req.method() != Post {
            return self.error(StatusCode::BadRequest);
        };
        let length = req.headers()
            .get::<ContentLength>()
            .expect("content-length to exist")
            .0;
        if length > 10_000 {
            // 10 kB
            return Box::new(futures::future::err(hyper::Error::TooLarge));
        }
        let data = self.data.get();
        Box::new(self.pool.spawn_fn(move || {
            req.body()
                .fold(Vec::new(), |mut acc, chunk| {
                    acc.extend_from_slice(&*chunk);
                    futures::future::ok::<_, <Self as Service>::Error>(acc)
                })
                .map(move |body| {
                    let body: D = match serde_json::from_slice(&body) {
                        Ok(d) => d,
                        Err(err) => {
                            error!(
                                "failed to deserialize request {}: {:?}",
                                String::from_utf8_lossy(&body),
                                err
                            );
                            return Response::new()
                                .with_header(ContentType::plaintext())
                                .with_body(format!("Failed to deserialize request; {:?}", err));
                        }
                    };
                    let result = handler(body, &data, params);
                    Response::new()
                        .with_header(ContentType::json())
                        .with_body(serde_json::to_string(&result).unwrap())
                })
        }))
    }

    fn error(&self, status: StatusCode) -> <Server as Service>::Future {
        Box::new(futures::future::ok(
            Response::new()
                .with_header(ContentType::html())
                .with_status(status),
        ))
    }
}

impl Service for Server {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        info!("handling: req.path()={:?}", req.path());

        match self.router.recognize(req.path()) {
            Ok(Match { handler, params }) => handler(self, req, params),
            Err(_) => self.error(StatusCode::NotFound),
        }
    }
}

macro_rules! route {
    ($router:ident, $path:expr, $method:ident, $($handler:tt)* ) => (
        $router.add($path,
            Box::new(|server: &Server, req, params| server.$method(req, params, $($handler)*)));
    )
}

pub fn start(data: Data) {
    let mut router = Router::<Handler>::new();
    route!(router, "/api/get", handle_get, api::get::handler);
    route!(router, "/api/post", handle_post, api::post::handler);
    route!(
        router,
        "/api/ex/:experiment/results",
        handle_get,
        api::ex_report::handler
    );
    route!(
        router,
        "/api/ex/:experiment/config",
        handle_get,
        api::ex_config::handler
    );
    route!(
        router,
        "/report/:experiment",
        handle_template,
        ContentType::html(),
        api::template_report::handler,
        include_str!("../../template/report.html")
    );
    route!(
        router,
        "/static/report.js",
        handle_static,
        ContentType(mime::TEXT_JAVASCRIPT),
        include_str!("../../static/report.js")
    );
    route!(
        router,
        "/static/report.css",
        handle_static,
        ContentType(mime::TEXT_CSS),
        include_str!("../../static/report.css")
    );

    let server = Arc::new(Server {
        router,
        data: ArcCell::new(Arc::new(data)),
        pool: CpuPool::new_num_cpus(),
    });
    let mut server_address: SocketAddr = "0.0.0.0:2346".parse().unwrap();
    server_address.set_port(
        env::var("PORT")
            .ok()
            .and_then(|x| x.parse().ok())
            .unwrap_or(2346),
    );
    let server = Http::new().bind(&server_address, move || Ok(Arc::clone(&server)));
    server.unwrap().run().unwrap();
}
