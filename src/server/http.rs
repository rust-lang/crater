use errors::*;
use futures::future::{self, Future};
use futures::prelude::*;
use futures_cpupool::CpuPool;
use hyper::{Method, StatusCode};
use hyper::header::ContentLength;
use hyper::server::{Http, Request, Response, Service};
use std::collections::HashMap;
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
            let message = "404: Not Found\n";

            Box::new(future::ok(
                Response::new()
                    .with_header(ContentLength(message.len() as u64))
                    .with_body(message)
                    .with_status(StatusCode::NotFound),
            ))
        }
    }
}
