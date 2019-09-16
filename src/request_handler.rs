use hyper::{Body, Request, Response};

pub fn request_entrypoint(req: Request<Body>) -> Response<Body> {
    Response::new(Body::from(req.uri().path().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_request_succeeds() {
        let req = Request::builder().uri("/").body(Body::empty()).unwrap();
        let resp = request_entrypoint(req);

        assert_eq!(resp.status().as_u16(), 200)
    }
}
