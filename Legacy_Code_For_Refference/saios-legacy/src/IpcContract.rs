//! Canonical IPC ownership authority.
//!
//! Individual IPC implementations own mechanics; this contract owns object
//! taxonomy, namespace attribution, and creation accounting.

use crate::resource_contract::{
    AccountableEntity, AttributionChain, ResourceContract, ResourceKind,
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcMechanism {
    AnonymousPipe = 1,
    NamedPipe = 2,
    UnixDomainSocket = 3,
    PosixMessageQueue = 4,
    PosixSemaphore = 5,
    SystemVSharedMemory = 6,
    SystemVSemaphore = 7,
    SystemVMessageQueue = 8,
    Ipec = 9,
    Futex = 10,
    Signal = 11,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcNamespaceKind {
    Root = 1,
    Container = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcNamespaceId {
    pub kind: IpcNamespaceKind,
    pub id: u64,
}

impl IpcNamespaceId {
    pub const ROOT: Self = Self {
        kind: IpcNamespaceKind::Root,
        id: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcCreateRequest {
    pub mechanism: IpcMechanism,
    pub namespace: IpcNamespaceId,
    pub buffer_bytes: u64,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcObjectGrant {
    pub mechanism: IpcMechanism,
    pub namespace: IpcNamespaceId,
    pub accountable: AccountableEntity,
    pub buffer_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcCapabilityView {
    pub contract_owner: bool,
    pub namespace_owner_metadata: bool,
    pub raf_creation_accounting: bool,
    pub anonymous_pipes: bool,
    pub named_pipes: bool,
    pub unix_socketpair: bool,
    pub unix_domain_sockets: bool,
    pub posix_message_queues: bool,
    pub posix_semaphores: bool,
    pub system_v_ipc: bool,
    pub ipec_channels: bool,
    pub futex_compatibility: bool,
    pub signal_delivery: bool,
    pub lifecycle_destroy_events: bool,
}

pub struct IpcContract;

impl IpcContract {
    pub const ANONYMOUS_PIPE_BUFFER_SIZE: u64 = 64 * 1024;

    pub fn capability_view() -> IpcCapabilityView {
        IpcCapabilityView {
            contract_owner: true,
            namespace_owner_metadata: true,
            raf_creation_accounting: true,
            anonymous_pipes: true,
            named_pipes: false,
            unix_socketpair: true,
            unix_domain_sockets: false,
            posix_message_queues: false,
            posix_semaphores: false,
            system_v_ipc: false,
            ipec_channels: false,
            futex_compatibility: true,
            signal_delivery: true,
            lifecycle_destroy_events: false,
        }
    }

    pub fn create_object(request: IpcCreateRequest) -> Result<IpcObjectGrant, &'static str> {
        let chain = AttributionChain::current();
        ResourceContract::charge(chain, ResourceKind::IpcObjects, 1)?;
        if let Err(err) =
            ResourceContract::charge(chain, ResourceKind::IpcBytes, request.buffer_bytes)
        {
            ResourceContract::release(chain.accountable, ResourceKind::IpcObjects, 1);
            return Err(err);
        }

        Ok(IpcObjectGrant {
            mechanism: request.mechanism,
            namespace: request.namespace,
            accountable: chain.accountable,
            buffer_bytes: request.buffer_bytes,
        })
    }

    pub fn create_anonymous_pipe() -> Result<IpcObjectGrant, &'static str> {
        Self::create_object(IpcCreateRequest {
            mechanism: IpcMechanism::AnonymousPipe,
            namespace: IpcNamespaceId::ROOT,
            buffer_bytes: Self::ANONYMOUS_PIPE_BUFFER_SIZE,
            tag: "pipe.create",
        })
    }
}
