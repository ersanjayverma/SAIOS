use alloc::string::String;
use alloc::vec::Vec;

use crate::object_manager;
use crate::provider::ProviderType;
use crate::saifs::{self, Event, EventType, SaifsError};
use crate::som::{ObjectId, ProviderId};
use crate::snom::{abi_version, AbiVersion};

pub fn init() {
    object_manager::init();
    saifs::init();
}

pub fn snom_abi_version() -> AbiVersion {
    abi_version()
}

pub fn query(expr: &str) -> Result<Vec<String>, &'static str> {
    object_manager::query(expr)
}

#[derive(Clone)]
pub struct ProviderView {
    pub id: ProviderId,
    pub name: String,
    pub provider_type: ProviderType,
    pub namespace: String,
}

pub fn providers() -> Vec<ProviderView> {
    object_manager::providers()
        .into_iter()
        .map(|p| ProviderView {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            namespace: p.namespace,
        })
        .collect()
}

pub fn open(path: &str) -> Result<saifs::SaifsHandle, SaifsError> {
    saifs::open(path)
}

pub fn list(path: &str) -> Result<Vec<String>, SaifsError> {
    saifs::list(path)
}

pub fn read_text(path: &str) -> Result<String, SaifsError> {
    saifs::read_text(path)
}

pub fn publish_event(event_type: EventType, object: Option<ObjectId>, payload: &str) {
    saifs::publish_event(event_type, object, payload);
}

pub fn events(limit: usize) -> Vec<Event> {
    saifs::events(limit)
}
