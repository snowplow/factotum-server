// Copyright (c) 2017-2021 Snowplow Analytics Ltd. All rights reserved.
//
// This program is licensed to you under the Apache License Version 2.0, and
// you may not use this file except in compliance with the Apache License
// Version 2.0.  You may obtain a copy of the Apache License Version 2.0 at
// http://www.apache.org/licenses/LICENSE-2.0.
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the Apache License Version 2.0 is distributed on an "AS
// IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
// implied.  See the Apache License Version 2.0 for the specific language
// governing permissions and limitations there under.
//

use std::fmt;
use std::panic;
use std::thread::Result as ThreadResult;
use consul::Client;
use serde_json;
use base64::decode;

use factotum_server::server::JobRequest;

#[cfg(test)]
mod tests;

pub trait Persistence {
    fn id(&self) -> &str;
    fn set_key(&self, key: &str, value: &str) -> ThreadResult<()>;
    fn get_key(&self, key: &str) -> ThreadResult<Option<String>>;
    fn prepend_namespace(&self, key: &str) -> String;
}

#[derive(Clone, Debug)]
pub struct ConsulPersistence {
    server_id: String,
    host: String,
    port: u32,
    namespace: String,
}

impl ConsulPersistence {
    pub fn new(wrapped_id: Option<String>, wrapped_host: Option<String>, wrapped_port: Option<u32>, wrapped_namespace: Option<String>) -> ConsulPersistence {
        ConsulPersistence {
            server_id: if let Some(server_id) = wrapped_id { server_id } else { ::CONSUL_NAME_DEFAULT.to_string() },
            host: if let Some(host) = wrapped_host { host } else { ::CONSUL_IP_DEFAULT.to_string() },
            port: if let Some(port) = wrapped_port { port } else { ::CONSUL_PORT_DEFAULT },
            namespace: if let Some(namespace) = wrapped_namespace { namespace } else { ::CONSUL_NAMESPACE_DEFAULT.to_string() },
        }
    }

    fn client(&self) -> Client {
        Client::new(&format!("{}:{}", self.host.clone(), self.port.clone()))
    }
}

impl Persistence for ConsulPersistence {
    fn id(&self) -> &str {
        &self.server_id
    }

    fn set_key(&self, key: &str, value: &str) -> ThreadResult<()> {
        panic::catch_unwind(|| {
            self.client().keystore.set_key(key.to_owned(), value.to_owned())
        })
    }

    fn get_key(&self, key: &str) -> ThreadResult<Option<String>> {
        panic::catch_unwind(|| {
            self.client().keystore.get_key(key.to_owned())
        })
    }

    fn prepend_namespace(&self, job_ref: &str) -> String {
        apply_namespace_if_absent(&self.namespace, job_ref)
    }
}

pub fn set_entry<T: Persistence>(persistence: &T, job_ref: &str, job_request: &JobRequest, state: &JobState, outcome: &JobOutcome) -> bool
{
    let job_entry = JobEntry::new(state, job_request, persistence.id(), outcome);
    let job_entry_json = serde_json::to_string(&job_entry).expect("JSON compact encode error");

    let job_key = persistence.prepend_namespace(job_ref);
    let result = persistence.set_key(&job_key, &job_entry_json);

    match result {
        Ok(_) => true,
        Err(_) => {
            error!("Persistence Error: could not set K/V: {}::{}", job_key, job_entry_json);
            false
        },
    }
}

pub fn get_entry<T: Persistence>(persistence: &T, job_ref: &str) -> Option<JobEntry> {
    let job_key = persistence.prepend_namespace(job_ref);
    let result = persistence.get_key(&job_key);

    let keystore_val = match result {
        Ok(state) => state,
        Err(_) => {
            error!("Persistence Error: could not get key: {}", job_ref);
            None
        },
    };

    // decode base64 string
    // deserialize to JobEntry
    if let Some(base64_str) = keystore_val {
        let decode_result = &decode(&base64_str).expect("Base64 string decode error");
        let raw_value = ::std::str::from_utf8(decode_result).expect("Error converting from bytes to string");
        let job_entry: JobEntry = serde_json::from_str(raw_value).expect("JSON decode error");
        Some(job_entry)
    } else {
        None
    }
}

pub fn apply_namespace_if_absent(namespace: &str, id: &str) -> String {
    if id.starts_with(namespace) {
        id.to_owned()
    } else {
        format!("{}/{}", namespace, id)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum JobState {
    QUEUED,
    WORKING,
    DONE,
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum JobOutcome {
    SUCCEEDED,
    FAILED,
    RUNNING,
    WAITING,
}

impl fmt::Display for JobOutcome {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEntry {
    pub state: JobState,
    pub job_request: JobRequest,
    pub last_run_from: String,
    pub last_outcome: JobOutcome,
}

impl JobEntry {
    pub fn new(state: &JobState, request: &JobRequest, server_id: &str, outcome: &JobOutcome) -> JobEntry {
        JobEntry {
            state: state.to_owned(),
            job_request: request.to_owned(),
            last_run_from: server_id.to_owned(),
            last_outcome: outcome.to_owned(),
        }
    }
}
