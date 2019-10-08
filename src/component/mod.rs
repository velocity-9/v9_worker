use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::time::Instant;

use hyper::{Body, Method, Response};
use parking_lot::Mutex;
use systemstat::{Platform, System};

use crate::component::stats::StatTracker;
use crate::model::{
    ActivateRequest, ActivateResponse, ActivationStatus, ComponentId, ComponentPath, ComponentStatus,
    DeactivateRequest, DeactivateResponse, DeactivationStatus, StatusResponse,
};

mod stats;

pub struct ComponentManager {
    system: System,
    // Invariant: No method without exclusive access (&mut self) can lock multiple components at a time
    // (Otherwise deadlock is possible)
    active_components: HashMap<ComponentPath, Mutex<ComponentHandle>>,
}

impl Debug for ComponentManager {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentManager")
            .field("system", &"[unable to format this]")
            .field("active_components", &self.active_components)
            .finish()
    }
}

impl ComponentManager {
    pub fn new() -> ComponentManager {
        ComponentManager {
            system: System::new(),
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
                stat_tracker: StatTracker::default(),
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

    pub fn status(&self) -> StatusResponse {
        debug!("Processing status request by looking up system averages...");
        let cpu_usage = f64::from(
            self.system
                .load_average()
                .map(|avg| avg.one / 100.0)
                .unwrap_or(-1.0),
        );
        let memory_usage = self
            .system
            .memory()
            .map(|mem| 1.0 - mem.total.as_usize() as f64 / mem.free.as_usize() as f64)
            .unwrap_or(-1.0);
        // TODO: Actually implement network usage
        let network_usage = -1.0;

        let active_components = self
            .active_components
            .values()
            .map(|component_handle| component_handle.lock().get_component_status())
            .collect();

        StatusResponse {
            cpu_usage,
            memory_usage,
            network_usage,
            active_components,
        }
    }
}

#[derive(Debug)]
pub struct ComponentHandle {
    id: ComponentId,
    stat_tracker: StatTracker,
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
        let start = Instant::now();

        // TODO: Implement component calls
        error!("Component calls not yet implemented!");
        let resp_code = 200;
        let resp_body = "{}";
        let resp = Response::builder()
            .status(resp_code)
            .body(Body::from(resp_body))
            .unwrap();

        let processing_duration = start.elapsed();
        let response_bytes = resp_body.len();
        self.stat_tracker
            .add_stat_event(processing_duration.as_millis() as u32, response_bytes as u32);

        resp
    }

    pub fn deactivate(&mut self) {
        warn!("Component deactivation currently a no-op...")
    }

    pub fn get_component_status(&mut self) -> ComponentStatus {
        let component_stats = self.stat_tracker.get_component_stats();

        ComponentStatus {
            id: self.id.clone(),
            component_stats,
        }
    }
}
