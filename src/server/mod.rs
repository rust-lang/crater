use arc_cell::ArcCell;
use futures::{self, Future, Stream};
use futures_cpupool::CpuPool;
use hyper::{self, Get, Post, StatusCode};
use hyper::header::{ContentLength, ContentType};
use hyper::server::{Http, Request, Response, Service};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json;
use std::env;
use std::fs::File;
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::str;
use std::sync::Arc;

mod api;

pub struct Data;

struct Server {
    data: ArcCell<Data>,
    pool: CpuPool,
}

impl Server {
    fn handle_get<F, S>(&self, req: &Request, handler: F) -> <Server as Service>::Future
        where F: FnOnce(&Data) -> S,
              S: Serialize
    {
        assert_eq!(*req.method(), Get);
        let data = self.data.get();
        let result = handler(&data);
        let response = Response::new()
            .with_header(ContentType::json())
            .with_body(serde_json::to_string(&result).unwrap());
        futures::future::ok(response).boxed()
    }

    fn handle_post<F, D, S>(&self, req: Request, handler: F) -> <Server as Service>::Future
        where F: FnOnce(D, &Data) -> S + Send + 'static,
              D: DeserializeOwned,
              S: Serialize
    {
        assert_eq!(*req.method(), Post);
        let length = req.headers()
            .get::<ContentLength>()
            .expect("content-length to exist")
            .0;
        if length > 10_000 {
            // 10 kB
            return futures::future::err(hyper::Error::TooLarge).boxed();
        }
        let data = self.data.get();
        self.pool
            .spawn_fn(move || {
                req.body()
                    .fold(Vec::new(), |mut acc, chunk| {
                        acc.extend_from_slice(&*chunk);
                        futures::future::ok::<_, <Self as Service>::Error>(acc)
                    })
                    .map(move |body| {
                        let body: D = match serde_json::from_slice(&body) {
                            Ok(d) => d,
                            Err(err) => {
                                error!("failed to deserialize request {}: {:?}",
                                       String::from_utf8_lossy(&body),
                                       err);
                                return Response::new()
                                           .with_header(ContentType::plaintext())
                                           .with_body(format!("Failed to deserialize request; {:?}",
                                                              err));
                            }
                        };
                        let result = handler(body, &data);
                        Response::new()
                            .with_header(ContentType::json())
                            .with_body(serde_json::to_string(&result).unwrap())
                    })
            })
            .boxed()
    }
}

impl Service for Server {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        let fs_path = format!("static{}",
                              if req.path() == "" || req.path() == "/" {
                                  "/index.html"
                              } else {
                                  req.path()
                              });

        info!("handling: req.path()={:?}, fs_path={:?}",
              req.path(),
              fs_path);

        if fs_path.contains("./") | fs_path.contains("../") {
            return futures::future::ok(Response::new()
                                           .with_header(ContentType::html())
                                           .with_status(StatusCode::NotFound))
                           .boxed();
        }

        if Path::new(&fs_path).is_file() {
            return self.pool
                       .spawn_fn(move || {
                                     let mut f = File::open(&fs_path).unwrap();
                                     let mut source = Vec::new();
                                     f.read_to_end(&mut source).unwrap();
                                     futures::future::ok(Response::new().with_body(source))
                                 })
                       .boxed();
        }

        match req.path() {
            "/api/get" => self.handle_get(&req, api::get::handler),
            "/api/post" => self.handle_post(req, api::post::handler),
            _ => {
                futures::future::ok(Response::new()
                                        .with_header(ContentType::html())
                                        .with_status(StatusCode::NotFound))
                        .boxed()
            }
        }
    }
}

pub fn start(data: Data) {
    let server = Arc::new(Server {
                              data: ArcCell::new(Arc::new(data)),
                              pool: CpuPool::new_num_cpus(),
                          });
    let mut server_address: SocketAddr = "0.0.0.0:2346".parse().unwrap();
    server_address.set_port(env::var("PORT")
                                .ok()
                                .and_then(|x| x.parse().ok())
                                .unwrap_or(2346));
    let server = Http::new().bind(&server_address, move || Ok(server.clone()));
    server.unwrap().run().unwrap();
}
