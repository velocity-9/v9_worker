// These are just nice PORO (plain old rust objects) for modeling requests and responses

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ComponentPath {
    pub user: String,
    pub repo: String,
}

impl ComponentPath {
    pub fn new(user: String, repo: String) -> Self {
        Self { user, repo }
    }
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ComponentId {
    #[serde(flatten)]
    pub path: ComponentPath,
    pub hash: String,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ExecutionMethod {
    #[serde(rename = "docker-archive")]
    DockerArchive,
    #[serde(rename = "python-unsafe")]
    PythonUnsafe,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ActivationStatus {
    #[serde(rename = "activation-successful")]
    ActivationSuccessful,
    #[serde(rename = "already-running")]
    AlreadyRunning,
    #[serde(rename = "failed-to-find-executable")]
    FailedToFindExecutable,
    #[serde(rename = "failed-to-start")]
    FailedToStart,
    #[serde(rename = "invalid-request")]
    InvalidRequest,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ActivateRequest {
    pub id: ComponentId,
    pub executable_file: String,
    pub execution_method: ExecutionMethod,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ActivateResponse {
    pub result: ActivationStatus,
    pub dbg_message: String,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum DeactivationStatus {
    #[serde(rename = "component-not-found")]
    ComponentNotFound,
    #[serde(rename = "deactivation-successful")]
    DeactivationSuccessful,
    #[serde(rename = "failed-to-deactivate")]
    FailedToDeactivate,
    #[serde(rename = "invalid-request")]
    InvalidRequest,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct DeactivateRequest {
    pub id: ComponentId,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct DeactivateResponse {
    pub result: DeactivationStatus,
    pub dbg_message: String,
}

#[derive(Clone, Deserialize, Debug, PartialEq, Serialize)]
pub struct ComponentStats {
    pub stat_window_seconds: f64,

    pub hits: f64,

    pub avg_response_bytes: f64,
    pub avg_ms_latency: f64,
    pub ms_latency_percentiles: Vec<f64>,
}

#[derive(Clone, Deserialize, Debug, PartialEq, Serialize)]
pub struct ComponentStatus {
    pub id: ComponentId,
    #[serde(flatten)]
    pub component_stats: ComponentStats,
}

#[derive(Clone, Deserialize, Debug, PartialEq, Serialize)]
pub struct StatusResponse {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub network_usage: f64,
    pub active_components: Vec<ComponentStatus>,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ComponentRequest {
    pub called_function: String,

    pub http_method: String,
    pub path: String,
    pub request_arguments: String,
    pub request_body: String,
}

#[derive(Clone, Deserialize, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ComponentResponse {
    pub response_body: String,
    pub http_response_code: u32,
    pub error_message: Option<String>,
}
