use std::collections::HashMap;

use hyper::{Body, Method, Response};

use crate::model::{
    ActivateRequest, ActivateResponse, ActivationStatus, ComponentId, ComponentPath,
    DeactivateRequest, DeactivateResponse, DeactivationStatus, StatusResponse,
};

#[derive(Debug)]
pub struct ComponentManager {
    // TODO: Refactor so that multiple threads can be messing with this at the same time
    active_components: HashMap<ComponentPath, ComponentHandle>,
}

impl ComponentManager {
    pub fn new() -> ComponentManager {
        ComponentManager {
            active_components: HashMap::new(),
        }
    }

    pub fn lookup_component(&mut self, path: &ComponentPath) -> Option<&mut ComponentHandle> {
        self.active_components.get_mut(path)
    }

    pub fn activate(&mut self, activate_request: ActivateRequest) -> ActivateResponse {
        if self
            .active_components
            .contains_key(&activate_request.id.path)
        {
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
            ComponentHandle {
                id: activate_request.id.clone(),
            },
        );

        info!(
            "Successfully activated a component ({:?})",
            activate_request
        );

        ActivateResponse {
            result: ActivationStatus::ActivationSuccessful,
            dbg_message: "successfully activated".to_string(),
        }
    }

    pub fn deactivate(&mut self, deactivate_request: DeactivateRequest) -> DeactivateResponse {
        if !self
            .active_components
            .contains_key(&deactivate_request.id.path)
        {
            warn!(
                "Attempt to deactivate a non-active component ({:?}) was foiled!",
                deactivate_request
            );
            return DeactivateResponse {
                result: DeactivationStatus::ComponentNotFound,
                dbg_message: "deactivation failed, since the component was not activated"
                    .to_string(),
            };
        }

        // This is a safe unwrap, since we just checked that the map contains this key
        self.active_components
            .get_mut(&deactivate_request.id.path)
            .unwrap()
            .deactivate();
        self.active_components.remove(&deactivate_request.id.path);

        info!(
            "Successfully activated a component ({:?})",
            deactivate_request
        );

        DeactivateResponse {
            result: DeactivationStatus::DeactivationSuccessful,
            dbg_message: "deactivation succesful".to_string(),
        }
    }

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
