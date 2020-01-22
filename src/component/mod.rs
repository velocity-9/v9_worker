mod isolation;
mod logs;
mod stats;

use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{self, Debug, Formatter};
use std::time::Instant;

use hyper::{Body, Method, Response};
use parking_lot::Mutex;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use systemstat::{Platform, System};

use crate::component::isolation::IsolatedProcessWrapper;
use crate::component::logs::LogTracker;
use crate::component::stats::StatTracker;
use crate::error::WorkerError;
use crate::model::{
    ActivateRequest, ActivateResponse, ActivationStatus, ComponentId, ComponentLog, ComponentPath,
    ComponentRequest, ComponentResponse, ComponentStatus, DeactivateRequest, DeactivateResponse,
    DeactivationStatus, LogResponse, StatusColor, StatusResponse,
};

pub use crate::component::logs::LogPolicy;

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

    // TODO: Activate and Deactivate should respect the hash passsed in, instead of blindly deactivating anything

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
                log_tracker: LogTracker::new(),
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

        // This is a safe unwrap, since we just checked if deactivate_request was in an error state
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

        info!("Successfully deactivated a component ({:?})", deactivate_request);

        DeactivateResponse {
            result: DeactivationStatus::DeactivationSuccessful,
            dbg_message: "deactivation succesful".to_string(),
        }
    }

    pub fn logs(&self) -> LogResponse {
        let logs = self
            .active_components
            .values()
            .map(|component| {
                let mut locked_component = component.lock();
                locked_component.get_component_log()
            })
            .collect();

        LogResponse { logs }
    }

    pub fn status(&self) -> StatusResponse {
        debug!("Processing status request by looking up system averages...");

        let cpu_usage = self
            .system
            .load_average()
            .map(|avg| f64::from(avg.one) / 100.0)
            .map_err(|e| {
                warn!("Could not get cpu usage {}", e);
                e
            })
            .unwrap_or(-1.0);

        let memory_usage = self
            .system
            .memory()
            .map(|mem| 1.0 - mem.free.as_u64() as f64 / mem.total.as_u64() as f64)
            .map_err(|e| {
                warn!("Could not get memory usage {}", e);
                e
            })
            .unwrap_or(-1.0);

        // TODO: I believe this returns error rate since bootup. It should only cover the last minute or so
        let packets_data = self.system.networks().map(|networks| {
            networks
                .values()
                .flat_map(|network| {
                    let stats = self.system.network_stats(&network.name);
                    if let Err(e) = &stats {
                        warn!(
                            "Could not get network stats for network {}. err: {}",
                            network.name, e
                        )
                    }
                    stats.ok()
                })
                .fold((0, 0), |(total_packets, failed_packets), stats| {
                    (
                        total_packets + stats.tx_packets + stats.rx_packets,
                        failed_packets + stats.tx_errors + stats.rx_errors,
                    )
                })
        });

        let network_usage = match packets_data {
            Ok((0, _)) => {
                warn!("Zero packet info available!");
                -1.0
            }
            Ok((total_packets, failed_packets)) => failed_packets as f64 / total_packets as f64,
            Err(e) => {
                warn!("Could not get network usage {}", e);
                -1.0
            }
        };

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

    // The heartbeat function is called periodically
    pub fn heartbeat(&self) {
        for component in self.active_components.values() {
            // It's okay not to block on the lock -- heartbeats have no guaranteed periodicity
            // (Plus, this is only used for component shutdown, if someone has this lock, the
            // component  is clearly still in use)
            if let Some(mut handle) = component.try_lock() {
                handle.heartbeat()
            }
        }
    }
}

#[derive(Debug)]
pub struct ComponentHandle {
    id: ComponentId,

    component_process_wrapper: IsolatedProcessWrapper,

    log_tracker: LogTracker,
    stat_tracker: StatTracker,
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

        debug!("Firing component request {:?}", request);

        // Our communication with subprocesses has protocol calls for one percent encoded JSON per request/response
        // We handle this deserialization here to keep it general
        let serialized_request = serde_json::to_string(&request)?;
        let encoded_request = utf8_percent_encode(&serialized_request, NON_ALPHANUMERIC);

        let encoded_response = self
            .component_process_wrapper
            .query_process(&encoded_request.to_string(), &mut self.log_tracker)?;
        let serialized_response = percent_decode_str(&encoded_response).decode_utf8()?.to_string();
        let response: ComponentResponse = serde_json::from_str(&serialized_response)?;

        debug!("Got component response {:?}", response);

        let resp_code: u16 = response.http_response_code.try_into()?;

        if let Some(m) = response.error_message {
            if !m.is_empty() {
                let resp = Response::builder().status(resp_code).body(Body::from(m)).unwrap();
                return Ok(resp);
            }
        }

        let resp_body = response.response_body;
        let response_bytes = resp_body.len();
        let resp = Response::builder()
            .status(resp_code)
            .body(Body::from(resp_body))
            .unwrap();

        let processing_duration = start.elapsed();
        self.stat_tracker.add_stat_event(
            processing_duration.as_millis().try_into()?,
            response_bytes.try_into()?,
        );

        Ok(resp)
    }

    pub fn get_component_status(&mut self) -> ComponentStatus {
        let component_stats = self.stat_tracker.get_component_stats();

        ComponentStatus {
            id: self.id.clone(),
            component_stats,
        }
    }

    pub fn get_component_log(&mut self) -> ComponentLog {
        let (dedup_number, log) = self.log_tracker.get_contents();

        match log {
            Ok(log) => ComponentLog {
                id: self.id.clone(),

                dedup_number,
                log,
                error: None,
            },
            Err(e) => {
                let err_msg = format!("Failure to get logs for component {:?}, err {}", self, e);
                warn!("{}", err_msg);
                ComponentLog {
                    id: self.id.clone(),

                    dedup_number,
                    log: None,

                    error: Some(err_msg),
                }
            }
        }
    }

    pub fn set_color(&mut self, color: StatusColor) {
        self.stat_tracker.set_color(color)
    }

    // The heartbeat function is called periodically
    pub fn heartbeat(&mut self) {
        self.component_process_wrapper.heartbeat()
    }
}
