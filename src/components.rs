use std::collections::HashMap;

use hyper::{Body, Method, Response};

use parking_lot::Mutex;

use crate::model::{
    ActivateRequest, ActivateResponse, ActivationStatus, ComponentId, ComponentPath, DeactivateRequest,
    DeactivateResponse, DeactivationStatus, StatusResponse,
};

#[derive(Debug)]
pub struct ComponentManager {
    active_components: HashMap<ComponentPath, Mutex<ComponentHandle>>,
}

impl ComponentManager {
    pub fn new() -> ComponentManager {
        ComponentManager {
            active_components: HashMap::new(),
        }
    }

    pub fn lookup_component(&self, path: &ComponentPath) -> Option<&Mutex<ComponentHandle>> {
        self.active_components.get(path)
    }

    pub fn activate(
        &mut self,
        activate_request: Result<ActivateRequest, serde_json::Error>,
    ) -> ActivateResponse {
        if let Err(e) = activate_request {
            return ActivateResponse {
                result: ActivationStatus::InvalidRequest,
                dbg_message: e.to_string(),
            };
        }

        // This is a safe unwrap, since we just checked if activate_request was in an error state
        let activate_request = activate_request.unwrap();

        if self.active_components.contains_key(&activate_request.id.path) {
            warn!(
                "Attempt to activate already activated component ({:?}) was foiled!",
                activate_request
            );
            return ActivateResponse {
                result: ActivationStatus::AlreadyRunning,
                dbg_message: "already running, redundant request!!".to_string(),
            };
        }

        self.active_components.insert(
            activate_request.id.path.clone(),
            Mutex::new(ComponentHandle {
                id: activate_request.id.clone(),
            }),
        );

        info!("Successfully activated a component ({:?})", activate_request);

        ActivateResponse {
            result: ActivationStatus::ActivationSuccessful,
            dbg_message: "successfully activated".to_string(),
        }
    }

    pub fn deactivate(
        &mut self,
        deactivate_request: Result<DeactivateRequest, serde_json::Error>,
    ) -> DeactivateResponse {
        if let Err(e) = deactivate_request {
            return DeactivateResponse {
                result: DeactivationStatus::InvalidRequest,
                dbg_message: e.to_string(),
            };
        }

        // This is a safe unwrap, since we just checked if activate_request was in an error state
        let deactivate_request = deactivate_request.unwrap();

        if !self.active_components.contains_key(&deactivate_request.id.path) {
            warn!(
                "Attempt to deactivate a non-active component ({:?}) was foiled!",
                deactivate_request
            );
            return DeactivateResponse {
                result: DeactivationStatus::ComponentNotFound,
                dbg_message: "deactivation failed, since the component was not activated".to_string(),
            };
        }

        // This is a safe unwrap, since we just checked that the map contains this key
        self.active_components
            .get_mut(&deactivate_request.id.path)
            .unwrap()
            .lock()
            .deactivate();
        self.active_components.remove(&deactivate_request.id.path);

        info!("Successfully activated a component ({:?})", deactivate_request);

        DeactivateResponse {
            result: DeactivationStatus::DeactivationSuccessful,
            dbg_message: "deactivation succesful".to_string(),
        }
    }

    // TODO: Try and just take a read lock for status / make it take &self instead of &mut self
    pub fn status(&mut self) -> StatusResponse {
        // TODO: Actually implement the status response
        warn!("Returning dummy status response, since stats are unimplemented");
        StatusResponse {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            network_usage: 0.0,
            active_components: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct ComponentHandle {
    id: ComponentId,
}

impl ComponentHandle {
    pub fn handle_component_call(
        &mut self,
        _component_method: &str,
        _http_verb: Method,
        _additional_path_components: &[&str],
        _query: String,
        _body: String,
    ) -> Response<Body> {
        // TODO: Implement component handling
        error!("Component calls not yet implemented!");
        Response::new(Body::from("{}"))
    }

    pub fn deactivate(&mut self) {
        warn!("Component deactivation currently a no-op...")
    }
}
