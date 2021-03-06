use crate::clap_app::get_clap_app;
use crate::config::DevproxyConfig;
use crate::mapper::Mapper;
use actix_web::{
    client::Client, http::header::HOST, middleware, web, App, Error, HttpRequest, HttpResponse,
    HttpServer,
};
use futures::Future;
use std::collections::HashMap;
use std::path::PathBuf;

mod clap_app;
mod config;
mod mapper;

/// streaming client request to a streaming server response
fn streaming(
    req: HttpRequest,
    payload: web::Payload,
    client: web::Data<Client>,
    mapper: web::Data<Mapper>,
) -> impl Future<Item = HttpResponse, Error = impl Into<Error>> {
    let path = req.match_info().get("path").unwrap();
    let host = req
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap(); // no host header can not happen in HTTP 1.1, so… unwrap

    let url = dbg!(mapper.uri(host, path));

    let forwarded_req = client.request_from(url, req.head());

    forwarded_req
        .send_stream(payload)
        .map_err(Error::from)
        .and_then(|upstream_res| {
            let mut res = HttpResponse::build(upstream_res.status());
            res.no_chunking();

            // send upstream headers to downstream
            upstream_res
                .headers()
                .iter()
                .filter(|(h, _)| *h != "connection") // do not pass through connection-headers
                .for_each(|(header_name, header_value)| {
                    res.header(header_name.clone(), header_value.clone());
                });

            res.streaming(upstream_res)
        })
}

fn get_sample_mapping<'a>() -> Mapper<'a> {
    let mut map = HashMap::new();
    map.insert("example.com", "localhost:8180");

    Mapper::new(map)
}

fn main() {
    ::std::env::set_var("RUST_LOG", "actix_web=info");
    env_logger::init();

    let matches = get_clap_app().get_matches();

    let config_file = matches.value_of("config").map(PathBuf::from);

    let config = dbg!(DevproxyConfig::new(config_file));

    let sys = actix::System::new("http-proxy");

    HttpServer::new(|| {
        App::new()
            .data(Client::new())
            .data(get_sample_mapping())
            .wrap(middleware::Logger::default())
            .service(web::resource("/{path:.*}").to_async(streaming))
    })
    .bind(config.addr)
    .unwrap()
    .start();

    println!("Started http server: {}", config.addr);
    let _ = sys.run();
}
