use std::str;
use std::sync::{Mutex, MutexGuard};

use hyper::rt::{Future, Stream};
use hyper::{Body, Method, Request, Response, Uri};

use crate::components::ComponentManager;
use crate::error::WorkerError;
use crate::model::ComponentPath;
use crate::server::BoxedHyperFuture;

lazy_static! {
    pub static ref GLOBAL_HANDLER: HttpRequestHandler = HttpRequestHandler::new();
}

// Warning: This method is somewhat complicated, since it needs to deal with async stuff
pub fn global_request_entrypoint(req: Request<Body>) -> BoxedHyperFuture {
    debug!("{:?}", req);

    // Pull the verb, uri, and query stuff out of the request
    let http_verb = req.method().clone();
    let uri = req.uri().clone();
    let query = uri.query().unwrap_or("").to_string();

    // Then get a future representing the body (this is a future, since hyper may not of received the whole body yet)
    let body_future = req.into_body().concat2().map(|c| {
        str::from_utf8(&c)
            .map(str::to_owned)
            .map_err(WorkerError::from)
    });

    // Next we want to an operation on the body. This needs to happen in a future for two reasons
    // 1) We want to handle multiple requests at once, so we don't want to block a thread
    // 2) Hyper literally doesn't let you deal with the body unless you're inside a future context (there is no API to escape this)
    // Note: We already have a result (body_result) here, since we might get an Utf8 decode error above
    Box::new(body_future.map(move |body_result| {
        let resp: Response<Body> = body_result
            // Delegate to the GLOBAL_HANDLER to actually deal with this request
            .and_then(|body| GLOBAL_HANDLER.handle(http_verb, uri, query, body))
            .unwrap_or_else(|e| {
                warn!("Forced to convert error {:?} into a http response", e);
                e.into()
            });

        if resp.status() == 400 {
            error!("INTERNAL SERVER ERROR -- {:?}", resp);
        } else {
            debug!("{:?}", resp);
        }

        resp
    }))
}

#[derive(Debug)]
pub struct HttpRequestHandler {
    serverless_component_manager: Mutex<ComponentManager>,
}

impl HttpRequestHandler {
    fn new() -> HttpRequestHandler {
        HttpRequestHandler {
            serverless_component_manager: Mutex::new(ComponentManager::new()),
        }
    }

    fn handle(
        &self,
        http_verb: Method,
        uri: Uri,
        query: String,
        body: String,
    ) -> Result<Response<Body>, WorkerError> {
        // Get the uri path, and then split it around slashes into components
        // Note: All URIs start with a slash, so we skip the first entry in the split (which is always just "")
        let path_components: Vec<&str> = uri.path().split('/').skip(1).collect();

        let mut component_router = self
            .serverless_component_manager
            .lock()
            .map_err(|_| WorkerError::MutexPoisonedError)?;

        if path_components[0] == "meta" && path_components.len() >= 2 {
            self.handle_meta_request(component_router, path_components[1], &body)
        } else if path_components[0] == "sl" && path_components.len() >= 4 {
            debug!("Starting serverless request processing...");
            let user = path_components[1].to_string();
            let repo = path_components[2].to_string();
            let method = path_components[3];

            let path = ComponentPath::new(user, repo);
            let component = component_router.lookup_component(&path);

            component
                .map(|component_handle| {
                    Ok(component_handle.handle_component_call(
                        method,
                        http_verb,
                        &path_components[4..],
                        query,
                        body.to_string(),
                    ))
                })
                .unwrap_or_else(|| {
                    debug!("Could not find serverless component {:?}", path);
                    Err(WorkerError::PathNotFound(path_components.join("/")))
                })
        } else {
            Err(WorkerError::PathNotFound(path_components.join("/")))
        }
    }

    fn handle_meta_request(
        &self,
        mut component_router: MutexGuard<ComponentManager>,
        route: &str,
        body: &str,
    ) -> Result<Response<Body>, WorkerError> {
        // TODO: Add invalid request error, and send back something other than a 500 when the JSON is invalid
        // TODO: Validate the HTTP verb is correct
        let result_body = Body::from(match route {
            "activate" => {
                let resp = component_router
                    .activate(serde_json::from_str(&body).map_err(WorkerError::from)?);
                serde_json::to_string(&resp).map_err(WorkerError::from)?
            }
            "deactivate" => {
                let resp = component_router
                    .deactivate(serde_json::from_str(&body).map_err(WorkerError::from)?);
                serde_json::to_string(&resp).map_err(WorkerError::from)?
            }
            "status" => {
                let resp = component_router.status();
                serde_json::to_string(&resp).map_err(WorkerError::from)?
            }
            _ => Err(WorkerError::PathNotFound("meta/".to_string() + route))?,
        });
        Ok(Response::builder().status(200).body(result_body).unwrap())
    }
}
