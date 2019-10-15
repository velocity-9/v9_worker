mod isolation;
mod stats;

use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::time::Instant;

use hyper::{Body, Method, Response};
use parking_lot::Mutex;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use systemstat::{Platform, System};

use crate::component::isolation::IsolatedProcessWrapper;
use crate::component::stats::StatTracker;
use crate::error::WorkerError;
use crate::model::{
    ActivateRequest, ActivateResponse, ActivationStatus, ComponentId, ComponentPath, ComponentRequest,
    ComponentResponse, ComponentStatus, DeactivateRequest, DeactivateResponse, DeactivationStatus,
    StatusResponse,
};

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
    pub fn new() -> Self {
        Self {
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

        let isolated_process_wrapper = match IsolatedProcessWrapper::new(activate_request.clone()) {
            Ok(w) => w,
            Err(e) => {
                return ActivateResponse {
                    result: ActivationStatus::FailedToStart,
                    dbg_message: e.to_string(),
                }
            }
        };

        self.active_components.insert(
            activate_request.id.path.clone(),
            Mutex::new(ComponentHandle {
                id: activate_request.id.clone(),
                component_process_wrapper: isolated_process_wrapper,
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
            .map(|mem| 1.0 - mem.total.as_u64() as f64 / mem.free.as_u64() as f64)
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
    component_process_wrapper: IsolatedProcessWrapper,
}

impl ComponentHandle {
    pub fn handle_component_call(
        &mut self,
        component_method: &str,
        http_verb: &Method,
        additional_path_components: &[&str],
        query: String,
        body: String,
    ) -> Result<Response<Body>, WorkerError> {
        let start = Instant::now();

        let request = ComponentRequest {
            called_function: component_method.to_string(),

            http_method: http_verb.to_string(),
            path: additional_path_components.join("/"),
            request_arguments: query,
            request_body: body,
        };

        // Our communication with subprocesses has protocol calls for one percent encoded JSON per request/response
        // We handle this deserialization here to keep it general
        let serialized_request = serde_json::to_string(&request)?;
        let encoded_request = utf8_percent_encode(&serialized_request, NON_ALPHANUMERIC);

        let encoded_response = self
            .component_process_wrapper
            .query_process(&encoded_request.to_string())?;
        let serialized_response = percent_decode_str(&encoded_response).decode_utf8()?.to_string();
        let response: ComponentResponse = serde_json::from_str(&serialized_response)?;

        debug!("Got component response {:?}", response);

        if let Some(m) = response.error_message {
            if !m.is_empty() {
                let resp = Response::builder()
                    .status(response.http_response_code as u16)
                    .body(Body::from(m))
                    .unwrap();
                return Ok(resp);
            }
        }

        let resp_code = response.http_response_code;
        let resp_body = response.response_body;
        let response_bytes = resp_body.len();
        let resp = Response::builder()
            .status(resp_code as u16)
            .body(Body::from(resp_body))
            .unwrap();

        let processing_duration = start.elapsed();
        self.stat_tracker
            .add_stat_event(processing_duration.as_millis() as u32, response_bytes as u32);

        Ok(resp)
    }

    pub fn get_component_status(&mut self) -> ComponentStatus {
        let component_stats = self.stat_tracker.get_component_stats();

        ComponentStatus {
            id: self.id.clone(),
            component_stats,
        }
    }
}
