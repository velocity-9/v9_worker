use std::str;
use std::sync::Arc;

use futures_util::stream::StreamExt;
use hyper::{Body, Method, Request, Response, StatusCode, Uri};
use parking_lot::RwLock;

use crate::component::ComponentManager;
use crate::error::{WorkerError, WorkerErrorKind};
use crate::model::{ComponentPath, StatusColor};

// Warning: This method is somewhat complicated, since it needs to deal with async stuff
// There should be no state here beyond the handler, so no need for an actual hyper service
// (We don't want to lock into hyper that hard anyway)
pub async fn global_request_entrypoint(
    handler: Arc<HttpRequestHandler>,
    req: Request<Body>,
) -> Result<Response<Body>, hyper::error::Error> {
    debug!("{:?}", req);

    // Pull the verb, uri, and query stuff out of the request
    // (It's okay to do this, since it's all quite quick to execute)
    let http_verb = req.method().clone();
    let uri = req.uri().clone();
    let query = uri.query().unwrap_or("").to_string();

    // Then get a future representing the body (this is a future, since hyper may not of received the whole body yet)
    let hyper_body = req.into_body();
    let body_result = hyper_body
        .fold(Ok(String::new()), |fold, byte_result| {
            async {
                let mut acc = fold?;
                let bytes = byte_result?;

                let body_part = str::from_utf8(&bytes)?;
                acc.push_str(body_part);

                Ok::<String, WorkerError>(acc)
            }
        })
        .await;

    let resp = body_result
        .and_then(|body| {
            debug!("body = {:?}", body);

            // Delegate to the handler to actually deal with this request
            // NOTE: We cannot handle panics here, since it could leave the handler in an inconsistent state
            // Better to just bomb out
            // TODO: Investigate handling panics at a lower level
            handler.handle(http_verb, &uri, query, body)
        })
        .unwrap_or_else(|e| {
            warn!("Forced to convert error {:?} into a http response", e);
            e.into()
        });

    if resp.status() == StatusCode::INTERNAL_SERVER_ERROR {
        error!("INTERNAL SERVER ERROR -- {:?}", resp);
    } else {
        debug!("{:?}", resp);
    }

    Ok(resp)
}

#[derive(Debug)]
pub struct HttpRequestHandler {
    serverless_component_manager: RwLock<ComponentManager>,
}

#[allow(clippy::unused_self)]
impl HttpRequestHandler {
    pub fn new() -> Self {
        Self {
            serverless_component_manager: RwLock::new(ComponentManager::new()),
        }
    }

    // TODO: Make async and pipe down
    fn handle(
        &self,
        http_verb: Method,
        uri: &Uri,
        query: String,
        body: String,
    ) -> Result<Response<Body>, WorkerError> {
        // Get the uri path, and then split it around slashes into components
        // Note: All URIs start with a slash, so we skip the first entry in the split (which is always just "")
        let path_components: Vec<&str> = uri.path().split('/').skip(1).collect();
        debug!("path = {:?}", path_components);

        if path_components[0] == "meta" && path_components.len() == 2 {
            self.handle_meta_request(
                &self.serverless_component_manager,
                http_verb,
                path_components[1],
                &body,
            )
        } else if path_components[0] == "sl" && path_components.len() >= 4 {
            let component_router = self.serverless_component_manager.read();

            debug!("Starting serverless request processing...");
            let user = path_components[1].to_string();
            let repo = path_components[2].to_string();
            let method = path_components[3];

            let path = ComponentPath::new(user, repo);
            let component = component_router.lookup_component(&path);

            let resp = component.map_or_else(
                || {
                    warn!("Could not find serverless component {:?}", path);
                    Err(WorkerErrorKind::PathNotFound(path_components.join("/")).into())
                },
                |component_handle| {
                    let mut locked_handle = component_handle.lock();
                    let call_resp = locked_handle.handle_component_call(
                        method,
                        &http_verb,
                        &path_components[4..],
                        query,
                        body,
                    );

                    let color = match &call_resp {
                        Ok(resp) => {
                            if resp.status().is_success() || resp.status().is_redirection() {
                                StatusColor::Green
                            } else if resp.status().is_server_error() || resp.status() == 543 {
                                StatusColor::Red
                            } else {
                                // Covers `resp.status().is_client_error()`
                                StatusColor::Orange
                            }
                        }
                        Err(_) => StatusColor::Red,
                    };
                    locked_handle.set_color(color);

                    call_resp
                },
            );

            trace!("Finished serverless request processing... ({:?})", resp);

            resp
        } else {
            Err(WorkerErrorKind::PathNotFound(path_components.join("/")).into())
        }
    }

    // TODO: Refactor to associated function
    fn handle_meta_request(
        &self,
        component_router: &RwLock<ComponentManager>,
        http_verb: Method,
        route: &str,
        body: &str,
    ) -> Result<Response<Body>, WorkerError> {
        let result_body = Body::from(match (route, http_verb) {
            ("activate", Method::POST) => {
                let resp = component_router.write().activate(serde_json::from_str(body));
                serde_json::to_string(&resp)?
            }
            ("deactivate", Method::POST) => {
                let resp = component_router.write().deactivate(serde_json::from_str(body));
                serde_json::to_string(&resp)?
            }
            ("status", Method::GET) => {
                let resp = component_router.read().status();
                serde_json::to_string(&resp)?
            }

            ("activate", _) | ("deactivate", _) | ("status", _) => {
                return Err(WorkerErrorKind::WrongMethod.into())
            }
            _ => return Err(WorkerErrorKind::PathNotFound("meta/".to_string() + route).into()),
        });
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(result_body)
            .unwrap())
    }

    pub fn component_manager(&self) -> &RwLock<ComponentManager> {
        &self.serverless_component_manager
    }
}
