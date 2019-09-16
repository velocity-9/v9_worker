use hyper::{Body, Request, Response};

lazy_static! {
    pub static ref GLOBAL_HANDLER: HttpRequestHandler = HttpRequestHandler::new();
}

pub fn global_request_entrypoint(req: Request<Body>) -> Response<Body> {
    GLOBAL_HANDLER.handle(req)
}

#[derive(Debug)]
pub struct HttpRequestHandler {}

impl HttpRequestHandler {
    fn new() -> HttpRequestHandler {
        HttpRequestHandler {}
    }

    fn handle(&self, req: Request<Body>) -> Response<Body> {
        let method = req.method();
        let uri = req.uri();
        let query = uri.query().unwrap_or("");

        // Get the uri path, and then split it around slashes into components
        let path_components: Vec<&str> = uri.path().split('/').skip(1).collect();

        let response_body = if path_components[0] == "meta" {
            format!(
                "This is a meta request. path = {:?}, method = {:?}, query = {:?}",
                path_components, method, query
            )
        } else if path_components[0] == "sl" {
            format!(
                "This is a serverless request. path = {:?}, method = {:?}, query = {:?}",
                path_components, method, query
            )
        } else {
            "Invalid request".to_string()
        };

        Response::new(Body::from(response_body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_request_succeeds() {
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = global_request_entrypoint(req);

        assert_eq!(resp.status().as_u16(), 200)
    }
}
