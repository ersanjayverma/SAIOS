pub mod event;
pub mod handle;
pub mod id;
pub mod manager;
pub mod registry;
pub mod state;
pub mod r#trait;
pub mod types;

pub use event::{ObjectEvent, ObjectEventRecord};
pub use handle::ObjectHandle;
pub use id::ObjectId;
pub use registry::{KomStats, ObjectRecord};
pub use manager::{
    count, enumerate, events, find, find_by_name, find_by_type, init, inspect, register, stats,
    unregister,
};
pub use state::ObjectState;
pub use r#trait::KernelObject;
pub use types::ObjectType;
